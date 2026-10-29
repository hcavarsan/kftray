use kftray_commons::models::config_model::Config;
use kftray_commons::models::ssh_provider::{
    GcpBastionProvider,
    SshProvider,
    SshProviderKind,
    WORKLOAD_SSH_PORT_FORWARD,
};

use crate::fixtures::{
    Expect,
    Fixture,
};
use crate::harness::free_port;
use crate::harness::gcp_bastion::{
    ECHO_HOST,
    INSTANCE,
    PROJECT,
    ZONE,
};
use crate::harness::workload::{
    NAMESPACE,
    Workload,
};

const ECHO_BODY: &str = "kftray-e2e";
const ECHO_DATAGRAM: &[u8] = b"kftray-e2e";

fn base(alias: &str, workload: Workload, workload_type: &str) -> Config {
    Config {
        alias: Some(alias.to_owned()),
        service: Some(workload.name().to_owned()),
        namespace: NAMESPACE.to_owned(),
        context: Some("default".to_owned()),
        local_port: free_port().ok(),
        remote_port: Some(workload.port()),
        workload_type: Some(workload_type.to_owned()),
        protocol: "tcp".to_owned(),
        remote_address: None,
        target: None,
        kubeconfig: None,
        ..Config::default()
    }
}

pub fn service_http() -> Fixture {
    Fixture {
        name: "service-http",
        workload: Workload::EchoHttp,
        config: base("svc-http", Workload::EchoHttp, "service"),
        expect: Expect::Http { body: ECHO_BODY },
    }
}

pub fn pod_http() -> Fixture {
    let mut config = base("pod-http", Workload::EchoHttp, "pod");
    config.service = None;
    config.target = Some(Workload::EchoHttp.selector());
    Fixture {
        name: "pod-http",
        workload: Workload::EchoHttp,
        config,
        expect: Expect::Http { body: ECHO_BODY },
    }
}

pub fn service_udp() -> Fixture {
    let mut config = base("svc-udp", Workload::EchoUdp, "service");
    config.protocol = "udp".to_owned();
    Fixture {
        name: "service-udp",
        workload: Workload::EchoUdp,
        config,
        expect: Expect::Udp {
            send: ECHO_DATAGRAM,
            reply: ECHO_DATAGRAM,
        },
    }
}

pub fn proxy_http() -> Fixture {
    let mut config = base("proxy-http", Workload::EchoHttp, "proxy");
    config.service = None;
    config.remote_address = Some(Workload::EchoHttp.service_host(NAMESPACE));
    Fixture {
        name: "proxy-http",
        workload: Workload::EchoHttp,
        config,
        expect: Expect::Http { body: ECHO_BODY },
    }
}

pub fn ssh_gcp_bastion_http() -> Fixture {
    let config = Config {
        alias: Some("ssh-gcp-bastion-http".to_owned()),
        workload_type: Some(WORKLOAD_SSH_PORT_FORWARD.to_owned()),
        protocol: "tcp".to_owned(),
        ssh_provider: Some(SshProvider::Known(SshProviderKind::GcpBastion(
            GcpBastionProvider {
                project: PROJECT.to_owned(),
                zone: ZONE.to_owned(),
                instance: INSTANCE.to_owned(),
                interface: "nic0".to_owned(),
            },
        ))),
        remote_address: Some(ECHO_HOST.to_owned()),
        remote_port: Some(Workload::EchoHttp.port()),
        local_port: free_port().ok(),
        namespace: String::new(),
        context: None,
        service: None,
        target: None,
        kubeconfig: None,
        ..Config::default()
    };
    Fixture {
        name: "ssh-gcp-bastion-http",
        workload: Workload::EchoHttp,
        config,
        expect: Expect::Http { body: ECHO_BODY },
    }
}
