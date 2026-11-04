//! GUI subscription adapter only. Core owns streams, cadence, queries and pools.
use crate::commands::{TaskControlState, PROGRESS_GAP_NOTICE};
use quantnight_task_core::runtime::{RuntimeEvent, RuntimeObserver};
use quantnight_task_core::task_progress::ProgressSink;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, State};

// This is only the GUI warning projection clock, not a task state machine or
// watcher owner. A local gap notice cannot be older than an observed host event.
fn subscription_observer(sink: ProgressSink) -> RuntimeObserver {
    let last_version = AtomicU64::new(0);
    Arc::new(move |event| match event {
        RuntimeEvent::GuiProgress { name, payload } => {
            if let Some(version) = payload.get("updated_at").and_then(|v| v.as_u64()) {
                last_version.fetch_max(version, Ordering::SeqCst);
            }
            sink(&name, payload);
        }
        RuntimeEvent::ProgressGap { .. } => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let previous = last_version
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| {
                    Some(now.max(last.saturating_add(1)))
                })
                .unwrap();
            let updated_at = now.max(previous.saturating_add(1));
            sink(
                "task-progress-refresh-error",
                serde_json::json!({
                    "scope":"all", "error":PROGRESS_GAP_NOTICE, "updated_at":updated_at,
                }),
            );
        }
        // Operation events are consumed only by commands.operation_observer.
        RuntimeEvent::Bash(_) | RuntimeEvent::PauseStage(_) => {}
    })
}

async fn ensure_subscription(
    app: AppHandle,
    control: &TaskControlState,
    start_if_absent: bool,
) -> Result<(), String> {
    let endpoint = control.endpoint()?;
    let mut subscription = control.subscription.lock().await;
    let subscribe = match subscription.as_ref() {
        Some(current) => !current.is_alive(),
        None => start_if_absent,
    };
    if subscribe {
        let sink = subscription_observer(Arc::new(move |name, payload| {
            let _ = app.emit(name, payload);
        }));
        // Explicit ready/refresh may replace a closed stream. No background
        // GUI retry loop, operation replay, host stop or new watcher owner.
        // If renewal fails, retain the dead token so another explicit refresh
        // can retry subscription admission without replaying business calls.
        let next = endpoint
            .subscribe_progress(sink)
            .await
            .map_err(|e| e.to_string())?;
        *subscription = Some(next);
    }
    Ok(())
}

#[tauri::command]
pub async fn frontend_ready(
    app: AppHandle,
    control: State<'_, TaskControlState>,
) -> Result<(), String> {
    ensure_subscription(app, &control, true).await?;
    control
        .endpoint()?
        .refresh_progress()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refresh_task_progress(
    app: AppHandle,
    control: State<'_, TaskControlState>,
) -> Result<(), String> {
    // Before frontend_ready, keep the original refresh-only/no-scan behavior.
    // A previously admitted but closed subscription may be renewed explicitly.
    ensure_subscription(app, &control, false).await?;
    control
        .endpoint()?
        .refresh_progress()
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn subscription_gap_warns_without_task_outcome_and_preserves_original_events() {
        let rows = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
        let recorded = rows.clone();
        let observer = subscription_observer(Arc::new(move |name, payload| {
            recorded.lock().unwrap().push((name.into(), payload))
        }));
        let version = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 10_000;
        let original = serde_json::json!({"collection":"exact","success":2,"updated_at":version});
        observer(RuntimeEvent::GuiProgress {
            name: "task-progress-update".into(),
            payload: original.clone(),
        });
        observer(RuntimeEvent::ProgressGap { dropped: 64 });
        observer(RuntimeEvent::ProgressGap { dropped: 1 });
        observer(RuntimeEvent::GuiProgress {
            name: "task-registry-changed".into(),
            payload: serde_json::json!({"remote":false}),
        });
        let rows = rows.lock().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], ("task-progress-update".into(), original));
        for row in &rows[1..3] {
            assert_eq!(row.0, "task-progress-refresh-error");
            assert_eq!(row.1["scope"], "all");
            assert_eq!(row.1["error"], PROGRESS_GAP_NOTICE);
            for authority in ["ok", "terminal", "status", "confirmed", "taskStatus"] {
                assert!(row.1.get(authority).is_none());
            }
        }
        assert!(rows[1].1["updated_at"].as_u64().unwrap() > version);
        assert!(
            rows[2].1["updated_at"].as_u64().unwrap() > rows[1].1["updated_at"].as_u64().unwrap()
        );
        assert_eq!(
            rows[3],
            (
                "task-registry-changed".into(),
                serde_json::json!({"remote":false})
            )
        );
    }
}
