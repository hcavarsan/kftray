use kube::api::{
    Api,
    ListParams,
};

use crate::models::kube::{
    AnyReady,
    PodSelection,
    Target,
    TargetPod,
    TargetSelector,
};

pub struct TargetPodFinder<'a> {
    pub pod_api: &'a Api<k8s_openapi::api::core::v1::Pod>,
    pub svc_api: &'a Api<k8s_openapi::api::core::v1::Service>,
}

impl<'a> TargetPodFinder<'a> {
    pub(crate) async fn find(&self, target: &Target) -> anyhow::Result<TargetPod> {
        let ready_pod = AnyReady {};

        match &target.selector {
            TargetSelector::ServiceName(name) => match self.svc_api.get(name).await {
                Ok(service) => {
                    if let Some(selector) = service.spec.and_then(|spec| spec.selector) {
                        let label_selector_str = selector
                            .iter()
                            .map(|(key, value)| format!("{}={}", key, value))
                            .collect::<Vec<_>>()
                            .join(",");

                        let pods = self
                            .pod_api
                            .list(&ListParams::default().labels(&label_selector_str))
                            .await?;

                        let pod = ready_pod.select(&pods.items, &label_selector_str)?;

                        target.find(pod, None)
                    } else {
                        Err(anyhow::anyhow!("No selector found for service '{}'", name))
                    }
                }
                Err(kube::Error::Api(kube::error::ErrorResponse { code: 404, .. })) => {
                    let label_selector_str = format!("app={}", name);

                    let pods = self
                        .pod_api
                        .list(&ListParams::default().labels(&label_selector_str))
                        .await?;

                    let pod = ready_pod.select(&pods.items, &label_selector_str)?;

                    target.find(pod, None)
                }
                Err(e) => Err(anyhow::anyhow!("Error finding service '{}': {}", name, e)),
            },
        }
    }
}
