use std::{net, env, fs, time};
use std::borrow::Cow;
use core::fmt;

use crate::Uri;

//Special environment variables set by kuberentes for pod to access internal API
const KUBERNETES_SERVICE_HOST: &str = "KUBERNETES_SERVICE_HOST";
const KUBERNETES_SERVICE_PORT: &str = "KUBERNETES_SERVICE_PORT";

//Special files provided by kubernetes within pod environment
const SERVICE_TOKENFILE: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const SERVICE_CERTFILE: &str = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";
const SERVICE_DEFAULT_NS: &str = "/var/run/secrets/kubernetes.io/serviceaccount/namespace";

#[derive(Debug)]
///Possible errors reading kube config
pub enum KubeError {
    ///Missing env variables KUBERNETES_SERVICE_HOST
    MissingServiceHost,
    ///Missing env variables KUBERNETES_SERVICE_PORT
    MissingServicePort,
    ///Service port is not valid port
    InvalidServicePort,
    ///Unable to construct valid URI out of host and port
    InvalidServiceUri(ureq::http::Error),
    ///Unable to read ca.crt
    UnableReadCert,
    ///ca.crt is not valid PEM file
    InvalidCert,
    ///namespace is not valid utf-8 file
    UnableReadNamespace,
    ///token is not valid utf-8 file
    UnableReadToken,
}

impl fmt::Display for KubeError {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingServiceHost => fmt.write_fmt(format_args!("env::{KUBERNETES_SERVICE_HOST} is missing")),
            Self::MissingServicePort => fmt.write_fmt(format_args!("env::{KUBERNETES_SERVICE_PORT} is missing")),
            Self::InvalidServicePort => fmt.write_fmt(format_args!("env::{KUBERNETES_SERVICE_PORT} is not valid port")),
            Self::InvalidServiceUri(error) => fmt.write_fmt(format_args!("Unable to construct valid URI: {error}")),
            Self::UnableReadCert => fmt.write_fmt(format_args!("{SERVICE_CERTFILE}: failed to read")),
            Self::InvalidCert => fmt.write_fmt(format_args!("{SERVICE_CERTFILE}: not a valid PEM certificate")),
            Self::UnableReadNamespace => fmt.write_fmt(format_args!("{SERVICE_DEFAULT_NS}: not a valid utf-8 file")),
            Self::UnableReadToken => fmt.write_fmt(format_args!("{SERVICE_TOKENFILE}: not a valid utf-8 file")),
        }
    }
}

#[inline(always)]
fn read_token(path: &str) -> Result<String, KubeError> {
    fs::read_to_string(path).map_err(|_| KubeError::UnableReadToken)
}

///Local cluster auth token read from mounted file system
///
///Token is re-used for at most 1 minute, and unless file is missing, will be reloaded every time after 1 minute
///If it is impossible to refresh token, then existing token will be used (actual token validity is within 10 minutes)
pub struct ClusterToken {
    file: Cow<'static, str>,
    token: String,
    last_fetched_at: time::Instant,
}

impl ClusterToken {
    #[inline(always)]
    ///Creates new [ClusterToken] by performing initial fetch of the token at `file` location
    ///
    ///Returns error if unable to fetch.
    ///
    ///After this, `file` is assumed to be always valid, but if re-fetch fails, it will use
    ///existing token
    pub fn new_token(file: Cow<'static, str>) -> Result<Self, KubeError> {
        let token = read_token(file.as_ref())?;
        Ok(Self {
            file,
            token,
            last_fetched_at: time::Instant::now()
        })
    }

    #[inline(always)]
    ///Checks if token is expired, returning `true` if that's the case
    pub fn is_expired(&self) -> bool {
        self.last_fetched_at.elapsed() >= time::Duration::from_secs(60)
    }

    ///Force refresh token, returning `false` if failed to read file
    pub fn refresh(&mut self) -> bool {
        if let Ok(new_token) = read_token(self.file.as_ref()) {
            self.token = new_token;
            true
        } else {
            false
        }
    }

    #[inline(always)]
    ///Requests to perform token refresh if it expires
    ///
    ///Returns `true` if token has been refreshed
    pub fn refresh_if_expired(&mut self) -> bool {
        if self.is_expired() {
            self.refresh()
        } else {
            false
        }
    }

    #[inline(always)]
    ///Returns current token
    pub fn token(&self) -> &str {
        self.token.as_str()
    }
}

fn build_kube_uri(host: &str, port: u16) -> Result<Uri, KubeError> {
    let uri = Uri::builder().scheme("https");
    let uri = match host.parse::<net::IpAddr>() {
        Ok(ip) => {
            if port == 443 {
                if ip.is_ipv6() {
                    let host = format!("[{ip}]");
                    uri.authority(host.as_str())
                } else {
                    uri.authority(host)
                }
            } else {
                let addr = net::SocketAddr::new(ip, port);
                uri.authority(addr.to_string().as_str())
            }
        }
        Err(_) => {
            if port == 443 {
                uri.authority(host)
            } else {
                let host = format!("{host}:{port}");
                uri.authority(host.as_str())
            }
        }
    };

    uri.build().map_err(|error| KubeError::InvalidServiceUri(error))
}

///Kubernetes API config
pub struct KubeConfig {
    pub(crate) uri: Uri,
    pub(crate) certs: Vec<ureq::tls::Certificate<'static>>,
    pub(crate) namespace: String,
    pub(crate) auth_token: ClusterToken,
}

impl KubeConfig {
    ///Creates kubeconfig based on in-cluster environment
    ///
    ///Returns `None` if environment is not valid or missing some environment variables
    pub fn in_cluster_env() -> Result<Self, KubeError> {
        let host = env::var(KUBERNETES_SERVICE_HOST).map_err(|_| KubeError::MissingServiceHost)?;
        let port: u16 = env::var(KUBERNETES_SERVICE_PORT).map_err(|_| KubeError::MissingServicePort).and_then(|port| port.parse().map_err(|_| KubeError::InvalidServicePort))?;
        let uri = build_kube_uri(&host, port)?;
        let cert = fs::read(SERVICE_CERTFILE).map_err(|_| KubeError::UnableReadCert)?;
        let certs = ureq::tls::parse_pem(&cert).filter_map(|pem| match pem {
            Ok(ureq::tls::PemItem::Certificate(cert)) => Some(cert),
            _ => None
        }).collect();

        let auth_token = ClusterToken::new_token(SERVICE_TOKENFILE.into())?;
        let namespace = fs::read_to_string(SERVICE_DEFAULT_NS).map_err(|_| KubeError::UnableReadNamespace)?;
        let result = Self {
            uri,
            certs,
            namespace,
            auth_token,
        };

        if result.certs.is_empty() {
            return Err(KubeError::InvalidCert);
        }

        Ok(result)
    }
}

#[derive(Copy, Clone)]
///General HTTP config
pub struct HttpConfig {
    pub(crate) timeout: time::Duration,
}

impl HttpConfig {
    #[inline]
    ///Creates new default config
    pub const fn new() -> Self {
        Self {
            timeout: time::Duration::from_secs(10)
        }
    }

    #[inline]
    ///Sets `timeout` on requests.
    ///
    ///Defaults to `10s`
    pub const fn with_timeout(mut self, timeout: time::Duration) -> Self {
        self.timeout = timeout;
        self
    }
}
