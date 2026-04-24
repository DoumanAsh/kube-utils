//!Minimal client for kubernetes internal API
//!
//!If you need full fledged API client, then prefer to use [kube-client](https://crates.io/crates/kube-client)

#![warn(missing_docs)]
#![allow(clippy::style)]

use std::{io, env};
use std::sync::RwLock;

pub mod data;
mod config;
pub use config::*;

pub use ureq::http::Uri;

const MAX_BODY_SIZE: u64 = 10 * 1024 * 1024;

///Returns pod name using following environment variables:
///- POD_NAME
///- HOSTNAME
///
///Returns None, if neither of variable available
pub fn pod_name() -> Option<String> {
    env::var("POD_NAME").or_else(|_| env::var("HOSTNAME")).ok()
}

struct State {
    auth_token: RwLock<ClusterToken>,
    namespace: String,
    uri: Uri,
}

#[derive(Clone)]
///Client for internal API (i.e. to be used within cluster)
pub struct Client {
    http: ureq::Agent,
    state: std::sync::Arc<State>
}

impl Client {
    ///Creates new client
    pub fn new(http: HttpConfig, KubeConfig { auth_token, uri, certs, namespace }: KubeConfig) -> Self {
        let state = State {
            auth_token: RwLock::new(auth_token),
            uri,
            namespace
        };

        let ca = ureq::tls::RootCerts::Specific(std::sync::Arc::new(certs));
        let tls_config = ureq::tls::TlsConfig::builder().use_sni(false).root_certs(ca).build();
        let http = ureq::Agent::config_builder().timeout_per_call(Some(http.timeout)).tls_config(tls_config).build();
        let http = ureq::Agent::new_with_config(http);
        Self {
            http,
            state: std::sync::Arc::new(state)
        }
    }

    ///Gets pod information in the same namespace as current client
    pub fn get_pod(&self, pod_name: &str) -> Result<data::Pod, ureq::Error> {
        let auth_token = self.state.auth_token.read().expect("internal error");
        let bearer = if auth_token.is_expired() {
            drop(auth_token);
            let mut auth_token = self.state.auth_token.write().expect("internal error");
            auth_token.refresh();
            format!("Bearer {}", auth_token.token())
        } else {
            let result = format!("Bearer {}", auth_token.token());
            drop(auth_token);
            result
        };

        let uri = format!("{}/api/v1/namespaces/{}/pods/{pod_name}", self.state.uri, self.state.namespace);
        let response = self.http.get(&uri).header("Authorization", bearer).call()?;
        let body = response.into_body().into_with_config().limit(MAX_BODY_SIZE).reader();
        serde_json::from_reader(body).map_err(|error| {
            if error.is_io() || error.is_eof() {
                ureq::Error::Io(io::Error::other(error))
            } else {
                ureq::Error::Other(Box::new(error))
            }
        })
    }
}
