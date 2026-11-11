use crate::workspace_settings::{KUBERNETES, kubectl, workspace_configs};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const CONVERSATION_LABEL: &str = "inference-gateway.com/conversation";
const AGENT_CONTAINER: &str = "orchestrator";
const GPU_SUFFIX: &str = "/gpu";

/// What a running computer was given when it started: its CPUs and memory as
/// Kubernetes quantities, empty when the cluster decided, and its GPUs.
#[derive(Debug, PartialEq, serde::Serialize)]
pub(crate) struct ComputerResources {
    cpu: String,
    memory: String,
    gpus: u64,
}

/// The container the agent runs in, which is the one the workspace's amounts
/// go to. A pod without one by that name is read by its first container.
fn agent_container(pod: &Value) -> Option<&Value> {
    let containers = pod["spec"]["containers"].as_array()?;
    containers
        .iter()
        .find(|container| container["name"] == AGENT_CONTAINER)
        .or_else(|| containers.first())
}

/// What the container may use of `resource`, or what it asked for when it
/// has no limit.
fn given(resources: &Value, resource: &str) -> String {
    ["limits", "requests"]
        .iter()
        .find_map(|block| resources[block][resource].as_str())
        .unwrap_or_default()
        .to_owned()
}

fn is_gpu(resource: &str) -> bool {
    resource == crate::cluster_gpu::RESOURCE || resource.ends_with(GPU_SUFFIX)
}

fn gpus(resources: &Value) -> u64 {
    resources["limits"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(resource, _)| is_gpu(resource))
        .filter_map(|(_, amount)| amount.as_str()?.parse::<u64>().ok())
        .sum()
}

/// Each chat's computer from kubectl's JSON listing of pods, by the chat the
/// pod is labelled with.
fn parse_computers(pods_json: &str) -> BTreeMap<String, ComputerResources> {
    let listing: Value = serde_json::from_str(pods_json).unwrap_or_default();
    listing["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pod| {
            let chat = pod["metadata"]["labels"][CONVERSATION_LABEL].as_str()?;
            let resources = &agent_container(pod)?["resources"];
            let computer = ComputerResources {
                cpu: given(resources, "cpu"),
                memory: given(resources, "memory"),
                gpus: gpus(resources),
            };
            Some((chat.to_owned(), computer))
        })
        .collect()
}

/// Where kubectl looks for the pods: the workspace's namespace, or every
/// namespace for a workspace that names none.
fn namespace_args(namespace: &str) -> Vec<&str> {
    if namespace.is_empty() {
        vec!["--all-namespaces"]
    } else {
        vec!["-n", namespace]
    }
}

fn computers_in(context: &str, namespace: &str) -> BTreeMap<String, ComputerResources> {
    let mut args = vec!["get", "pods", "-l", CONVERSATION_LABEL, "-o", "json"];
    args.extend(namespace_args(namespace));
    parse_computers(&kubectl(context, &args).unwrap_or_default())
}

/// The computers of every cluster, asked at once so one that does not answer
/// holds the others up no longer than its own time limit.
fn computers_of(clusters: &BTreeSet<(String, String)>) -> BTreeMap<String, ComputerResources> {
    std::thread::scope(|scope| {
        let listings: Vec<_> = clusters
            .iter()
            .map(|(context, namespace)| scope.spawn(|| computers_in(context, namespace)))
            .collect();
        listings
            .into_iter()
            .flat_map(|listing| listing.join().unwrap_or_default())
            .collect()
    })
}

/// What the computer of each chat on a Kubernetes workspace was given, read
/// from its running pod and not from the workspace's form. A cluster that
/// does not answer adds none.
#[tauri::command]
pub(crate) async fn computer_resources() -> Result<BTreeMap<String, ComputerResources>, String> {
    tokio::task::spawn_blocking(|| {
        let clusters: BTreeSet<(String, String)> = workspace_configs()
            .into_iter()
            .filter(|workspace| workspace.kind == KUBERNETES && !workspace.context.is_empty())
            .map(|workspace| (workspace.context, workspace.namespace))
            .collect();
        computers_of(&clusters)
    })
    .await
    .map_err(|e| format!("listing task failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PODS: &str = r#"{"items":[
      {"metadata":{"labels":{"inference-gateway.com/conversation":"chat-a"}},
       "spec":{"containers":[
         {"name":"sidecar","resources":{"limits":{"cpu":"1"}}},
         {"name":"orchestrator","resources":{
           "requests":{"cpu":"2","memory":"4Gi"},
           "limits":{"cpu":"8","devic.es/dri":"1","nvidia.com/gpu":"2"}}}]}},
      {"metadata":{"labels":{"inference-gateway.com/conversation":"chat-b"}},
       "spec":{"containers":[{"name":"orchestrator","resources":{"limits":{"devic.es/dri":"0"}}}]}},
      {"metadata":{"labels":{}},"spec":{"containers":[{"name":"gateway","resources":{}}]}}
    ]}"#;

    #[test]
    fn a_computer_shows_its_limit_or_else_its_request_and_counts_its_gpus() {
        let computers = parse_computers(PODS);
        assert_eq!(computers.len(), 2);
        assert_eq!(
            computers["chat-a"],
            ComputerResources {
                cpu: "8".into(),
                memory: "4Gi".into(),
                gpus: 3,
            }
        );
        assert_eq!(
            computers["chat-b"],
            ComputerResources {
                cpu: String::new(),
                memory: String::new(),
                gpus: 0,
            }
        );
    }

    #[test]
    fn a_workspace_without_a_namespace_is_looked_for_in_every_one() {
        assert_eq!(namespace_args("agents"), ["-n", "agents"]);
        assert_eq!(namespace_args(""), ["--all-namespaces"]);
    }

    #[test]
    fn a_listing_that_is_not_json_holds_no_computer() {
        assert!(parse_computers("").is_empty());
    }
}
