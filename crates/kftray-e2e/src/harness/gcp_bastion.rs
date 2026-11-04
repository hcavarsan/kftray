use std::net::{
    Ipv4Addr,
    SocketAddr,
};
use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
};
use kftray_ssh::KftrayKey;
use kftray_ssh::consts::KEY_RELATIVE_PATH;
use kftray_ssh::testing::gcp::{
    FakeGcp,
    FakeGcpConfig,
};
use testcontainers_modules::testcontainers::core::{
    IntoContainerPort,
    WaitFor,
};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{
    ContainerAsync,
    GenericImage,
    ImageExt,
};

use crate::harness::workload::{
    HTTP_ECHO_NAME,
    HTTP_ECHO_TAG,
    Workload,
};

pub const PROJECT: &str = "kftray-e2e";
pub const ZONE: &str = "local-central1-a";
pub const INSTANCE: &str = "bastion";
pub const ECHO_HOST: &str = "kftray-e2e-echo";
pub const TOKEN: &str = "kftray-e2e";

const NETWORK: &str = "kftray-e2e-ssh";
const KEY_DIR: &str = "gcp-bastion";
const SSHD_IMAGE: &str = "alpine";
const SSHD_TAG: &str = "3.22";
const SSH_PORT: u16 = 22;
const POSIX_USER: &str = "kftray";
const INSTANCE_ID: &str = "1234567890123456789";
const EMAIL: &str = "e2e@kftray.test";
const SSHD_SCRIPT: &str = r#"apk add --no-cache openssh-server && ssh-keygen -A && adduser -D -s /bin/sh kftray && sed -i 's/^kftray:!:/kftray:*:/' /etc/shadow && mkdir -p /home/kftray/.ssh && printf '%s\n' "$AUTHORIZED_KEY" > /home/kftray/.ssh/authorized_keys && chown -R kftray /home/kftray/.ssh && chmod 700 /home/kftray/.ssh && chmod 600 /home/kftray/.ssh/authorized_keys && exec /usr/sbin/sshd -D -e -o AllowTcpForwarding=yes -o PasswordAuthentication=no"#;

pub struct GcpBastion {
    _echo: ContainerAsync<GenericImage>,
    _sshd: ContainerAsync<GenericImage>,
    gcp: FakeGcp,
    ssh_key: PathBuf,
}

impl GcpBastion {
    pub async fn start(out_dir: &Path) -> Result<Self> {
        let key_dir = out_dir.join(KEY_DIR);
        std::fs::create_dir_all(&key_dir)
            .with_context(|| format!("create {}", key_dir.display()))?;
        let key = KftrayKey::load_or_create(&key_dir).context("create the e2e SSH key")?;

        log::info!("starting the gcp-bastion echo and sshd containers");
        let echo = GenericImage::new(HTTP_ECHO_NAME, HTTP_ECHO_TAG)
            .with_wait_for(WaitFor::message_on_stderr("server is listening"))
            .with_network(NETWORK)
            .with_container_name(ECHO_HOST)
            .with_cmd(Workload::echo_http_args())
            .start()
            .await
            .context("start the echo container; docker must be running")?;
        let sshd = GenericImage::new(SSHD_IMAGE, SSHD_TAG)
            .with_exposed_port(SSH_PORT.tcp())
            .with_wait_for(WaitFor::message_on_stderr("Server listening"))
            .with_network(NETWORK)
            .with_env_var("AUTHORIZED_KEY", key.public_openssh())
            .with_cmd(["sh", "-c", SSHD_SCRIPT])
            .start()
            .await
            .context("start the sshd container")?;
        let ssh_port = sshd
            .get_host_port_ipv4(SSH_PORT)
            .await
            .context("read the mapped sshd port")?;

        let gcp = FakeGcp::start(FakeGcpConfig {
            project: PROJECT.to_owned(),
            zone: ZONE.to_owned(),
            instance: INSTANCE.to_owned(),
            instance_id: INSTANCE_ID.to_owned(),
            email: EMAIL.to_owned(),
            posix_user: POSIX_USER.to_owned(),
            authorized_key: key.public_openssh(),
            ssh_upstream: SocketAddr::from((Ipv4Addr::LOCALHOST, ssh_port)),
            token: TOKEN.to_owned(),
            oslogin_certificates: false,
            guest_host_key: None,
            user_ca: None,
        })
        .await
        .context("start the fake GCP APIs")?;
        log::info!("gcp-bastion ready, sshd on 127.0.0.1:{ssh_port}");

        Ok(Self {
            _echo: echo,
            _sshd: sshd,
            gcp,
            ssh_key: key_dir.join(KEY_RELATIVE_PATH),
        })
    }

    pub fn endpoints_json(&self) -> Result<String> {
        serde_json::to_string(&self.gcp.endpoints()).context("serialize the fake GCP endpoints")
    }

    pub fn ssh_key(&self) -> &Path {
        &self.ssh_key
    }
}
