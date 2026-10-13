//! Says why the pods a forward selects are not ready, for the message of a
//! start that ran out of time waiting for one.

use std::time::Duration;

use k8s_openapi::api::core::v1::{
    ContainerState,
    ContainerStatus,
    Pod,
};
use kube::{
    Api,
    ResourceExt,
    api::ListParams,
};
use kube_portforward::PodSelector;

/// Pods named in the message; the rest are only counted.
const MAX_PODS_DESCRIBED: usize = 3;
/// Scheduler and kubelet messages can run to several lines.
const MAX_DETAIL_CHARS: usize = 160;
/// The start is already over its deadline, so reading the pods must not hold
/// the failure back for long.
pub(crate) const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Reads the selected pods and describes why none of them is ready.
///
/// The pod watcher behind the forwarder drops container statuses, so the
/// reason needs its own read. It only runs after a start has timed out, never
/// on the success path. `None` when the selector kind is not one this knows
/// how to read.
pub(crate) async fn explain_unready_pods(
    pod_api: &Api<Pod>, selector: &PodSelector,
) -> Option<String> {
    let lookup = async {
        match selector {
            PodSelector::Name(name) => pod_api
                .get_opt(name)
                .await
                .map(|pod| pod.into_iter().collect::<Vec<_>>())
                .map(Some),
            PodSelector::Labels { selector } => pod_api
                .list(&ListParams::default().labels(selector))
                .await
                .map(|pods| Some(pods.items)),
            _ => Ok(None),
        }
    };
    match tokio::time::timeout(LOOKUP_TIMEOUT, lookup).await {
        Ok(Ok(Some(pods))) => Some(describe_unready_pods(&pods, selector)),
        Ok(Ok(None)) => None,
        Ok(Err(error)) => Some(format!(
            "the pods could not be read to find out why: {error}"
        )),
        Err(_) => Some("reading the pods to find out why took too long".to_string()),
    }
}

fn describe_unready_pods(pods: &[Pod], selector: &PodSelector) -> String {
    if pods.is_empty() {
        return match selector {
            PodSelector::Name(name) => format!("no pod named {name}"),
            PodSelector::Labels { selector } => format!("no pod matches {selector}"),
            _ => "no pod matches the selector".to_string(),
        };
    }

    let mut described = pods
        .iter()
        .take(MAX_PODS_DESCRIBED)
        .map(|pod| format!("pod {} {}", pod.name_any(), pod_problem(pod)))
        .collect::<Vec<_>>()
        .join("; ");
    if pods.len() > MAX_PODS_DESCRIBED {
        let more = pods.len() - MAX_PODS_DESCRIBED;
        described = format!("{described}; and {more} more");
    }
    described
}

/// What is wrong with one pod, as a predicate for "pod <name> ...".
fn pod_problem(pod: &Pod) -> String {
    if pod.metadata.deletion_timestamp.is_some() {
        return "is terminating".to_string();
    }
    let Some(status) = &pod.status else {
        return "has not reported a status yet".to_string();
    };
    let phase = status.phase.as_deref().unwrap_or("Unknown");

    if matches!(phase, "Failed" | "Succeeded") {
        let detail = [status.reason.as_deref(), status.message.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(": ");
        return with_detail(format!("has {}", phase.to_lowercase()), &detail);
    }

    if let Some(condition) = status
        .conditions
        .iter()
        .flatten()
        .find(|condition| condition.type_ == "PodScheduled" && condition.status == "False")
    {
        let reason = condition.reason.as_deref().unwrap_or("not scheduled");
        return with_detail(
            format!("is not scheduled ({reason})"),
            condition.message.as_deref().unwrap_or_default(),
        );
    }

    let init_problem = status
        .init_container_statuses
        .iter()
        .flatten()
        .find_map(|container| container_problem(container, "init container"));
    if let Some(problem) = init_problem {
        return problem;
    }

    let problems = status
        .container_statuses
        .iter()
        .flatten()
        .filter(|container| !container.ready)
        .filter_map(|container| container_problem(container, "container"))
        .collect::<Vec<_>>();
    if !problems.is_empty() {
        return problems.join(", ");
    }

    let ready = status
        .conditions
        .iter()
        .flatten()
        .find(|condition| condition.type_ == "Ready");
    match ready {
        Some(condition) if condition.status == "True" => {
            "is ready now, but was not before the wait ended".to_string()
        }
        Some(condition) => with_detail(
            format!("is {phase} but not ready"),
            &[condition.reason.as_deref(), condition.message.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(": "),
        ),
        None => format!("is {phase} and has not reported readiness yet"),
    }
}

/// `None` for a container that is fine or has finished cleanly (an init
/// container that completed).
fn container_problem(container: &ContainerStatus, kind: &str) -> Option<String> {
    let name = &container.name;
    let restarts = match container.restart_count {
        0 => String::new(),
        1 => ", restarted once".to_string(),
        count => format!(", restarted {count} times"),
    };
    match container.state.as_ref()? {
        ContainerState {
            waiting: Some(waiting),
            ..
        } => {
            let reason = waiting.reason.as_deref().unwrap_or("waiting");
            let last_exit = container
                .last_state
                .as_ref()
                .and_then(|state| state.terminated.as_ref())
                .map(|terminated| format!(", last exit code {}", terminated.exit_code))
                .unwrap_or_default();
            Some(format!(
                "{kind} {name} is waiting ({reason}{restarts}{last_exit})"
            ))
        }
        ContainerState {
            terminated: Some(terminated),
            ..
        } => {
            if terminated.exit_code == 0 && kind == "init container" {
                return None;
            }
            let reason = terminated.reason.as_deref().unwrap_or("terminated");
            Some(format!(
                "{kind} {name} exited ({reason}, exit code {}{restarts})",
                terminated.exit_code
            ))
        }
        ContainerState {
            running: Some(_), ..
        } if kind == "container" => Some(format!(
            "{kind} {name} is running but not ready, so its readiness probe is failing"
        )),
        _ => None,
    }
}

fn with_detail(summary: String, detail: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        return summary;
    }
    let mut chars = detail.chars();
    let shown = chars.by_ref().take(MAX_DETAIL_CHARS).collect::<String>();
    let ellipsis = if chars.next().is_some() { "..." } else { "" };
    format!("{summary}: {shown}{ellipsis}")
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::core::v1::{
        ContainerStateRunning,
        ContainerStateTerminated,
        ContainerStateWaiting,
        PodCondition,
        PodStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{
        ObjectMeta,
        Time,
    };

    use super::*;

    fn pod(name: &str, status: PodStatus) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            status: Some(status),
            ..Default::default()
        }
    }

    fn labels(selector: &str) -> PodSelector {
        PodSelector::Labels {
            selector: selector.to_string(),
        }
    }

    fn waiting(name: &str, reason: &str, restarts: i32) -> ContainerStatus {
        ContainerStatus {
            name: name.to_string(),
            restart_count: restarts,
            state: Some(ContainerState {
                waiting: Some(ContainerStateWaiting {
                    reason: Some(reason.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn no_pod_is_reported_with_its_selector() {
        assert_eq!(
            describe_unready_pods(&[], &labels("app=web")),
            "no pod matches app=web"
        );
        assert_eq!(
            describe_unready_pods(&[], &PodSelector::Name("web-0".into())),
            "no pod named web-0"
        );
    }

    #[test]
    fn a_crash_looping_container_is_named_with_its_last_exit_code() {
        let mut container = waiting("app", "CrashLoopBackOff", 7);
        container.last_state = Some(ContainerState {
            terminated: Some(ContainerStateTerminated {
                exit_code: 137,
                ..Default::default()
            }),
            ..Default::default()
        });
        let pod = pod(
            "web-1",
            PodStatus {
                phase: Some("Running".into()),
                container_statuses: Some(vec![container]),
                ..Default::default()
            },
        );

        assert_eq!(
            describe_unready_pods(&[pod], &labels("app=web")),
            "pod web-1 container app is waiting (CrashLoopBackOff, restarted 7 times, last exit \
             code 137)"
        );
    }

    #[test]
    fn an_image_that_cannot_be_pulled_is_named() {
        let pod = pod(
            "web-1",
            PodStatus {
                phase: Some("Pending".into()),
                container_statuses: Some(vec![waiting("app", "ImagePullBackOff", 0)]),
                ..Default::default()
            },
        );

        assert_eq!(
            describe_unready_pods(&[pod], &labels("app=web")),
            "pod web-1 container app is waiting (ImagePullBackOff)"
        );
    }

    #[test]
    fn a_pod_the_scheduler_cannot_place_gives_the_scheduler_message() {
        let pod = pod(
            "web-1",
            PodStatus {
                phase: Some("Pending".into()),
                conditions: Some(vec![PodCondition {
                    type_: "PodScheduled".into(),
                    status: "False".into(),
                    reason: Some("Unschedulable".into()),
                    message: Some("0/3 nodes are available: 3 Insufficient memory.".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            },
        );

        assert_eq!(
            describe_unready_pods(&[pod], &labels("app=web")),
            "pod web-1 is not scheduled (Unschedulable): 0/3 nodes are available: 3 \
             Insufficient memory."
        );
    }

    #[test]
    fn a_running_container_that_is_not_ready_points_at_the_readiness_probe() {
        let pod = pod(
            "web-1",
            PodStatus {
                phase: Some("Running".into()),
                conditions: Some(vec![PodCondition {
                    type_: "Ready".into(),
                    status: "False".into(),
                    ..Default::default()
                }]),
                container_statuses: Some(vec![ContainerStatus {
                    name: "app".into(),
                    state: Some(ContainerState {
                        running: Some(ContainerStateRunning::default()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            },
        );

        assert_eq!(
            describe_unready_pods(&[pod], &labels("app=web")),
            "pod web-1 container app is running but not ready, so its readiness probe is failing"
        );
    }

    #[test]
    fn a_stuck_init_container_is_reported_before_the_app_containers() {
        let pod = pod(
            "web-1",
            PodStatus {
                phase: Some("Pending".into()),
                init_container_statuses: Some(vec![waiting("migrate", "CrashLoopBackOff", 2)]),
                container_statuses: Some(vec![waiting("app", "PodInitializing", 0)]),
                ..Default::default()
            },
        );

        assert!(
            describe_unready_pods(&[pod], &labels("app=web"))
                .starts_with("pod web-1 init container migrate is waiting (CrashLoopBackOff"),
        );
    }

    #[test]
    fn a_terminating_pod_is_reported_as_such() {
        let mut terminating = pod(
            "web-1",
            PodStatus {
                phase: Some("Running".into()),
                ..Default::default()
            },
        );
        terminating.metadata.deletion_timestamp =
            Some(Time(k8s_openapi::jiff::Timestamp::UNIX_EPOCH));

        assert_eq!(
            describe_unready_pods(&[terminating], &labels("app=web")),
            "pod web-1 is terminating"
        );
    }

    #[test]
    fn an_evicted_pod_gives_the_kubelet_reason() {
        let pod = pod(
            "web-1",
            PodStatus {
                phase: Some("Failed".into()),
                reason: Some("Evicted".into()),
                message: Some("The node was low on resource: memory.".into()),
                ..Default::default()
            },
        );

        assert_eq!(
            describe_unready_pods(&[pod], &labels("app=web")),
            "pod web-1 has failed: Evicted: The node was low on resource: memory."
        );
    }

    #[test]
    fn long_details_are_cut_and_extra_pods_are_counted() {
        let long = "x".repeat(MAX_DETAIL_CHARS + 50);
        let pending = |name: &str| {
            pod(
                name,
                PodStatus {
                    phase: Some("Pending".into()),
                    conditions: Some(vec![PodCondition {
                        type_: "PodScheduled".into(),
                        status: "False".into(),
                        reason: Some("Unschedulable".into()),
                        message: Some(long.clone()),
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
            )
        };
        let pods = (0..5)
            .map(|index| pending(&format!("web-{index}")))
            .collect::<Vec<_>>();

        let message = describe_unready_pods(&pods, &labels("app=web"));

        assert!(message.contains("pod web-2 "), "{message}");
        assert!(!message.contains("pod web-3 "), "{message}");
        assert!(message.ends_with("; and 2 more"), "{message}");
        assert!(
            message.contains(&format!("{}...", "x".repeat(MAX_DETAIL_CHARS))),
            "{message}"
        );
        assert!(
            !message.contains(&"x".repeat(MAX_DETAIL_CHARS + 1)),
            "{message}"
        );
    }
}
