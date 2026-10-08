use kftray_commons::models::config_model::Config;

use crate::fixtures::{
    Expect,
    Fixture,
};
use crate::harness::free_port;
use crate::harness::workload::{
    NAMESPACE,
    Workload,
};

const ECHO_BODY: &str = "kftray-e2e";

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
