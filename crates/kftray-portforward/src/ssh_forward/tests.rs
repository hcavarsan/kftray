use std::net::SocketAddr;

use kftray_commons::models::config_model::Config;
use kftray_commons::models::ssh_provider::{
    GcpBastionProvider,
    SshProvider,
    SshProviderKind,
    WORKLOAD_SSH_PORT_FORWARD,
};
use kftray_commons::test_utils::{
    EnvVarGuard,
    HostsFileGuard,
    TestDb,
    test_db,
};
use kftray_commons::utils::config::insert_config_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_ssh::KftrayKey;
use kftray_ssh::testing::gcp::{
    FakeGcp,
    FakeGcpConfig,
};
use kftray_ssh::testing::{
    MockSshd,
    MockSshdOptions,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::TcpStream;

use crate::kube::{
    start_port_forward_with_mode,
    stop_port_forward_with_mode,
};
use crate::port_forward::CHILD_PROCESSES;

const MODE: DatabaseMode = DatabaseMode::Memory;
const PROJECT: &str = "kftray-test";
const ZONE: &str = "us-central1-b";
const INSTANCE: &str = "jumper";
const TOKEN: &str = "test-token";

/// Everything a test holds. Fields drop in declaration order: the fakes go
/// first and the locks last, and the env guard that owns the env lock (the
/// first one set) is dropped after the others.
struct Bastion {
    mock: MockSshd,
    _fake: FakeGcp,
    hosts: std::path::PathBuf,
    _hosts_guard: HostsFileGuard,
    _token: EnvVarGuard,
    _endpoints: EnvVarGuard,
    _config_dir: EnvVarGuard,
    _dir: tempfile::TempDir,
    _lock: tokio::sync::MutexGuard<'static, ()>,
    _db: TestDb,
}

async fn bastion() -> Bastion {
    let db = test_db().await;
    let lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
    let dir = tempfile::tempdir().unwrap();
    // The runtime loads this key lazily from KFTRAY_CONFIG; made first so
    // the sshd and OS Login can know it.
    let key = KftrayKey::load_or_create(dir.path()).unwrap();
    let mock = MockSshd::start(MockSshdOptions {
        authorized: vec![key.public()],
        ..MockSshdOptions::default()
    })
    .await
    .unwrap();
    let fake = FakeGcp::start(FakeGcpConfig {
        project: PROJECT.into(),
        zone: ZONE.into(),
        instance: INSTANCE.into(),
        instance_id: "1234567890".into(),
        email: "dev@example.com".into(),
        posix_user: "dev_example_com".into(),
        authorized_key: key.public_openssh(),
        ssh_upstream: mock.addr(),
        token: TOKEN.into(),
        oslogin_certificates: false,
        guest_host_key: None,
        user_ca: None,
    })
    .await
    .unwrap();
    let endpoints = serde_json::to_string(&fake.endpoints()).unwrap();
    let config_dir = EnvVarGuard::set("KFTRAY_CONFIG", &dir.path().to_string_lossy());
    let endpoints = EnvVarGuard::set("KFTRAY_GCP_ENDPOINTS", &endpoints);
    let token = EnvVarGuard::set("KFTRAY_GCP_STATIC_TOKEN", TOKEN);
    let hosts = dir.path().join("hosts");
    std::fs::write(&hosts, "127.0.0.1 localhost\n").unwrap();
    let hosts_guard = HostsFileGuard::point_at(&hosts);
    super::reset_caches().await;
    Bastion {
        mock,
        _fake: fake,
        hosts,
        _hosts_guard: hosts_guard,
        _token: token,
        _endpoints: endpoints,
        _config_dir: config_dir,
        _dir: dir,
        _lock: lock,
        _db: db,
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn ssh_config(alias: &str, destination: SocketAddr) -> Config {
    let config = Config {
        alias: Some(alias.to_owned()),
        workload_type: Some(WORKLOAD_SSH_PORT_FORWARD.to_owned()),
        protocol: "tcp".to_owned(),
        namespace: String::new(),
        context: None,
        service: None,
        target: None,
        kubeconfig: None,
        ssh_provider: Some(SshProvider::Known(SshProviderKind::GcpBastion(
            GcpBastionProvider {
                project: PROJECT.into(),
                zone: ZONE.into(),
                instance: INSTANCE.into(),
                interface: "nic0".into(),
            },
        ))),
        remote_address: Some(destination.ip().to_string()),
        remote_port: Some(destination.port()),
        local_address: Some("127.0.0.1".to_owned()),
        local_port: Some(free_port()),
        auto_loopback_address: false,
        domain_enabled: Some(true),
        ..Config::default()
    };
    let id = insert_config_with_mode(config.clone(), MODE).await.unwrap();
    Config {
        id: Some(id),
        ..config
    }
}

async fn start(config: &Config) -> Result<u16, String> {
    let responses = start_port_forward_with_mode(vec![config.clone()], "tcp", MODE, false).await?;
    Ok(responses[0].local_port)
}

async fn echo(port: u16, payload: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(payload).await.unwrap();
    let mut received = vec![0u8; payload.len()];
    stream.read_exact(&mut received).await.unwrap();
    received
}

async fn stop(config: &Config) {
    stop_port_forward_with_mode(config.id.unwrap().to_string(), MODE)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_started_ssh_forward_should_carry_bytes_to_the_destination() {
    let bastion = bastion().await;
    let config = ssh_config("ssh-echo", bastion.mock.echo_addr()).await;
    let port = start(&config).await.unwrap();

    let received = echo(port, b"ping").await;

    stop(&config).await;
    assert_eq!(received, b"ping");
}

#[tokio::test]
async fn a_started_ssh_forward_should_write_its_domain_alias() {
    let bastion = bastion().await;
    let config = ssh_config("ssh-echo", bastion.mock.echo_addr()).await;
    start(&config).await.unwrap();

    let hosts = std::fs::read_to_string(&bastion.hosts).unwrap();

    stop(&config).await;
    assert!(hosts.contains("ssh-echo"), "{hosts}");
}

#[tokio::test]
async fn a_stopped_ssh_forward_should_release_its_port_alias_and_process() {
    let bastion = bastion().await;
    let config = ssh_config("ssh-echo", bastion.mock.echo_addr()).await;
    let port = start(&config).await.unwrap();

    stop(&config).await;

    let hosts = std::fs::read_to_string(&bastion.hosts).unwrap();
    let released = (
        TcpStream::connect(("127.0.0.1", port)).await.is_err(),
        hosts.contains("ssh-echo"),
        CHILD_PROCESSES.is_empty(),
    );
    assert_eq!(released, (true, false, true));
}

#[tokio::test]
async fn an_ssh_forward_to_a_closed_port_should_fail_and_leave_nothing() {
    let bastion = bastion().await;
    let closed = SocketAddr::from(([127, 0, 0, 1], free_port()));
    let config = ssh_config("ssh-closed", closed).await;

    let error = start(&config).await.unwrap_err();

    let hosts = std::fs::read_to_string(&bastion.hosts).unwrap();
    assert!(
        error.contains("did not respond")
            && CHILD_PROCESSES.is_empty()
            && !hosts.contains("ssh-closed"),
        "{error}"
    );
}

#[tokio::test]
async fn two_ssh_forwards_on_one_bastion_should_share_one_session() {
    let bastion = bastion().await;
    let first = ssh_config("ssh-one", bastion.mock.echo_addr()).await;
    let second = ssh_config("ssh-two", bastion.mock.echo_addr()).await;
    start(&first).await.unwrap();
    start(&second).await.unwrap();

    let sessions = bastion.mock.session_count();

    stop(&first).await;
    stop(&second).await;
    assert_eq!(sessions, 1);
}

#[tokio::test]
async fn a_failed_listener_should_end_the_forward_task_while_the_session_lives() {
    // The sender stands in for the live session the lease keeps open.
    let (_session, state) = tokio::sync::watch::channel(kftray_ssh::SessionState::Ready);
    let failing = async { Err(anyhow::anyhow!("accept failed")) };

    let ended = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        super::run_until_listener_ends(
            failing,
            state,
            1,
            MODE,
            tokio_util::sync::CancellationToken::new(),
        ),
    )
    .await;

    assert!(matches!(ended, Ok(Err(_))), "the forward task kept running");
}
