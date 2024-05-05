use anyhow::Context;

use crate::{
    kubeforward::vx::Pod,
    models::kube::{
        AnyReady,
        PodSelection,
    },
};

impl PodSelection for AnyReady {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod> {
        // todo: randomly select from the ready pods
        let pod = pods.iter().find(is_pod_ready).context(anyhow::anyhow!(
            "No ready pods found matching the selector '{}'",
            selector
        ))?;

        Ok(pod)
    }
}

fn is_pod_ready(pod: &&Pod) -> bool {
    let conditions = pod.status.as_ref().and_then(|s| s.conditions.as_ref());

    conditions
        .map(|c| c.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
        .unwrap_or(false)
}
