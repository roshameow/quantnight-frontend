//! Original GUI task-list projection, hosted next to the original task owner.
//! This is not CLI list: remote-only display and generation refresh diagnostics remain distinct.
use crate::{
    create_generate::GenerationRefresh, runtime::TaskRuntimeContext, task_control::TaskControl,
};
use bson::{doc, oid::ObjectId, Document};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::time::timeout;

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskControlSource {
    Local,
    RemoteOnly,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Task {
    #[serde(rename = "_id")]
    pub id: ObjectId, // 用 id 显得更自然一些，但必须 rename "_id"
    pub name: String,
    #[serde(default, skip_deserializing, rename = "controlSource")]
    pub control_source: Option<TaskControlSource>,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub templatefile: String,
    pub auth_profile: Option<String>,
    pub status: String,
    // Persisted generation outcome is independent of the task control status.
    // Optional for old task records; retain it through the actual list DTO.
    #[serde(default)]
    pub generation: Option<JsonValue>,
    #[serde(default, rename = "lastError")]
    pub last_error: Option<String>,
    // Response-only refresh diagnostic. Stored data cannot impersonate this result
    // and a probe error never overwrites the persisted generation outcome.
    #[serde(
        default,
        skip_deserializing,
        skip_serializing_if = "Option::is_none",
        rename = "generationRefreshError"
    )]
    pub generation_refresh_error: Option<String>,
    #[serde(rename = "isRemote")] // 👈 加上这个
    pub is_remote: Option<bool>, // 可选字段
    #[serde(default, rename = "targetRemote")]
    pub target_remote: Option<bool>,
    #[serde(rename = "taskType")] // 👈 加上这个
    pub task_type: Option<String>, // 或者 task_type: Option<String>
    #[serde(rename = "mission_config")]
    pub mission_config: Option<JsonValue>, // 加上这一行，接收嵌套的 config
    #[serde(rename = "startConfig")]
    pub start_config: Option<JsonValue>,
    #[serde(default, rename = "launchAttemptId")]
    pub launch_attempt_id: Option<String>,
    #[serde(default, rename = "launchSettledAttemptId")]
    pub launch_settled_attempt_id: Option<String>,
    pub config: Option<JsonValue>,
    #[serde(default, rename = "stopConfirmedAt")]
    pub stop_confirmed_at: Option<bson::DateTime>,
    #[serde(default, rename = "stopConfirmedRemote")]
    pub stop_confirmed_remote: Option<bool>,
    #[serde(default, rename = "stopConfirmedTaskId")]
    pub stop_confirmed_task_id: Option<String>,
    #[serde(default, rename = "stopConfirmationBasis")]
    pub stop_confirmation_basis: Option<String>,
}

// This source is a property of the original read branch, never of stored data.
fn listed_task(document: Document, source: TaskControlSource) -> Result<Task, bson::de::Error> {
    let mut task = bson::from_document::<Task>(document)?;
    task.control_source = Some(source);
    Ok(task)
}

// Keep the original GUI list namespace; do not silently reroute its reads.
const GUI_TASK_MISSION_DATABASE: &str = "simulation_mission";

// The facade callback is lazy: ordinary/remote-only rows never initialize a
// control runtime or probe processes. This same orchestration is tested offline.
async fn append_local_listed_task<F, Fut>(
    tasks: &mut Vec<Task>,
    seen_names: &mut std::collections::HashSet<String>,
    document: Document,
    configured_mission_database: &str,
    reconcile: F,
) where
    F: FnOnce(Document) -> Fut,
    Fut: std::future::Future<Output = Result<GenerationRefresh, String>>,
{
    let Ok(mut task) = listed_task(document.clone(), TaskControlSource::Local) else {
        return;
    };
    // Reserve the local snapshot's name even if the exact record disappears
    // during reconciliation. Never resurrect a cached remote duplicate.
    seen_names.insert(task.name.clone());
    if task.status == "generating"
        && document
            .get_document("generation")
            .ok()
            .and_then(|generation| generation.get_str("phase").ok())
            == Some("writing")
    {
        // TaskControlState uses this same verified config for its store. An
        // original hardcoded GUI namespace must match before any mutation.
        let result = if configured_mission_database != GUI_TASK_MISSION_DATABASE {
            Err("GUI task-list namespace differs from the verified core binding; generation reconciliation skipped".into())
        } else {
            reconcile(document).await
        };
        match result {
            Ok(GenerationRefresh { task: None, .. }) => return,
            Ok(GenerationRefresh { task: Some(current), diagnostic, .. }) => {
                match listed_task(current, TaskControlSource::Local) {
                    Ok(mut current) if current.id == task.id && current.name == task.name => {
                        current.generation_refresh_error = diagnostic;
                        task = current;
                    }
                    Ok(_) => task.generation_refresh_error = Some(
                        "generation refresh returned another task identity; original snapshot retained".into()),
                    Err(error) => task.generation_refresh_error = Some(
                        format!("generation refresh document could not be displayed; original snapshot retained: {error}")),
                }
            }
            Err(error) => task.generation_refresh_error = Some(error),
        }
        if let Some(error) = &task.generation_refresh_error {
            log::warn!(
                "Generation refresh unconfirmed for {} ({}): {}",
                task.name,
                task.id,
                error
            );
        }
    }
    tasks.push(task);
}

fn merge_remote_listed_tasks(
    tasks: &mut Vec<Task>,
    seen_names: &mut std::collections::HashSet<String>,
    remote_tasks: Vec<Task>,
) {
    for task in remote_tasks {
        // Local names, including deactive records, shadow cached remote rows.
        if seen_names.insert(task.name.clone()) {
            tasks.push(task);
        }
    }
}

/// 远程任务列表缓存：短 TTL，避免每次本地操作都穿透 SSH 隧道查远程库。
/// 缓存的是“远程独有任务”（不含本地任务），读取时再与本地合并去重。
/// 隧道不可用/超时/失败时返回空，不影响本地任务展示。
struct RemoteTaskCache {
    fetched_at: Instant,
    remote_only_tasks: Vec<Task>,
}

/// Per-runtime projection cache, never shared across backend/config identities.
#[derive(Default)]
pub(crate) struct QueryState {
    remote: Mutex<Option<RemoteTaskCache>>,
}

/// 远程查询 TTL：窗口内重复调用直接读缓存，不穿透隧道
const REMOTE_TASK_CACHE_TTL: Duration = Duration::from_secs(10);
/// 远程查询硬超时：隧道慢/挂时快速失败，避免每次操作被拖住（驱动默认 server selection 可达 30s）
const REMOTE_TASK_TIMEOUT: Duration = Duration::from_secs(2);

/// 从远程 mission.tasks 拉取任务列表（带 TTL 缓存 + 超时）。
/// 远程不可达/超时时静默返回空，不影响本地任务展示。
async fn fetch_remote_tasks(context: &TaskRuntimeContext) -> Vec<Task> {
    // 本地/远程指向同一个库时无需重复查询
    if context.config().mongodb.remote_uri == context.config().mongodb.local_uri {
        return Vec::new();
    }

    // 命中缓存直接返回
    {
        let cache = &context.gui_queries().remote;
        if let Ok(guard) = cache.lock() {
            if let Some(c) = guard.as_ref() {
                if c.fetched_at.elapsed() < REMOTE_TASK_CACHE_TTL {
                    return c.remote_only_tasks.clone();
                }
            }
        }
    }

    // 网络请求限定时间，避免隧道不可用时每次操作都卡住
    let result = timeout(REMOTE_TASK_TIMEOUT, async {
        let remote_client = context.client(true).await?;
        let remote_db = remote_client.database("simulation_mission");
        let remote_collection = remote_db.collection::<Document>("tasks");

        let mut remote_cursor = remote_collection
            .find(None, None)
            .await
            .map_err(|e| e.to_string())?;
        let mut remote_tasks = Vec::new();
        while let Some(mut document) = remote_cursor.try_next().await.map_err(|e| e.to_string())? {
            // 远程来源统一标记 isRemote = true
            document.insert("isRemote", true);
            if let Ok(task) = listed_task(document, TaskControlSource::RemoteOnly) {
                remote_tasks.push(task);
            }
        }
        Ok::<Vec<Task>, String>(remote_tasks)
    })
    .await;

    match result {
        Ok(Ok(remote_tasks)) => {
            let cache = &context.gui_queries().remote;
            if let Ok(mut guard) = cache.lock() {
                *guard = Some(RemoteTaskCache {
                    fetched_at: Instant::now(),
                    remote_only_tasks: remote_tasks.clone(),
                });
            }
            remote_tasks
        }
        _ => {
            // 超时或连接失败：静默跳过，不影响本地任务展示
            eprintln!(
                "⚠️ 远程任务查询失败或超时（{}s），本次跳过远程聚合",
                REMOTE_TASK_TIMEOUT.as_secs()
            );
            Vec::new()
        }
    }
}

/// Executed only by the shared dispatcher against its checked task context.
pub async fn list(
    context: &TaskRuntimeContext,
    control: &TaskControl,
) -> Result<JsonValue, String> {
    context.verify_current().await?;
    let config = context.config();
    let local = context.client(false).await?;
    let collection = local
        .database(GUI_TASK_MISSION_DATABASE)
        .collection::<Document>("tasks");
    let mut cursor = collection
        .find(None, None)
        .await
        .map_err(|e| e.to_string())?;
    let mut tasks = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    while let Some(document) = cursor.try_next().await.map_err(|e| e.to_string())? {
        append_local_listed_task(
            &mut tasks,
            &mut seen_names,
            document,
            &config.mongodb.databases.mission,
            |observed| async move { control.reconcile_generation_for_list(&observed).await },
        )
        .await;
    }
    let remote_tasks = fetch_remote_tasks(context).await;
    merge_remote_listed_tasks(&mut tasks, &mut seen_names, remote_tasks);
    serde_json::to_value(tasks).map_err(|e| e.to_string())
}

/// The response-only fields must not be accepted from persisted BSON. Restore
/// them only when decoding this specific, checked dispatcher response.
pub fn decode_list(value: JsonValue) -> Result<Vec<Task>, String> {
    let rows = value
        .as_array()
        .ok_or("GUI task list response is not an array")?;
    rows.iter()
        .map(|row| {
            let mut task: Task = serde_json::from_value(row.clone()).map_err(|e| e.to_string())?;
            task.control_source = match row.get("controlSource").and_then(|v| v.as_str()) {
                Some("local") => Some(TaskControlSource::Local),
                Some("remote_only") => Some(TaskControlSource::RemoteOnly),
                _ => return Err("GUI task list response has no verified control source".into()),
            };
            task.generation_refresh_error = row
                .get("generationRefreshError")
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or("invalid generation refresh diagnostic")
                })
                .transpose()?;
            Ok(task)
        })
        .collect()
}

#[cfg(test)]
mod task_control_source_tests {
    use super::*;
    #[test]
    fn listed_generation_failure_survives_actual_bson_to_task_to_json() {
        let id = ObjectId::parse_str("0123456789abcdef01234567").unwrap();
        let generation = doc! {"id":"attempt-original","phase":"generation_failed",
        "producerPid":1234,"startedAt":"2026-10-31T00:00:00Z","finishedAt":"2026-10-31T00:00:01Z"};
        let original_error = "writer exited 17\noriginal cause: invalid template";
        for status in [
            "pending",
            "ready",
            "generation_failed",
            "generation_unknown",
        ] {
            for source in [TaskControlSource::Local, TaskControlSource::RemoteOnly] {
                let row = doc! {"_id":id,"name":"generation-fixture","status":status,
                "isRemote":false,"taskType":"regular","generation":generation.clone(),
                "lastError":original_error};
                let listed = listed_task(row, source.clone()).unwrap();
                let wire = serde_json::to_value(listed).unwrap();
                assert_eq!(wire["_id"]["$oid"], id.to_hex());
                assert_eq!(wire["status"], status); // Never promote failure to ready in a DTO.
                assert_eq!(
                    wire["generation"],
                    serde_json::to_value(&generation).unwrap()
                );
                assert_eq!(wire["lastError"], original_error);
                assert_eq!(wire["controlSource"], serde_json::to_value(source).unwrap());
                assert!(wire.get("last_error").is_none());
            }
        }
    }
    #[test]
    fn listed_generation_optional_legacy_and_new_attempt_data_are_not_reinterpreted() {
        let base = doc! {"_id":ObjectId::new(),"name":"generation-fixture","status":"pending"};
        for optional in [
            doc! {},
            doc! {"generation":bson::Bson::Null,"lastError":bson::Bson::Null},
        ] {
            let mut row = base.clone();
            row.extend(optional);
            let wire =
                serde_json::to_value(listed_task(row, TaskControlSource::Local).unwrap()).unwrap();
            assert_eq!(wire["status"], "pending");
            assert!(wire["generation"].is_null());
            assert!(wire["lastError"].is_null());
        }
        for (status, phase) in [
            ("generating", "writing"),
            ("ready", "complete"),
            ("pending", "prepared"),
            ("generation_unknown", "generation_unknown"),
        ] {
            let mut row = base.clone();
            row.extend(
                doc! {"status":status,"generation":{"id":"attempt-new","phase":phase},
                "lastError":"previous error retained by owner"},
            );
            let wire =
                serde_json::to_value(listed_task(row, TaskControlSource::Local).unwrap()).unwrap();
            assert_eq!(wire["status"], status);
            assert_eq!(wire["generation"]["id"], "attempt-new");
            assert_eq!(wire["generation"]["phase"], phase);
            assert_eq!(wire["lastError"], "previous error retained by owner");
        }
    }
    #[test]
    fn list_source_is_assigned_by_original_read_branch_not_db_self_claim() {
        for forged in [
            bson::Bson::String("local".into()),
            bson::Bson::String("remote_only".into()),
            bson::Bson::Boolean(true),
            bson::Bson::Document(doc! {"local": true}),
        ] {
            let row = doc! {"_id": ObjectId::new(), "name":"source-fixture", "status":"ready", "isRemote":true,
            "controlSource":forged};
            let local = listed_task(row.clone(), TaskControlSource::Local).unwrap();
            assert_eq!(local.control_source, Some(TaskControlSource::Local));
            assert_eq!(local.is_remote, Some(true)); // remote execution is still locally controllable
            let remote = listed_task(row.clone(), TaskControlSource::RemoteOnly).unwrap();
            assert_eq!(remote.control_source, Some(TaskControlSource::RemoteOnly));
            assert_eq!(
                serde_json::to_value(remote).unwrap()["controlSource"],
                "remote_only"
            );
            let unclassified: Task = bson::from_document(row).unwrap();
            assert!(unclassified.control_source.is_none());
        }
    }
    #[test]
    fn checked_response_roundtrip_retains_source_refresh_warning_and_bson_shape() {
        let id = ObjectId::new();
        let at = bson::DateTime::from_millis(1_794_000_000_123);
        for source in [TaskControlSource::Local, TaskControlSource::RemoteOnly] {
            let document = doc! {"_id":id,"name":"exact","status":"generating","isRemote":true,
            "launchAttemptId":"original-launch", "stopConfirmedAt":at,
            "generation":{"phase":"writing","producerPid":1234_i64},
            "mission_config":{"opaque":{"$date":"literal config, not a BSON date"},"limit":9_007_199_254_740_991_i64}};
            let mut task = listed_task(document, source.clone()).unwrap();
            task.generation_refresh_error = Some("original refresh not confirmed".into());
            let expected = serde_json::to_value(&task).unwrap();
            let returned = decode_list(serde_json::json!([expected.clone()]))
                .unwrap()
                .remove(0);
            assert_eq!(returned.id, id);
            assert_eq!(returned.control_source, Some(source));
            assert_eq!(returned.stop_confirmed_at, Some(at));
            assert_eq!(
                returned.generation_refresh_error.as_deref(),
                Some("original refresh not confirmed")
            );
            assert_eq!(serde_json::to_value(returned).unwrap(), expected);
        }
    }

    #[test]
    fn list_wire_is_not_a_second_persisted_document_authority() {
        let document = doc! {"_id":ObjectId::new(),"name":"exact","status":"pending",
        "controlSource":"remote_only","generationRefreshError":"stored forgery"};
        let raw: Task = bson::from_document(document).unwrap();
        assert!(raw.control_source.is_none());
        assert!(raw.generation_refresh_error.is_none());
        assert!(decode_list(serde_json::json!([serde_json::to_value(raw).unwrap()])).is_err());
        assert!(decode_list(serde_json::json!({"ok":true})).is_err());
        let valid = listed_task(
            doc! {"_id":ObjectId::new(),"name":"exact","status":"pending"},
            TaskControlSource::Local,
        )
        .unwrap();
        let mut invalid = serde_json::to_value(valid).unwrap();
        invalid["generationRefreshError"] = serde_json::json!(false);
        assert!(decode_list(serde_json::json!([invalid])).is_err());
    }
}

#[cfg(test)]
mod task_list_refresh_tests {
    use super::*;
    use std::collections::HashSet;
    use std::{fs, path::Path, sync::Arc};

    fn row(name: &str, status: &str) -> Document {
        doc! {"_id": ObjectId::parse_str("0123456789abcdef01234567").unwrap(),
        "name": name, "status": status, "isRemote": false, "taskType": "regular"}
    }
    fn writing(name: &str) -> Document {
        let mut task = row(name, "generating");
        task.insert(
            "generation",
            doc! {"id":"original-attempt","phase":"writing"},
        );
        task
    }
    fn result(
        task: Option<Document>,
        changed: bool,
        diagnostic: Option<&str>,
    ) -> GenerationRefresh {
        GenerationRefresh {
            task,
            changed,
            diagnostic: diagnostic.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn ordinary_and_nonwriting_rows_never_initialize_or_probe_control() {
        let mut tasks = Vec::new();
        let mut seen = HashSet::new();
        for status in [
            "pending",
            "waiting",
            "ready",
            "running",
            "paused",
            "completed",
            "completed_with_errors",
            "generation_failed",
            "generation_unknown",
            "deactive",
        ] {
            let mut document = writing(status);
            document.insert("status", status);
            append_local_listed_task(
                &mut tasks,
                &mut seen,
                document,
                "other-namespace",
                |_| async { panic!("ordinary list must not acquire control/probe processes") },
            )
            .await;
        }
        for phase in [None, Some("complete"), Some("generation_unknown")] {
            let mut document = row("nonwriting", "generating");
            if let Some(phase) = phase {
                document.insert("generation", doc! {"phase":phase});
            }
            append_local_listed_task(
                &mut tasks,
                &mut seen,
                document,
                GUI_TASK_MISSION_DATABASE,
                |_| async { panic!("nonwriting generation must not be reconciled by list") },
            )
            .await;
        }
        assert_eq!(tasks.len(), 13);
        assert!(tasks
            .iter()
            .all(|task| task.generation_refresh_error.is_none()));
        append_local_listed_task(
            &mut tasks,
            &mut seen,
            doc! {"name":"malformed","status":"generating"},
            GUI_TASK_MISSION_DATABASE,
            |_| async { panic!("invalid GUI row must retain original skip behavior") },
        )
        .await;
        assert_eq!(tasks.len(), 13);
        assert!(!seen.contains("malformed"));
    }

    #[tokio::test]
    async fn local_remote_execution_passes_full_original_snapshot_and_preserves_rich_dto() {
        let mut original = writing("rich-local");
        original.extend(
            doc! {"isRemote":true,"targetRemote":true,"auth_profile":"user1",
            "mission_config":{"workers":3},"startConfig":{"workers":4},"config":{"workers":5},
            "launchAttemptId":"launch","stopConfirmedAt":bson::DateTime::from_millis(1234),
            "stopConfirmedRemote":true,"stopConfirmedTaskId":"0123456789abcdef01234567",
            "stopConfirmationBasis":"session_absent","lastError":"original error",
            "privateCoreFact":{"keep":"full snapshot"}},
        );
        let expected = original.clone();
        let mut tasks = Vec::new();
        let mut seen = HashSet::new();
        append_local_listed_task(&mut tasks, &mut seen, original, GUI_TASK_MISSION_DATABASE, |mut observed| async move {
            assert_eq!(observed, expected); // Not a lossy Task -> BSON roundtrip.
            observed.insert("status", "pending");
            observed.insert("generation", doc! {"id":"original-attempt","phase":"generation_failed","finishedAt":"original-time"});
            Ok(result(Some(observed), true, None))
        }).await;
        let wire = serde_json::to_value(&tasks[0]).unwrap();
        assert_eq!(wire["controlSource"], "local");
        assert_eq!(wire["isRemote"], true);
        assert_eq!(wire["status"], "pending");
        assert_eq!(wire["generation"]["id"], "original-attempt");
        assert_eq!(wire["generation"]["phase"], "generation_failed");
        assert_eq!(wire["lastError"], "original error");
        assert_eq!(wire["mission_config"]["workers"], 3);
        assert_eq!(wire["startConfig"]["workers"], 4);
        assert_eq!(wire["config"]["workers"], 5);
        assert_eq!(wire["launchAttemptId"], "launch");
        assert_eq!(tasks[0].stop_confirmed_at.unwrap().timestamp_millis(), 1234);
        assert_eq!(wire["stopConfirmedTaskId"], "0123456789abcdef01234567");
        assert_eq!(wire["stopConfirmationBasis"], "session_absent");
        assert!(wire.get("generationRefreshError").is_none());
    }

    #[tokio::test]
    async fn error_and_namespace_mismatch_keep_original_outcome_and_do_not_hide_other_rows() {
        let mut original = writing("failed-probe");
        original.insert("lastError", "persisted writer outcome");
        let mut tasks = Vec::new();
        let mut seen = HashSet::new();
        append_local_listed_task(
            &mut tasks,
            &mut seen,
            original.clone(),
            GUI_TASK_MISSION_DATABASE,
            |_| async { Err("fixture process observation failed".into()) },
        )
        .await;
        append_local_listed_task(
            &mut tasks,
            &mut seen,
            row("unrelated", "ready"),
            GUI_TASK_MISSION_DATABASE,
            |_| async { panic!("unrelated task was probed") },
        )
        .await;
        append_local_listed_task(
            &mut tasks,
            &mut seen,
            original,
            "different-mission",
            |_| async { panic!("namespace mismatch reached mutating facade") },
        )
        .await;
        assert_eq!(tasks.len(), 3);
        for task in [&tasks[0], &tasks[2]] {
            assert_eq!(task.status, "generating");
            assert_eq!(task.last_error.as_deref(), Some("persisted writer outcome"));
            assert_eq!(task.generation.as_ref().unwrap()["phase"], "writing");
        }
        assert_eq!(
            tasks[0].generation_refresh_error.as_deref(),
            Some("fixture process observation failed")
        );
        assert!(tasks[1].generation_refresh_error.is_none());
        assert!(tasks[2]
            .generation_refresh_error
            .as_ref()
            .unwrap()
            .contains("namespace"));
    }

    #[tokio::test]
    async fn unchanged_and_diagnostic_are_response_facts_never_reinterpreted_as_generation_failure()
    {
        let mut tasks = Vec::new();
        let mut seen = HashSet::new();
        for diagnostic in [None, Some("owner evidence unavailable")] {
            append_local_listed_task(
                &mut tasks,
                &mut seen,
                writing("current"),
                GUI_TASK_MISSION_DATABASE,
                |observed| async move { Ok(result(Some(observed), false, diagnostic)) },
            )
            .await;
        }
        assert!(tasks
            .iter()
            .all(|task| task.status == "generating" && task.last_error.is_none()));
        assert!(tasks[0].generation_refresh_error.is_none());
        assert_eq!(
            tasks[1].generation_refresh_error.as_deref(),
            Some("owner evidence unavailable")
        );
        let wire = serde_json::to_value(&tasks[1]).unwrap();
        assert_eq!(wire["generationRefreshError"], "owner evidence unavailable");
    }

    #[tokio::test]
    async fn original_merge_keeps_local_deactive_and_disappeared_name_precedence_remote_only_is_untouched(
    ) {
        let mut tasks = Vec::new();
        let mut seen = HashSet::new();
        append_local_listed_task(
            &mut tasks,
            &mut seen,
            row("hidden", "deactive"),
            GUI_TASK_MISSION_DATABASE,
            |_| async { panic!("deactive probe") },
        )
        .await;
        append_local_listed_task(
            &mut tasks,
            &mut seen,
            writing("gone"),
            GUI_TASK_MISSION_DATABASE,
            |_| async { Ok(result(None, false, None)) },
        )
        .await;
        let remote: Vec<Task> = ["hidden", "gone", "remote-only", "remote-only"]
            .into_iter()
            .map(|name| {
                let mut document = writing(name);
                document.insert("isRemote", true);
                listed_task(document, TaskControlSource::RemoteOnly).unwrap()
            })
            .collect();
        merge_remote_listed_tasks(&mut tasks, &mut seen, remote);
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.name.as_str())
                .collect::<Vec<_>>(),
            vec!["hidden", "remote-only"]
        );
        assert_eq!(tasks[0].status, "deactive");
        assert_eq!(tasks[1].status, "generating");
        assert_eq!(tasks[1].control_source, Some(TaskControlSource::RemoteOnly));
        assert!(tasks[1].generation_refresh_error.is_none());
        assert_eq!(tasks[1].is_remote, Some(true));
    }

    #[tokio::test]
    async fn other_identity_or_invalid_return_keeps_original_with_diagnostic_not_adoption() {
        let mut tasks = Vec::new();
        let mut seen = HashSet::new();
        let mut other_id = writing("original");
        other_id.insert("_id", ObjectId::new());
        for replacement in [writing("another-name"), other_id, doc! {"status":"pending"}] {
            append_local_listed_task(
                &mut tasks,
                &mut seen,
                writing("original"),
                GUI_TASK_MISSION_DATABASE,
                |_| async move { Ok(result(Some(replacement), true, None)) },
            )
            .await;
        }
        assert!(tasks.iter().all(|task| task.name == "original"
            && task.status == "generating"
            && task.generation_refresh_error.is_some()));
    }

    // Adapter -> actual TaskControl -> actual generation owner. The original
    // inode is held, so no store/process observation is permitted. Only binding
    // export's lowest process IO is a fixture; no Mongo client is constructed.
    #[cfg(unix)]
    #[tokio::test]
    async fn real_facade_busy_lock_stays_observational_through_gui_adapter() {
        use crate::{
            backend_binding::validate_backend_binding,
            config::bash_test_support::FakeBash,
            mission_lock::MissionLocks,
            task_store::{CollectionSummary, Host, TaskStore},
        };
        use std::os::unix::fs::PermissionsExt;
        struct NoIo;
        #[async_trait::async_trait]
        impl TaskStore for NoIo {
            async fn find_task(
                &self,
                _: Host,
                _: Document,
                _: bool,
            ) -> Result<Option<Document>, String> {
                panic!("busy refresh read store")
            }
            async fn update_task(&self, _: Host, _: Document, _: Document) -> Result<u64, String> {
                panic!("busy refresh wrote store")
            }
            async fn collection_names(&self, _: Host) -> Result<Vec<String>, String> {
                panic!("collection scan")
            }
            async fn summary(&self, _: Host, _: &str) -> Result<CollectionSummary, String> {
                panic!("item summary")
            }
            async fn has_rows(&self, _: Host, _: &str) -> Result<bool, String> {
                panic!("inventory probe")
            }
            async fn rows(
                &self,
                _: Host,
                _: &str,
                _: Option<Document>,
            ) -> Result<Vec<Document>, String> {
                panic!("item scan")
            }
            async fn insert_rows(&self, _: Host, _: &str, _: Vec<Document>) -> Result<(), String> {
                panic!("item insert")
            }
            async fn rename_collection(&self, _: Host, _: &str, _: &str) -> Result<(), String> {
                panic!("archive")
            }
            async fn ping(&self, _: Host) -> Result<(), String> {
                panic!("DB ping")
            }
        }
        let mut fake = FakeBash::new("echo unexpected-process >> forbidden; exit 17");
        fake.config.mongodb.databases.mission = GUI_TASK_MISSION_DATABASE.into();
        let root = Path::new(&fake.config.env.working_dir);
        fs::write(root.join("credentials.yaml"), "synthetic binding only\n").unwrap();
        fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let python = root.join(".venv/bin/python");
        fs::write(&python, "#!/bin/sh\nprintf '%s\\n' '{\"schemaVersion\":1,\"command\":\"validate-task-control-binding\",\"ok\":true}'\n").unwrap();
        fs::set_permissions(&python, fs::Permissions::from_mode(0o700)).unwrap();
        let binding = validate_backend_binding(&fake.config).await.unwrap();
        let core = TaskControl::with_store(binding, Arc::new(NoIo));
        let _lock = MissionLocks::acquire(root, &["busy-local"], false).unwrap();
        let mut tasks = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..2 {
            append_local_listed_task(
                &mut tasks,
                &mut seen,
                writing("busy-local"),
                GUI_TASK_MISSION_DATABASE,
                |observed| {
                    let core = &core;
                    async move { core.reconcile_generation_for_list(&observed).await }
                },
            )
            .await;
        }
        assert_eq!(tasks.len(), 2);
        assert!(tasks
            .iter()
            .all(|task| task.status == "generating" && task.generation_refresh_error.is_none())); // Normal busy is a silent no-op, not an error.
        assert!(!root.join("forbidden").exists());
    }

    #[test]
    fn stored_diagnostic_is_ignored_for_both_sources_and_cannot_replace_last_error() {
        for source in [TaskControlSource::Local, TaskControlSource::RemoteOnly] {
            for forged in [
                bson::Bson::String("fake refresh failure".into()),
                bson::Bson::Boolean(true),
            ] {
                let mut document = writing("forged");
                document.insert("generationRefreshError", forged);
                document.insert("lastError", "original outcome");
                let task = listed_task(document, source.clone()).unwrap();
                assert!(task.generation_refresh_error.is_none());
                assert_eq!(task.last_error.as_deref(), Some("original outcome"));
                assert!(serde_json::to_value(task)
                    .unwrap()
                    .get("generationRefreshError")
                    .is_none());
            }
        }
    }
}
