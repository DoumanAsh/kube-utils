//!Minimal data model to allow extraction of only useful information (for the purposes of this crate)

#[derive(serde_derive::Deserialize)]
///Common metadata for kubernetes objects
pub struct Metadata {
    ///Name of the pod within namespace
    pub name: String,
    #[serde(default)]
    ///Namespace of the object. Can be empty for default namespace
    pub namespace: String,
    #[serde(default)]
    ///Unique identifier of the object, unchanged once object is created.
    ///
    ///Normally it is UUID
    pub uid: String,
}

#[derive(serde_derive::Deserialize)]
///Container object definition
pub struct Container {
    ///Name of the container
    pub name: String,
    ///Image used by the container
    pub image: String,
}

#[derive(Default, serde_derive::Deserialize)]
///Pod spec
pub struct PodSpec {
    #[serde(default)]
    ///List of containers within the pod. Always at least 1 in normal kubernetes deployment
    pub containers: Vec<Container>,
}

#[derive(serde_derive::Deserialize)]
///Container object definition
pub struct ContainerStatus {
    ///Unique identifier of the container
    ///
    ///It normally starts with `containerd://` in kubernetes cluster
    pub container_id: String,
    ///Name of the container
    pub name: String,
    ///Image used by the container
    pub image: String,
    ///Indicates container status to be ready or not
    pub ready: bool,
    ///Indicates restart count
    pub restart_count: i32,
}

impl ContainerStatus {
    #[inline]
    ///Access unique id of container without type prefix
    pub fn container_id_suffix(&self) -> &str {
        let mut parts = self.container_id.split("://");
        let type_or_not = parts.next().unwrap();
        parts.next().unwrap_or(type_or_not)
    }
}

#[derive(Default, serde_derive::Deserialize)]
///Pod status
pub struct PodStatus {
    #[serde(default)]
    ///Statuses of the containers within pod
    pub container_statuses: Vec<ContainerStatus>,
}

#[derive(serde_derive::Deserialize)]
///Pod object definition
pub struct Pod {
    ///Pod metadata
    pub metadata: Metadata,
    #[serde(default)]
    ///Pod desired specification
    pub spec: PodSpec,
    #[serde(default)]
    ///Pod current status
    pub status: PodStatus,
}
