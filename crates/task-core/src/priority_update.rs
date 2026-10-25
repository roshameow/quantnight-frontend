//! Priority configuration publication is not consumer-start confirmation.
use std::path::Path;
use async_trait::async_trait;
use bson::{doc, Bson, Document};
use serde_json::json;
use crate::{cleanup_sync::{outcome, TaskOutcome}, lifecycle_policy, mission_lock::MissionLocks,
    start::StartRuntime, task_store::{Host, TaskStore}};

#[derive(Clone, Debug)]
pub struct UpdatePriorityRequest {
    pub name: String,
    pub config_json: String,
    pub remote: Option<bool>,
    pub expected_task_id: Option<String>,
}
#[async_trait]
pub trait PriorityUpdateRuntime: StartRuntime {
    /// Original atomic config publisher and reload trigger; never launch.
    async fn publish_priority_config(&self, root: &Path, task: &Document, config: &Document, remote: bool) -> Result<(), String>;
}
fn fence(name: &str, task: &Document) -> Result<Document, String> {
    let mut filter = lifecycle_policy::task_binding_query(name, task, "priority-update")?;
    for key in ["taskType", "auth_profile", "targetRemote", "mission_config", "startConfig", "acceptedObservation",
        "launchAttemptAt", "startedAt", "stopConfirmedAt", "stopConfirmedRemote", "stopConfirmedTaskId",
        "stopConfirmationBasis", "generation", "startupBinding"] {
        filter.insert(key, task.get(key).cloned().unwrap_or_else(|| Bson::Document(doc!{"$exists":false})));
    }
    Ok(filter)
}

pub async fn update_priority(root: &Path, store: &dyn TaskStore, runtime: &dyn PriorityUpdateRuntime,
    request: &UpdatePriorityRequest) -> TaskOutcome {
    let mut attempted = false;
    let mut published = false;
    let result: Result<Document, String> = async {
        lifecycle_policy::validate_name(&request.name)?;
        let _lock = MissionLocks::acquire(root, &[&request.name], false)?;
        let task = store.find_task(Host::Local, doc!{"name":&request.name,"status":{"$ne":"deactive"}}, true)
            .await?.ok_or("Priority task not found")?;
        let id = task.get_object_id("_id").map_err(|_| "original Priority task ObjectId missing")?;
        if request.expected_task_id.as_ref().is_some_and(|expected| *expected != id.to_hex()) {
            return Err("Priority task identity changed before update".into());
        }
        if task.get_str("taskType").ok() != Some("priority") { return Err("configuration update requires a Priority task".into()); }
        lifecycle_policy::assert_observation_closed(&task)?;
        if matches!(task.get_str("status").ok(), Some("generating"|"generation_unknown"|"launch_unknown"|"syncing"|"deleting")) {
            return Err("task mutation is in progress or unconfirmed; reconcile before config publication".into());
        }
        let remote = match task.get("isRemote") {
            None | Some(Bson::Null | Bson::Boolean(false)) => false,
            Some(Bson::Boolean(true)) => true,
            _ => return Err("invalid original Priority execution host".into()),
        };
        if request.remote.is_some_and(|requested| requested != remote) {
            return Err("configuration update cannot change execution host; use original task start/sync controls".into());
        }
        // Same JSON parser/finite checks and professional validator as start.
        let selected = crate::start::saved_start_config(&task, Some(&request.config_json))?;
        let config = runtime.priority_config(root, &task, Some(selected.clone())).await?;
        crate::start::preserves(&selected, &config)?;
        let filter = fence(&request.name, &task)?;
        if store.find_task(Host::Local, filter.clone(), false).await?.is_none() {
            return Err("Priority control snapshot changed before config publication".into());
        }
        attempted = true;
        runtime.publish_priority_config(root, &task, &config, remote).await?;
        published = true;
        let at = chrono::Utc::now().to_rfc3339();
        let changed = store.update_task(Host::Local, filter, doc! {
            "$set":{"mission_config":&config,"updatedAt":&at},
            "$push":{"events":{"id":bson::Uuid::new().to_string().replace('-',""),"command":"priority-update",
                "status":"ok","at":&at,"detail":{"configPublished":true,"adoptionVerified":false}}}
        }).await?;
        if changed != 1 { return Err("configuration published but original control CAS changed; adoption unverified, do not restart".into()); }
        // Deliberately no status=running, host change or launch/stop reset.
        Ok(task)
    }.await;
    match result {
        Ok(task) => outcome("priority-update", &request.name, true, json!({"taskId":task.get_object_id("_id").unwrap().to_hex(),
            "taskStatus":task.get_str("status").ok(),"remote":task.get_bool("isRemote").unwrap_or(false),"configPublished":true,"adoptionVerified":false})),
        Err(error) => outcome("priority-update", &request.name, false, json!({"errorType":"RuntimeError","error":error,
            "publicationAttempted":attempted,"configPublished":if published {Some(true)} else if attempted {None} else {Some(false)},
            "adoptionVerified":false})),
    }
}
