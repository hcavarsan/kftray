#[derive(serde::Serialize, serde::Deserialize, Debug)]

pub struct CustomResponse {
    pub id: Option<i64>,
    pub service: String,
    pub namespace: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub context: String,
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
    pub protocol: String,
}

pub struct CustomResponseBuilder {
    id: Option<i64>,
    service: Option<String>,
    namespace: Option<String>,
    local_port: Option<u16>,
    remote_port: Option<u16>,
    context: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
    status: Option<i32>,
    protocol: Option<String>,
}

impl CustomResponseBuilder {
    pub fn new() -> Self {
        CustomResponseBuilder {
            id: None,
            service: None,
            namespace: None,
            local_port: None,
            remote_port: None,
            context: None,
            stdout: None,
            stderr: None,
            status: None,
            protocol: None,
        }
    }

    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);

        self
    }

    pub fn service(mut self, service: String) -> Self {
        self.service = Some(service);

        self
    }

    pub fn namespace(mut self, namespace: String) -> Self {
        self.namespace = Some(namespace);

        self
    }

    pub fn local_port(mut self, local_port: u16) -> Self {
        self.local_port = Some(local_port);

        self
    }

    pub fn remote_port(mut self, remote_port: u16) -> Self {
        self.remote_port = Some(remote_port);

        self
    }

    pub fn context(mut self, context: String) -> Self {
        self.context = Some(context);

        self
    }

    pub fn stdout(mut self, stdout: String) -> Self {
        self.stdout = Some(stdout);

        self
    }

    pub fn stderr(mut self, stderr: String) -> Self {
        self.stderr = Some(stderr);

        self
    }

    pub fn status(mut self, status: i32) -> Self {
        self.status = Some(status);

        self
    }

    pub fn protocol(mut self, protocol: String) -> Self {
        self.protocol = Some(protocol);

        self
    }

    pub fn build(self) -> CustomResponse {
        CustomResponse {
            id: self.id,
            service: self.service.expect("Service is required"),
            namespace: self.namespace.expect("Namespace is required"),
            local_port: self.local_port.expect("Local port is required"),
            remote_port: self.remote_port.expect("Remote port is required"),
            context: self.context.expect("Context is required"),
            stdout: self.stdout.expect("Stdout is required"),
            stderr: self.stderr.expect("Stderr is required"),
            status: self.status.expect("Status is required"),
            protocol: self.protocol.expect("Protocol is required"),
        }
    }
}
