//! Common application entry point. Desktop and CLI share the original task
//! endpoint authority; desktop UI settings remain separate. Neither adapter
//! owns task mutation or a second client factory.
use std::sync::Arc;
use bson::{doc, oid::ObjectId, Document};
use serde_json::json;
use crate::{backend_binding::{validate_backend_binding, resolve_gui_binding, BackendBinding},
    cleanup_sync::{self, TaskOutcome}, config::{AppConfig, BashScriptObserver},
    mongo_manager::MongoClients, operations_runtime::OriginalOperationsRuntime,
    task_store::{Host, MongoTaskStore, TaskStore}};

/// Unpublished checked desktop context. No public constructor; consuming it
/// rechecks source freshness immediately before the GUI registers shared state.
/// The default type is the original Mongo client, not an alternate pool stack.
pub struct GuiContext<T = mongodb::Client> {
    config: AppConfig,
    clients: Arc<MongoClients<T>>,
    binding: BackendBinding,
    store: MongoTaskStore<T>,
}
impl<T> GuiContext<T> {
    fn checked_parts(self) -> Result<(AppConfig, Arc<MongoClients<T>>, BackendBinding, MongoTaskStore<T>), String> {
        self.binding.verify_current()?;
        Ok((self.config, self.clients, self.binding, self.store))
    }
}
impl GuiContext {
    pub fn into_parts(self) -> Result<(AppConfig, Arc<MongoClients>, TaskControl), String> {
        let (config, clients, binding, store) = self.checked_parts()?;
        Ok((config, clients, TaskControl { binding, store: Arc::new(store) }))
    }
}

// One startup algorithm. The only injection point is the lowest client
// factory; export, strict proof, merge, store wiring and freshness are real.
async fn bootstrap_gui_with<T, F, Fut>(config: AppConfig, build: F) -> Result<GuiContext<T>, String>
where F: FnMut(String, String) -> Fut,
      Fut: std::future::Future<Output = Result<Arc<T>, String>> {
    let (config, binding) = resolve_gui_binding(config).await?;
    let clients = Arc::new(MongoClients::new_with(&config, build).await?);
    let store = MongoTaskStore::new(clients.clone(), &config)?;
    binding.verify_current()?;
    Ok(GuiContext { config, clients, binding, store })
}

pub struct TaskControl {
    binding: BackendBinding,
    store: Arc<dyn TaskStore>,
}
impl TaskControl {
    /// Original GUI startup authority, before all shared reads/watchers.
    /// Endpoint options intentionally follow MongoConfig, not legacy TOML.
    pub async fn bootstrap_gui(config: AppConfig) -> Result<GuiContext, String> {
        bootstrap_gui_with(config, |role, uri| async move {
            MongoClients::build_client(&role, &uri).await.map_err(|error| error.to_string())
        }).await
    }
    /// Headless CLI bootstrap. Configuration proof happens before client setup.
    pub async fn connect(config: &AppConfig) -> Result<Self, String> {
        let binding = validate_backend_binding(config).await?;
        let store = Arc::new(MongoTaskStore::from_config(config)?);
        Ok(Self { binding, store })
    }
    /// Desktop reuses its existing application pools; never connects per button.
    pub async fn with_existing_clients(config: &AppConfig, clients: Arc<MongoClients>) -> Result<Self, String> {
        let binding = validate_backend_binding(config).await?;
        Ok(Self { binding, store: Arc::new(MongoTaskStore::new(clients, config)?) })
    }
    /// Injection of already-verified context and lowest storage port. This is
    /// not an alternate configuration or no-validation production shortcut.
    pub fn with_store(binding: BackendBinding, store: Arc<dyn TaskStore>) -> Self {
        Self { binding, store }
    }
    pub fn root(&self) -> &std::path::Path { self.binding.root() }

    /// GUI identifiers resolve on the local control plane, just like taskctl.
    /// Remote-only legacy records are NOT adopted by name or physically dropped.
    pub async fn task_by_id(&self, id: &str) -> Result<Document, String> {
        self.binding.refresh_if_changed().await?;
        let id = ObjectId::parse_str(id).map_err(|_| "invalid original task ObjectId")?;
        self.store.find_task(Host::Local, doc! {"_id": id}, false).await?
            .ok_or_else(|| "original task is not registered in this local control plane; retain remote-only records and manage them through their original owner".into())
    }
    pub async fn pause(&self, name: &str, options: crate::pause::PauseOptions<'_>,
        budget: crate::pause::StopBudget, observer: Option<BashScriptObserver>) -> crate::pause::PauseOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await {
            let mut failed = rejected("pause", name, error);
            failed.value["reason"] = json!("pause_unconfirmed");
            failed.value["stopConfirmation"] = json!("unknown");
            return crate::pause::PauseOutcome { value: failed.value, exit_code: 1, confirmed_stop: None };
        }
        let runtime = crate::operations_runtime::OriginalPauseRuntime {
            common: OriginalOperationsRuntime { binding: &self.binding, observer }, budget };
        crate::pause::pause(self.root(), self.store.as_ref(), &runtime, name, options).await
    }
    pub async fn create(&self, request: &crate::create_generate::CreateRequest,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("create", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        crate::create_generate::create(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn generate(&self, request: &crate::create_generate::GenerateRequest,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("generate", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        crate::create_generate::generate(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn list(&self, request: &crate::inspection::ListRequest) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await {
            let mut value = rejected("list", "", error); value.value["task"] = serde_json::Value::Null; return value;
        }
        crate::inspection::list(self.store.as_ref(), request).await
    }
    pub async fn status(&self, request: &crate::inspection::StatusRequest) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("status", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer: None };
        crate::inspection::status(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn results(&self, request: &crate::inspection::ResultsRequest) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("results", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer: None };
        crate::inspection::results(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn collect_results(&self, request: &crate::inspection::CollectRequest) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("collect-results", &request.name, error); }
        crate::inspection::collect_results(self.root(), self.store.as_ref(), request).await
    }
    pub async fn dataset_evidence(&self, request: &crate::inspection::EvidenceRequest) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected(request.action.command(), &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer: None };
        crate::inspection::dataset_evidence(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn watch(&self, request: &crate::maintenance_ops::WatchRequest) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("watch", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer: None };
        crate::maintenance_ops::watch(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn observe_accepted(&self, request: &crate::maintenance_ops::ObserveAcceptedRequest,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("observe-accepted", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        crate::maintenance_ops::observe_accepted(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn priority_prune(&self, request: &crate::maintenance_ops::PriorityPruneRequest,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("priority-prune", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        crate::maintenance_ops::priority_prune(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn update_priority(&self, request: &crate::priority_update::UpdatePriorityRequest,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("priority-update", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        crate::priority_update::update_priority(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn start(&self, request: &crate::start::StartRequest,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("start", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        crate::start::start(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn resume(&self, request: &crate::resume::ResumeRequest,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("resume", &request.name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        crate::resume::resume(self.root(), self.store.as_ref(), &runtime, request).await
    }
    pub async fn sync(&self, name: &str, expected_id: Option<&str>, observer: Option<BashScriptObserver>) -> TaskOutcome {
        self.sync_bound(name, expected_id, None, observer).await
    }
    pub async fn sync_bound(&self, name: &str, expected_id: Option<&str>, expected_remote: Option<bool>, observer: Option<BashScriptObserver>) -> TaskOutcome {
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("sync", name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        cleanup_sync::sync_with_expected_host(self.root(), self.store.as_ref(), &runtime, name, expected_id, expected_remote).await
    }
    pub async fn delete(&self, name: &str, expected_id: Option<&str>, confirm: Option<&str>,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        self.delete_bound(name, expected_id, None, confirm, observer).await
    }
    pub async fn delete_bound(&self, name: &str, expected_id: Option<&str>, expected_remote: Option<bool>, confirm: Option<&str>,
        observer: Option<BashScriptObserver>) -> TaskOutcome {
        // Old --force never overrides safety; callers do not pass it as a gate.
        if confirm.is_some_and(|confirm| confirm != name) {
            return rejected("delete", name, "--confirm, when supplied, must equal the exact task name".into());
        }
        if let Err(error) = self.binding.refresh_if_changed().await { return rejected("delete", name, error); }
        let runtime = OriginalOperationsRuntime { binding: &self.binding, observer };
        cleanup_sync::delete_with_expected_host(self.root(), self.store.as_ref(), &runtime, name, expected_id, expected_remote).await
    }
}
fn rejected(command: &str, name: &str, error: String) -> TaskOutcome {
    TaskOutcome { exit_code: 1, value: json!({"schemaVersion":1, "command":command,
        "task":name, "ok":false, "at":chrono::Utc::now().to_rfc3339(),
        "errorType":"RuntimeError", "error":error}) }
}

#[cfg(all(test, unix, feature = "test-support"))]
#[path = "gui_bootstrap_tests.rs"]
mod gui_bootstrap_tests;
