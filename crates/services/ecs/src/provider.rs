use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::store::{Cluster, ContainerDefinition, EcsStore, Service, Task, TaskDefinition};

pub struct EcsProvider {
    store: Arc<AccountRegionBundle<EcsStore>>,
}

impl EcsProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for EcsProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — ECS uses JSON protocol (application/x-amz-json-1.1)
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "message": message,
            }))
            .unwrap(),
        )),
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn cluster_arn(account_id: &str, region: &str, name: &str) -> String {
    format!("arn:aws:ecs:{region}:{account_id}:cluster/{name}")
}

fn service_arn(account_id: &str, region: &str, cluster: &str, name: &str) -> String {
    format!("arn:aws:ecs:{region}:{account_id}:service/{cluster}/{name}")
}

fn task_def_arn(account_id: &str, region: &str, family: &str, revision: u32) -> String {
    format!("arn:aws:ecs:{region}:{account_id}:task-definition/{family}:{revision}")
}

fn task_arn(account_id: &str, region: &str, id: &str) -> String {
    format!("arn:aws:ecs:{region}:{account_id}:task/{id}")
}

fn cluster_json(c: &Cluster) -> Value {
    json!({
        "clusterName": c.cluster_name,
        "clusterArn": c.cluster_arn,
        "status": c.status,
        "registeredContainerInstancesCount": c.registered_container_instances_count,
        "runningTasksCount": c.running_tasks_count,
        "pendingTasksCount": c.pending_tasks_count,
        "activeServicesCount": c.active_services_count,
    })
}

fn task_def_json(td: &TaskDefinition) -> Value {
    let containers: Vec<Value> = td
        .container_definitions
        .iter()
        .map(|cd| {
            json!({
                "name": cd.name,
                "image": cd.image,
                "cpu": cd.cpu,
                "memory": cd.memory,
                "essential": cd.essential,
            })
        })
        .collect();
    json!({
        "family": td.family,
        "revision": td.revision,
        "taskDefinitionArn": td.task_definition_arn,
        "status": td.status,
        "containerDefinitions": containers,
        "cpu": td.cpu,
        "memory": td.memory,
        "networkMode": td.network_mode,
    })
}

fn service_json(s: &Service) -> Value {
    json!({
        "serviceName": s.service_name,
        "serviceArn": s.service_arn,
        "clusterArn": s.cluster_arn,
        "taskDefinition": s.task_definition,
        "desiredCount": s.desired_count,
        "runningCount": s.running_count,
        "pendingCount": s.pending_count,
        "status": s.status,
    })
}

fn task_json(t: &Task) -> Value {
    json!({
        "taskArn": t.task_arn,
        "clusterArn": t.cluster_arn,
        "taskDefinitionArn": t.task_definition_arn,
        "lastStatus": t.last_status,
        "desiredStatus": t.desired_status,
        "group": t.group,
        "startedAt": t.started_at.map(|d| d.to_rfc3339()),
        "stoppedAt": t.stopped_at.map(|d| d.to_rfc3339()),
        "stopCode": t.stop_code,
        "stoppedReason": t.stopped_reason,
    })
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for EcsProvider {
    fn service_name(&self) -> &str {
        "ecs"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateCluster
            // ----------------------------------------------------------------
            "CreateCluster" => {
                let cluster_name =
                    str_param(ctx, "clusterName").unwrap_or_else(|| "default".to_string());
                let arn = cluster_arn(account_id, region, &cluster_name);

                let mut store = self.store.get_or_create(account_id, region);
                if store.clusters.contains_key(&arn) {
                    return Ok(json_error(
                        "ClusterAlreadyExistsException",
                        &format!("Cluster {cluster_name} already exists"),
                        400,
                    ));
                }
                let cluster = Cluster {
                    cluster_name: cluster_name.clone(),
                    cluster_arn: arn.clone(),
                    status: "ACTIVE".to_string(),
                    registered_container_instances_count: 0,
                    running_tasks_count: 0,
                    pending_tasks_count: 0,
                    active_services_count: 0,
                    created: Utc::now(),
                };
                store.clusters.insert(arn, cluster.clone());
                Ok(json_ok(json!({ "cluster": cluster_json(&cluster) })))
            }

            // ----------------------------------------------------------------
            // DeleteCluster
            // ----------------------------------------------------------------
            "DeleteCluster" => {
                let cluster_ref = match str_param(ctx, "cluster") {
                    Some(c) => c,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "cluster is required",
                            400,
                        ));
                    }
                };
                // Accept both name and ARN
                let mut store = self.store.get_or_create(account_id, region);
                let arn = if cluster_ref.starts_with("arn:") {
                    cluster_ref.clone()
                } else {
                    cluster_arn(account_id, region, &cluster_ref)
                };
                match store.clusters.remove(&arn) {
                    Some(c) => Ok(json_ok(json!({ "cluster": cluster_json(&c) }))),
                    None => Ok(json_error(
                        "ClusterNotFoundException",
                        &format!("Cluster {cluster_ref} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeClusters
            // ----------------------------------------------------------------
            "DescribeClusters" => {
                let cluster_refs: Vec<String> = ctx
                    .request_body
                    .get("clusters")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "clusters": [], "failures": [] })));
                };

                let clusters: Vec<Value> = if cluster_refs.is_empty() {
                    store.clusters.values().map(cluster_json).collect()
                } else {
                    cluster_refs
                        .iter()
                        .filter_map(|r| {
                            let arn = if r.starts_with("arn:") {
                                r.clone()
                            } else {
                                cluster_arn(account_id, region, r)
                            };
                            store.clusters.get(&arn).map(cluster_json)
                        })
                        .collect()
                };

                Ok(json_ok(json!({ "clusters": clusters, "failures": [] })))
            }

            // ----------------------------------------------------------------
            // ListClusters
            // ----------------------------------------------------------------
            "ListClusters" => {
                let arns: Vec<String> = self
                    .store
                    .get(account_id, region)
                    .map(|store| store.clusters.keys().cloned().collect())
                    .unwrap_or_default();
                Ok(json_ok(json!({ "clusterArns": arns, "nextToken": null })))
            }

            // ----------------------------------------------------------------
            // RegisterTaskDefinition
            // ----------------------------------------------------------------
            "RegisterTaskDefinition" => {
                let family = match str_param(ctx, "family") {
                    Some(f) => f,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "family is required",
                            400,
                        ));
                    }
                };
                let network_mode =
                    str_param(ctx, "networkMode").unwrap_or_else(|| "bridge".to_string());
                let cpu = str_param(ctx, "cpu");
                let memory = str_param(ctx, "memory");

                let container_defs: Vec<ContainerDefinition> = ctx
                    .request_body
                    .get("containerDefinitions")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|cd| {
                                let name = cd.get("name")?.as_str()?.to_string();
                                let image = cd
                                    .get("image")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("nginx")
                                    .to_string();
                                let cpu_val =
                                    cd.get("cpu").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                let mem =
                                    cd.get("memory").and_then(|v| v.as_u64()).map(|m| m as u32);
                                let essential = cd
                                    .get("essential")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true);
                                Some(ContainerDefinition {
                                    name,
                                    image,
                                    cpu: cpu_val,
                                    memory: mem,
                                    essential,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let mut store = self.store.get_or_create(account_id, region);
                let revision = {
                    let rev = store.task_def_revisions.entry(family.clone()).or_insert(0);
                    *rev += 1;
                    *rev
                };
                let arn = task_def_arn(account_id, region, &family, revision);
                let td = TaskDefinition {
                    family: family.clone(),
                    revision,
                    task_definition_arn: arn.clone(),
                    status: "ACTIVE".to_string(),
                    container_definitions: container_defs,
                    cpu,
                    memory,
                    network_mode,
                    registered_at: Utc::now(),
                };
                store.task_definitions.insert(arn, td.clone());
                Ok(json_ok(json!({ "taskDefinition": task_def_json(&td) })))
            }

            // ----------------------------------------------------------------
            // DeregisterTaskDefinition
            // ----------------------------------------------------------------
            "DeregisterTaskDefinition" => {
                let task_def_ref = match str_param(ctx, "taskDefinition") {
                    Some(t) => t,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "taskDefinition is required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                // Accept both family:revision ARN and short form
                let arn = if task_def_ref.starts_with("arn:") {
                    task_def_ref.clone()
                } else {
                    // Look up by family:revision key
                    store
                        .task_definitions
                        .values()
                        .find(|td| format!("{}:{}", td.family, td.revision) == task_def_ref)
                        .map(|td| td.task_definition_arn.clone())
                        .unwrap_or(task_def_ref.clone())
                };
                match store.task_definitions.get_mut(&arn) {
                    Some(td) => {
                        td.status = "INACTIVE".to_string();
                        let td_clone = td.clone();
                        Ok(json_ok(
                            json!({ "taskDefinition": task_def_json(&td_clone) }),
                        ))
                    }
                    None => Ok(json_error(
                        "InvalidParameterException",
                        &format!("Task definition {task_def_ref} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeTaskDefinition
            // ----------------------------------------------------------------
            "DescribeTaskDefinition" => {
                let task_def_ref = match str_param(ctx, "taskDefinition") {
                    Some(t) => t,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "taskDefinition is required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_error(
                        "InvalidParameterException",
                        &format!("Task definition {task_def_ref} not found"),
                        400,
                    ));
                };
                // Accept ARN or family:revision
                let td = if task_def_ref.starts_with("arn:") {
                    store.task_definitions.get(&task_def_ref)
                } else {
                    store.task_definitions.values().find(|td| {
                        format!("{}:{}", td.family, td.revision) == task_def_ref
                            || td.family == task_def_ref
                    })
                };
                match td {
                    Some(t) => Ok(json_ok(json!({ "taskDefinition": task_def_json(t) }))),
                    None => Ok(json_error(
                        "InvalidParameterException",
                        &format!("Task definition {task_def_ref} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // ListTaskDefinitions
            // ----------------------------------------------------------------
            "ListTaskDefinitions" => {
                let family_prefix = str_param(ctx, "familyPrefix");
                let arns: Vec<String> = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        store
                            .task_definitions
                            .values()
                            .filter(|td| {
                                td.status == "ACTIVE"
                                    && family_prefix
                                        .as_deref()
                                        .map(|p| td.family.starts_with(p))
                                        .unwrap_or(true)
                            })
                            .map(|td| td.task_definition_arn.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json_ok(
                    json!({ "taskDefinitionArns": arns, "nextToken": null }),
                ))
            }

            // ----------------------------------------------------------------
            // CreateService
            // ----------------------------------------------------------------
            "CreateService" => {
                let service_name = match str_param(ctx, "serviceName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "serviceName is required",
                            400,
                        ));
                    }
                };
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());
                let task_definition = match str_param(ctx, "taskDefinition") {
                    Some(t) => t,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "taskDefinition is required",
                            400,
                        ));
                    }
                };
                let desired_count = ctx
                    .request_body
                    .get("desiredCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;

                let c_arn = if cluster_ref.starts_with("arn:") {
                    cluster_ref.clone()
                } else {
                    cluster_arn(account_id, region, &cluster_ref)
                };
                let s_arn = service_arn(account_id, region, &cluster_ref, &service_name);

                let mut store = self.store.get_or_create(account_id, region);
                if store.services.contains_key(&s_arn) {
                    return Ok(json_error(
                        "InvalidParameterException",
                        &format!("Service {service_name} already exists"),
                        400,
                    ));
                }
                let svc = Service {
                    service_name: service_name.clone(),
                    service_arn: s_arn.clone(),
                    cluster_arn: c_arn,
                    task_definition,
                    desired_count,
                    running_count: 0,
                    pending_count: 0,
                    status: "ACTIVE".to_string(),
                    created: Utc::now(),
                };
                if let Some(cluster) = store.clusters.values_mut().find(|c| {
                    c.cluster_name == cluster_ref
                        || c.cluster_arn == cluster_arn(account_id, region, &cluster_ref)
                }) {
                    cluster.active_services_count += 1;
                }
                store.services.insert(s_arn, svc.clone());
                Ok(json_ok(json!({ "service": service_json(&svc) })))
            }

            // ----------------------------------------------------------------
            // DeleteService
            // ----------------------------------------------------------------
            "DeleteService" => {
                let service_ref = match str_param(ctx, "service") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "service is required",
                            400,
                        ));
                    }
                };
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());
                let s_arn = if service_ref.starts_with("arn:") {
                    service_ref.clone()
                } else {
                    service_arn(account_id, region, &cluster_ref, &service_ref)
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.services.remove(&s_arn) {
                    Some(svc) => Ok(json_ok(json!({ "service": service_json(&svc) }))),
                    None => Ok(json_error(
                        "ServiceNotFoundException",
                        &format!("Service {service_ref} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // UpdateService
            // ----------------------------------------------------------------
            "UpdateService" => {
                let service_ref = match str_param(ctx, "service") {
                    Some(s) => s,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "service is required",
                            400,
                        ));
                    }
                };
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());
                let s_arn = if service_ref.starts_with("arn:") {
                    service_ref.clone()
                } else {
                    service_arn(account_id, region, &cluster_ref, &service_ref)
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.services.get_mut(&s_arn) {
                    Some(svc) => {
                        if let Some(desired) = ctx
                            .request_body
                            .get("desiredCount")
                            .and_then(|v| v.as_u64())
                        {
                            svc.desired_count = desired as u32;
                        }
                        if let Some(td) = str_param(ctx, "taskDefinition") {
                            svc.task_definition = td;
                        }
                        let svc_clone = svc.clone();
                        Ok(json_ok(json!({ "service": service_json(&svc_clone) })))
                    }
                    None => Ok(json_error(
                        "ServiceNotFoundException",
                        &format!("Service {service_ref} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeServices
            // ----------------------------------------------------------------
            "DescribeServices" => {
                let service_refs: Vec<String> = ctx
                    .request_body
                    .get("services")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "services": [], "failures": [] })));
                };

                let services: Vec<Value> = service_refs
                    .iter()
                    .filter_map(|r| {
                        let s_arn = if r.starts_with("arn:") {
                            r.clone()
                        } else {
                            service_arn(account_id, region, &cluster_ref, r)
                        };
                        store.services.get(&s_arn).map(service_json)
                    })
                    .collect();

                Ok(json_ok(json!({ "services": services, "failures": [] })))
            }

            // ----------------------------------------------------------------
            // ListServices
            // ----------------------------------------------------------------
            "ListServices" => {
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());
                let c_arn = if cluster_ref.starts_with("arn:") {
                    cluster_ref.clone()
                } else {
                    cluster_arn(account_id, region, &cluster_ref)
                };
                let arns: Vec<String> = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        store
                            .services
                            .values()
                            .filter(|s| s.cluster_arn == c_arn)
                            .map(|s| s.service_arn.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json_ok(json!({ "serviceArns": arns, "nextToken": null })))
            }

            // ----------------------------------------------------------------
            // RunTask
            // ----------------------------------------------------------------
            "RunTask" => {
                let task_def_ref = match str_param(ctx, "taskDefinition") {
                    Some(t) => t,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "taskDefinition is required",
                            400,
                        ));
                    }
                };
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());
                let count = ctx
                    .request_body
                    .get("count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                let group = str_param(ctx, "group");

                let c_arn = if cluster_ref.starts_with("arn:") {
                    cluster_ref.clone()
                } else {
                    cluster_arn(account_id, region, &cluster_ref)
                };

                // Resolve task definition ARN
                let td_arn = {
                    let Some(store) = self.store.get(account_id, region) else {
                        return Ok(json_error(
                            "InvalidParameterException",
                            &format!("Task definition {task_def_ref} not found"),
                            400,
                        ));
                    };
                    if task_def_ref.starts_with("arn:") {
                        if store.task_definitions.contains_key(&task_def_ref) {
                            task_def_ref.clone()
                        } else {
                            return Ok(json_error(
                                "InvalidParameterException",
                                &format!("Task definition {task_def_ref} not found"),
                                400,
                            ));
                        }
                    } else {
                        store
                            .task_definitions
                            .values()
                            .filter(|td| {
                                td.family == task_def_ref
                                    || format!("{}:{}", td.family, td.revision) == task_def_ref
                            })
                            .max_by_key(|td| td.revision)
                            .map(|td| td.task_definition_arn.clone())
                            .unwrap_or_else(|| task_def_ref.clone())
                    }
                };

                let mut store = self.store.get_or_create(account_id, region);
                let mut tasks = Vec::new();
                for _ in 0..count {
                    let task_id = Uuid::new_v4().to_string();
                    let t_arn = task_arn(account_id, region, &task_id);
                    let task = Task {
                        task_arn: t_arn.clone(),
                        cluster_arn: c_arn.clone(),
                        task_definition_arn: td_arn.clone(),
                        last_status: "RUNNING".to_string(),
                        desired_status: "RUNNING".to_string(),
                        group: group.clone(),
                        started_at: Some(Utc::now()),
                        stopped_at: None,
                        stop_code: None,
                        stopped_reason: None,
                        created: Utc::now(),
                    };
                    tasks.push(task_json(&task));
                    store.tasks.insert(t_arn, task);
                }
                Ok(json_ok(json!({ "tasks": tasks, "failures": [] })))
            }

            // ----------------------------------------------------------------
            // StopTask
            // ----------------------------------------------------------------
            "StopTask" => {
                let task_ref = match str_param(ctx, "task") {
                    Some(t) => t,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "task is required",
                            400,
                        ));
                    }
                };
                let reason = str_param(ctx, "reason");
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());

                let t_arn = if task_ref.starts_with("arn:") {
                    task_ref.clone()
                } else {
                    task_arn(account_id, region, &task_ref)
                };
                let c_arn = if cluster_ref.starts_with("arn:") {
                    cluster_ref.clone()
                } else {
                    cluster_arn(account_id, region, &cluster_ref)
                };

                let mut store = self.store.get_or_create(account_id, region);
                match store.tasks.get_mut(&t_arn) {
                    Some(task) if task.cluster_arn == c_arn || task.task_arn == t_arn => {
                        task.last_status = "STOPPED".to_string();
                        task.desired_status = "STOPPED".to_string();
                        task.stopped_at = Some(Utc::now());
                        task.stop_code = Some("UserInitiated".to_string());
                        task.stopped_reason =
                            reason.or_else(|| Some("Task stopped by user".to_string()));
                        let task_clone = task.clone();
                        Ok(json_ok(json!({ "task": task_json(&task_clone) })))
                    }
                    _ => Ok(json_error(
                        "InvalidParameterException",
                        &format!("Task {task_ref} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeTasks
            // ----------------------------------------------------------------
            "DescribeTasks" => {
                let task_refs: Vec<String> = ctx
                    .request_body
                    .get("tasks")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());
                let c_arn = if cluster_ref.starts_with("arn:") {
                    cluster_ref.clone()
                } else {
                    cluster_arn(account_id, region, &cluster_ref)
                };

                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "tasks": [], "failures": [] })));
                };

                let tasks: Vec<Value> = task_refs
                    .iter()
                    .filter_map(|r| {
                        let t_arn = if r.starts_with("arn:") {
                            r.clone()
                        } else {
                            task_arn(account_id, region, r)
                        };
                        store
                            .tasks
                            .get(&t_arn)
                            .filter(|t| t.cluster_arn == c_arn || task_refs.len() == 1)
                            .map(task_json)
                    })
                    .collect();

                Ok(json_ok(json!({ "tasks": tasks, "failures": [] })))
            }

            // ----------------------------------------------------------------
            // ListTasks
            // ----------------------------------------------------------------
            "ListTasks" => {
                let cluster_ref =
                    str_param(ctx, "cluster").unwrap_or_else(|| "default".to_string());
                let c_arn = if cluster_ref.starts_with("arn:") {
                    cluster_ref.clone()
                } else {
                    cluster_arn(account_id, region, &cluster_ref)
                };
                let desired_status = str_param(ctx, "desiredStatus");

                let arns: Vec<String> = self
                    .store
                    .get(account_id, region)
                    .map(|store| {
                        store
                            .tasks
                            .values()
                            .filter(|t| t.cluster_arn == c_arn)
                            .filter(|t| {
                                desired_status
                                    .as_deref()
                                    .map(|s| t.desired_status == s)
                                    .unwrap_or(true)
                            })
                            .map(|t| t.task_arn.clone())
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(json_ok(json!({ "taskArns": arns, "nextToken": null })))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut clusters = Vec::new();
        for entry in self.store.iter() {
            for cluster in entry.value().clusters.values() {
                clusters.push(json!({
                    "id": cluster.cluster_arn, "kind": "cluster",
                    "attributes": [
                        {"key": "name", "value": cluster.cluster_name.clone()},
                        {"key": "status", "value": cluster.status.clone()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "ecs", "clusters": clusters }))
    }
}
