use crate::vx::Pod;
use anyhow::Context;

/// Pod selection according to impl specific criteria.
pub(crate) trait PodSelection {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod>;
}

/// Selects any pod so long as it's ready.
pub(crate) struct AnyReady {}

impl PodSelection for AnyReady {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod> {
        // todo: randomly select from the ready pods
        let pod = pods
            .iter()
            .find(is_pod_ready)
            .context(anyhow::anyhow!("No pod '{}' available", selector))?;
        Ok(pod)
    }
}

fn is_pod_ready(pod: &&Pod) -> bool {
    let conditions = pod.status.as_ref().and_then(|s| s.conditions.as_ref());
    conditions
        .map(|c| c.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
        .unwrap_or(false)
}
