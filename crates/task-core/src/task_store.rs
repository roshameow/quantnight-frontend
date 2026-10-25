//! Task-control storage over the EXISTING MongoClients factory. No new client,
//! resolver, fallback, retry owner or queue. Metadata reads never fetch payloads.
use crate::{config::AppConfig, mongo_manager::{MongoClients, ScopedMongoClients}};
use async_trait::async_trait;
use bson::{doc, Bson, Document};
use futures_util::TryStreamExt;
use mongodb::options::{AggregateOptions, FindOneOptions, FindOptions, InsertManyOptions, Hint};
use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};

const CONTROL_BUDGET: Duration = Duration::from_secs(8);
const QUERY_BUDGET: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Host {
    Local,
    Remote,
}
impl Host {
    pub fn from_remote(remote: bool) -> Self {
        if remote {
            Self::Remote
        } else {
            Self::Local
        }
    }
    pub fn is_remote(self) -> bool {
        self == Self::Remote
    }
}

/// Original collection_counts shape. Absence is an observed inventory fact;
/// failures/timeouts must remain Err, not an empty/default summary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CollectionSummary {
    pub exists: bool,
    pub total: u64,
    pub statuses: BTreeMap<String, u64>,
    pub latest_heartbeat_at: Option<Bson>,
}

/// EXACT taskctl RECEIPT_PROJECTION. Do not add payload/result/code fields here.
pub fn receipt_projection() -> Document {
    [
        "_id",
        "status",
        "acceptance",
        "terminalStatus",
        "alphaId",
        "progressUrl",
        "progress_url",
        "responseHeaders",
        "submissionAttempts",
        "campaign_post_attempted",
        "_not_posted_",
        "httpStatus",
        "rejection_evidence_received",
        "errorClass",
        "superSlotId",
        "childSimulationIds",
        "childSimulationId",
    ]
    .into_iter()
    .map(|key| (key.to_string(), Bson::Int32(1)))
    .collect()
}
pub fn id_projection() -> Document {
    doc! {"_id": 1}
}

/// Query builders are shared with lowest-layer offline shape tests. They do not
/// execute a query or replace TaskStore's production Mongo path.
pub fn summary_pipeline() -> Vec<Document> {
    vec![doc! {"$group": {"_id": {"$ifNull": ["$status", "missing"]}, "count": {"$sum": 1}}}]
}
pub fn heartbeat_query() -> (Document, FindOneOptions) {
    (
        doc! {"heartbeatAt": {"$exists": true}},
        FindOneOptions::builder()
            .projection(doc! {"heartbeatAt": 1, "_id": 0})
            .sort(doc! {"heartbeatAt": -1})
            .max_time(QUERY_BUDGET)
            .build(),
    )
}
pub fn nonempty_options() -> FindOptions {
    FindOptions::builder()
        .projection(id_projection())
        .limit(1)
        .batch_size(1)
        .max_time(QUERY_BUDGET)
        .build()
}
fn row_options(projection: Option<Document>) -> FindOptions {
    // No limit: metadata and actual copy must not truncate the candidate set.
    FindOptions::builder()
        .projection(projection)
        .max_time(QUERY_BUDGET)
        .build()
}

// Original never-started control ambiguity probe: limit applies to control
// records, never to candidates/items. Full control history is required.
pub fn never_started_controls_options() -> FindOptions {
    FindOptions::builder().limit(2).batch_size(2).max_time(CONTROL_BUDGET).build()
}
pub fn consumer_capability_query(name: &str) -> (Document, FindOneOptions) {
    (doc! {"task": name}, FindOneOptions::builder()
        .projection(id_projection()).max_time(CONTROL_BUDGET).build())
}

/// Canonical namespace returned by the original MongoConfig profile mapping.
/// Never derive this from the GUI's generic alpha database field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalNamespace { pub database: String, pub collection: String }
impl CanonicalNamespace {
    pub fn new(database: String, collection: String) -> Result<Self, String> {
        if database.trim().is_empty() || collection.trim().is_empty() {
            return Err("canonical result namespace missing; no fallback".into());
        }
        Ok(Self { database, collection })
    }
}
pub fn list_tasks_options() -> FindOptions {
    FindOptions::builder().projection(doc! {"name":1,"status":1,"taskType":1,
        "auth_profile":1,"isRemote":1,"targetRemote":1,"templatefile":1,"lastError":1})
        .sort(doc! {"_id":-1}).max_time(CONTROL_BUDGET).build()
}
pub fn result_items_options(limit: i64, offset: i64) -> Result<FindOptions, String> {
    if !(1..=200).contains(&limit) || !(0..=10000).contains(&offset) {
        return Err("limit must be 1..200 and offset 0..10000".into());
    }
    Ok(FindOptions::builder().projection(doc! {"_id":1,"alphaId":1,"status":1,
        "variantId":1,"variantHash":1,"candidateHash":1,"ideaId":1,"payloadHash":1,
        "acceptance":1,"terminalStatus":1}).sort(doc! {"_id":1})
        .skip(offset as u64).limit(limit+1).max_time(Duration::from_secs(5)).build())
}
/// Metadata from listIndexes, same first-key/hidden/partial guards as Python.
pub fn canonical_id_hint(indexes: &[Document]) -> Result<String, String> {
    let mut candidates: Vec<&str> = indexes.iter().filter_map(|index| {
        let key = index.get_document("key").ok()?;
        let (field, direction) = key.iter().next()?;
        if field != "id" || !matches!(direction, Bson::Int32(1) | Bson::Int64(1) | Bson::Double(1.0))
            || index.get_bool("hidden").unwrap_or(false)
            || index.get_document("partialFilterExpression").map(|d| !d.is_empty()).unwrap_or(false) {
            return None;
        }
        index.get_str("name").ok()
    }).collect();
    candidates.sort();
    candidates.first().map(|v| v.to_string()).ok_or_else(||
        "canonical result id index missing; refusing a full collection scan".into())
}
/// Explicit full audit export only. Original collect has no total/query cap;
/// transport behavior remains owned by the injected MongoClients factory.
pub fn collect_result_options() -> FindOptions {
    FindOptions::builder().projection(doc! {"_id":0}).build()
}
pub fn canonical_result_options(hint: String, count: usize) -> Result<FindOptions, String> {
    if !(1..=200).contains(&count) { return Err("canonical result read must be a bounded nonempty page".into()); }
    Ok(FindOptions::builder().projection(doc! {"_id":0,"id":1,"type":1,"status":1,
        "dateCreated":1,"dateModified":1,"updated_at":1,"settings.region":1,
        "settings.universe":1,"settings.delay":1,"is.sharpe":1,"is.fitness":1,
        "is.margin":1,"is.turnover":1,"is.returns":1,"is.drawdown":1})
        .hint(Hint::Name(hint)).limit(count as i64).max_time(Duration::from_secs(5)).build())
}

#[async_trait]
pub trait TaskStore: Send + Sync {
    async fn collect_result_items(&self, _host: Host, _name: &str) -> Result<Vec<Document>, String> {
        Err("explicit full result export unavailable".into())
    }
    async fn list_tasks(&self, _host: Host, _include_deactive: bool) -> Result<Vec<Document>, String> {
        Err("projected task listing unavailable".into())
    }
    async fn result_items_page(&self, _host: Host, _name: &str, _limit: i64, _offset: i64) -> Result<Vec<Document>, String> {
        Err("projected task result page unavailable".into())
    }
    async fn canonical_results(&self, _namespace: &CanonicalNamespace, _ids: &[Bson]) -> Result<Vec<Document>, String> {
        Err("indexed canonical result page unavailable".into())
    }
    async fn find_items(&self, _host: Host, _name: &str, _filter: Document, _projection: Option<Document>) -> Result<Vec<Document>, String> {
        Err("filtered task item query unavailable".into())
    }
    async fn update_item(&self, _host: Host, _name: &str, _filter: Document, _update: Document) -> Result<u64, String> {
        Err("single task item write unavailable".into())
    }
    async fn insert_task(&self, _host: Host, _task: Document) -> Result<Bson, String> {
        Err("task registration write unavailable".into())
    }
    async fn update_items(&self, _host: Host, _name: &str, _filter: Document, _update: Document) -> Result<u64, String> {
        Err("task item metadata write unavailable".into())
    }
    // Fail-closed defaults keep unrelated existing adapters source compatible;
    // positive never-started proof requires the real implementations below.
    async fn find_tasks(&self, _host: Host, _filter: Document) -> Result<Vec<Document>, String> {
        Err("never-started control ambiguity query unavailable".into())
    }
    async fn consumer_capability_exists(&self, _host: Host, _name: &str) -> Result<bool, String> {
        Err("never-started consumer capability query unavailable".into())
    }
    async fn find_task(
        &self,
        host: Host,
        filter: Document,
        newest: bool,
    ) -> Result<Option<Document>, String>;
    async fn update_task(
        &self,
        host: Host,
        filter: Document,
        update: Document,
    ) -> Result<u64, String>;
    async fn collection_names(&self, host: Host) -> Result<Vec<String>, String>;
    async fn summary(&self, host: Host, name: &str) -> Result<CollectionSummary, String>;
    async fn has_rows(&self, host: Host, name: &str) -> Result<bool, String>;
    async fn rows(
        &self,
        host: Host,
        name: &str,
        projection: Option<Document>,
    ) -> Result<Vec<Document>, String>;
    async fn insert_rows(&self, host: Host, name: &str, rows: Vec<Document>) -> Result<(), String>;
    async fn rename_collection(&self, host: Host, old: &str, new: &str) -> Result<(), String>;
    async fn ping(&self, host: Host) -> Result<(), String>;
}

/// One application attempt, bounded across connection/command/cursor drain.
/// Dropping a timed-out mutation does not establish whether Mongo accepted it.
/// Never resubmit here; the caller retains original identity/partial evidence.
async fn bounded<T>(
    operation: &str,
    budget: Duration,
    future: impl Future<Output = Result<T, mongodb::error::Error>>,
) -> Result<T, String> {
    tokio::time::timeout(budget, future)
        .await
        .map_err(|_| {
            format!(
                "{operation}: timeout after {} ms; result unconfirmed, not retried",
                budget.as_millis()
            )
        })?
        .map_err(|e| format!("{operation}: {e}"))
}

/// Immutable mappings supplied by the existing app config, never a new factory.
pub struct MongoTaskStore {
    clients: ScopedMongoClients,
    mission: String,
    simulation: String,
}
impl MongoTaskStore {
    /// Original desktop injection: retain the exact existing pools.
    pub fn new(clients: Arc<MongoClients>, config: &AppConfig) -> Result<Self, String> {
        Self::with_clients(ScopedMongoClients::existing(&clients), config)
    }
    /// Native bootstrap does not create unused clients or parse unused URIs.
    /// First access delegates to the original MongoClients connection owner.
    pub fn from_config(config: &AppConfig) -> Result<Self, String> {
        Self::with_clients(MongoClients::scoped(config), config)
    }
    fn with_clients(clients: ScopedMongoClients, config: &AppConfig) -> Result<Self, String> {
        let mission = config.mongodb.databases.mission.clone();
        let simulation = config.mongodb.databases.simulation.clone();
        if mission.trim().is_empty() || simulation.trim().is_empty() {
            return Err("task database bindings are required; no default database fallback".into());
        }
        Ok(Self {
            clients,
            mission,
            simulation,
        })
    }
    async fn client(&self, host: Host) -> Result<&mongodb::Client, String> {
        self.clients.client(host.is_remote()).await.map(Arc::as_ref)
    }
}

/// Stringification only for diagnostic group labels, NOT receipt policy.
fn group_label(value: &Bson) -> String {
    match value {
        Bson::String(v) => v.clone(),
        Bson::Boolean(true) => "True".into(),
        Bson::Boolean(false) => "False".into(),
        Bson::Null => "None".into(),
        other => other.to_string(),
    }
}
fn decode_summary(
    groups: Vec<Document>,
    latest: Option<Document>,
) -> Result<CollectionSummary, String> {
    let mut summary = CollectionSummary {
        exists: true,
        ..Default::default()
    };
    for row in groups {
        let count = match row.get("count") {
            Some(Bson::Int32(n)) if *n >= 0 => *n as u64,
            Some(Bson::Int64(n)) if *n >= 0 => *n as u64,
            _ => return Err("invalid status aggregation count".into()),
        };
        let key = row
            .get("_id")
            .ok_or("status aggregation missing group identity")?;
        summary.total = summary
            .total
            .checked_add(count)
            .ok_or("status aggregation count overflow")?;
        summary.statuses.insert(group_label(key), count);
    }
    summary.latest_heartbeat_at = latest.and_then(|row| row.get("heartbeatAt").cloned());
    Ok(summary)
}

#[async_trait]
impl TaskStore for MongoTaskStore {
    async fn collect_result_items(&self, host: Host, name: &str) -> Result<Vec<Document>, String> {
        let items = self.client(host).await?.database(&self.simulation).collection::<Document>(name);
        items.find(doc! {}, collect_result_options()).await.map_err(|e| format!("explicit result export: {e}"))?
            .try_collect().await.map_err(|e| format!("explicit result export: {e}"))
    }
    async fn list_tasks(&self, host: Host, include_deactive: bool) -> Result<Vec<Document>, String> {
        let tasks = self.client(host).await?.database(&self.mission).collection::<Document>("tasks");
        let filter = if include_deactive { doc! {} } else { doc! {"status":{"$ne":"deactive"}} };
        bounded("projected task listing", CONTROL_BUDGET, async {
            tasks.find(filter, list_tasks_options()).await?.try_collect().await
        }).await
    }
    async fn result_items_page(&self, host: Host, name: &str, limit: i64, offset: i64) -> Result<Vec<Document>, String> {
        let options = result_items_options(limit, offset)?;
        let items = self.client(host).await?.database(&self.simulation).collection::<Document>(name);
        bounded("projected task result page", CONTROL_BUDGET, async {
            items.find(doc! {}, options).await?.try_collect().await
        }).await
    }
    async fn canonical_results(&self, namespace: &CanonicalNamespace, ids: &[Bson]) -> Result<Vec<Document>, String> {
        if ids.is_empty() { return Ok(vec![]); }
        if ids.len() > 200 { return Err("canonical result read exceeds bounded page".into()); }
        CanonicalNamespace::new(namespace.database.clone(), namespace.collection.clone())?;
        let collection = self.client(Host::Local).await?.database(&namespace.database).collection::<Document>(&namespace.collection);
        // One budget covers metadata AND cursor drain; no retry, no implicit
        // index creation, no unhinted fallback when metadata/query fails.
        tokio::time::timeout(CONTROL_BUDGET, async {
            let indexes: Vec<mongodb::IndexModel> = collection.list_indexes(None).await.map_err(|e| e.to_string())?
                .try_collect().await.map_err(|e| e.to_string())?;
            let indexes = indexes.iter().map(bson::to_document).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            let hint = canonical_id_hint(&indexes)?;
            let options = canonical_result_options(hint, ids.len())?;
            collection.find(doc! {"id":{"$in":ids.to_vec()}}, options).await.map_err(|e| e.to_string())?
                .try_collect().await.map_err(|e| e.to_string())
        }).await.map_err(|_| "canonical result page: timeout after 8000 ms; result unconfirmed, not retried".to_string())?
    }
    async fn update_item(&self, host: Host, name: &str, filter: Document, update: Document) -> Result<u64, String> {
        let items = self.client(host).await?.database(&self.simulation).collection::<Document>(name);
        bounded("single task item write", CONTROL_BUDGET, items.update_one(filter, update, None))
            .await.map(|result| result.modified_count)
    }
    async fn insert_task(&self, host: Host, task: Document) -> Result<Bson, String> {
        let tasks = self.client(host).await?.database(&self.mission).collection::<Document>("tasks");
        bounded("task registration write", CONTROL_BUDGET, tasks.insert_one(task, None))
            .await.map(|result| result.inserted_id)
    }
    async fn update_items(&self, host: Host, name: &str, filter: Document, update: Document) -> Result<u64, String> {
        let items = self.client(host).await?.database(&self.simulation).collection::<Document>(name);
        bounded("task item metadata write", CONTROL_BUDGET, items.update_many(filter, update, None))
            .await.map(|result| result.matched_count)
    }
    async fn find_tasks(&self, host: Host, filter: Document) -> Result<Vec<Document>, String> {
        let tasks = self.client(host).await?.database(&self.mission).collection::<Document>("tasks");
        bounded("never-started control ambiguity query", CONTROL_BUDGET, async {
            tasks.find(filter, never_started_controls_options()).await?.try_collect().await
        }).await
    }
    async fn consumer_capability_exists(&self, host: Host, name: &str) -> Result<bool, String> {
        let capabilities = self.client(host).await?.database(&self.mission)
            .collection::<Document>("priority_consumer_capabilities");
        let (filter, options) = consumer_capability_query(name);
        bounded("never-started consumer capability query", CONTROL_BUDGET,
            capabilities.find_one(filter, options)).await.map(|row| row.is_some())
    }
    async fn find_task(
        &self,
        host: Host,
        filter: Document,
        newest: bool,
    ) -> Result<Option<Document>, String> {
        let mut options = FindOneOptions::default();
        options.max_time = Some(CONTROL_BUDGET);
        if newest {
            options.sort = Some(doc! { "_id": -1 });
        }
        let tasks = self
            .client(host).await?
            .database(&self.mission)
            .collection::<Document>("tasks");
        bounded("task read", CONTROL_BUDGET, tasks.find_one(filter, options)).await
    }
    async fn update_task(
        &self,
        host: Host,
        filter: Document,
        update: Document,
    ) -> Result<u64, String> {
        let tasks = self
            .client(host).await?
            .database(&self.mission)
            .collection::<Document>("tasks");
        bounded(
            "task control write",
            CONTROL_BUDGET,
            tasks.update_one(filter, update, None),
        )
        .await
        .map(|result| result.matched_count)
    }
    async fn collection_names(&self, host: Host) -> Result<Vec<String>, String> {
        let database = self.client(host).await?.database(&self.simulation);
        bounded(
            "task collection inventory",
            CONTROL_BUDGET,
            database.list_collection_names(None),
        )
        .await
    }
    async fn summary(&self, host: Host, name: &str) -> Result<CollectionSummary, String> {
        if !self.collection_names(host).await?.iter().any(|n| n == name) {
            return Ok(CollectionSummary::default());
        }
        let collection = self
            .client(host).await?
            .database(&self.simulation)
            .collection::<Document>(name);
        let groups: Vec<Document> = bounded("task status aggregation", QUERY_BUDGET, async {
            collection
                .aggregate(
                    summary_pipeline(),
                    AggregateOptions::builder().max_time(QUERY_BUDGET).build(),
                )
                .await?
                .try_collect()
                .await
        })
        .await?;
        let (filter, options) = heartbeat_query();
        let latest = bounded(
            "task latest heartbeat",
            QUERY_BUDGET,
            collection.find_one(filter, options),
        )
        .await?;
        decode_summary(groups, latest)
    }
    async fn has_rows(&self, host: Host, name: &str) -> Result<bool, String> {
        let collection = self
            .client(host).await?
            .database(&self.simulation)
            .collection::<Document>(name);
        bounded("task nonempty probe", QUERY_BUDGET, async {
            collection
                .find(doc! {}, nonempty_options())
                .await?
                .try_next()
                .await
        })
        .await
        .map(|row| row.is_some())
    }
    async fn rows(
        &self,
        host: Host,
        name: &str,
        projection: Option<Document>,
    ) -> Result<Vec<Document>, String> {
        self.find_items(host, name, doc! {}, projection).await
    }
    async fn find_items(&self, host: Host, name: &str, filter: Document, projection: Option<Document>) -> Result<Vec<Document>, String> {
        let collection = self.client(host).await?.database(&self.simulation).collection::<Document>(name);
        bounded("task item query/read", QUERY_BUDGET, async {
            collection.find(filter, row_options(projection)).await?.try_collect().await
        }).await
    }
    async fn insert_rows(&self, host: Host, name: &str, rows: Vec<Document>) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        let collection = self
            .client(host).await?
            .database(&self.simulation)
            .collection::<Document>(name);
        bounded(
            "initial task copy",
            CONTROL_BUDGET,
            collection.insert_many(rows, InsertManyOptions::builder().ordered(true).build()),
        )
        .await
        .map(|_| ())
    }
    async fn rename_collection(&self, host: Host, old: &str, new: &str) -> Result<(), String> {
        let database = self.client(host).await?.database("admin");
        bounded(
            "task archive rename",
            CONTROL_BUDGET,
            database.run_command(
                doc! {
                    "renameCollection": format!("{}.{}", self.simulation, old),
                    "to": format!("{}.{}", self.simulation, new), "dropTarget": false,
                    "maxTimeMS": CONTROL_BUDGET.as_millis() as i64
                },
                None,
            ),
        )
        .await
        .map(|_| ())
    }
    async fn ping(&self, host: Host) -> Result<(), String> {
        let database = self.client(host).await?.database("admin");
        bounded(
            "task storage availability",
            CONTROL_BUDGET,
            database.run_command(doc! {"ping": 1}, None),
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_queries_are_projected_and_only_nonempty_probe_has_a_limit() {
        assert_eq!(
            summary_pipeline(),
            vec![
                doc! {"$group": {"_id": {"$ifNull": ["$status", "missing"]}, "count": {"$sum": 1}}}
            ]
        );
        let (filter, latest) = heartbeat_query();
        assert_eq!(filter, doc! {"heartbeatAt": {"$exists": true}});
        assert_eq!(latest.projection, Some(doc! {"heartbeatAt": 1, "_id": 0}));
        assert_eq!(latest.sort, Some(doc! {"heartbeatAt": -1}));
        assert_eq!(latest.max_time, Some(Duration::from_secs(10)));
        let probe = nonempty_options();
        assert_eq!(probe.limit, Some(1));
        assert_eq!(probe.batch_size, Some(1));
        assert_eq!(probe.projection, Some(doc! {"_id": 1}));
        for projection in [None, Some(id_projection()), Some(receipt_projection())] {
            let options = row_options(projection.clone());
            assert_eq!(options.limit, None);
            assert_eq!(options.projection, projection);
            assert_eq!(options.max_time, Some(Duration::from_secs(10)));
        }
        assert_eq!(CONTROL_BUDGET, Duration::from_secs(8));
    }
    #[test]
    fn aggregate_decode_preserves_zero_vs_error_and_bson_heartbeat() {
        let stamp = bson::DateTime::from_millis(1234);
        let summary = decode_summary(
            vec![
                doc! {"_id": "pending", "count": 5i32},
                doc! {"_id": "success", "count": 7i64},
            ],
            Some(doc! {"heartbeatAt": stamp}),
        )
        .unwrap();
        assert!(summary.exists);
        assert_eq!(summary.total, 12);
        assert_eq!(summary.latest_heartbeat_at, Some(Bson::DateTime(stamp)));
        assert!(decode_summary(vec![], None).unwrap().exists);
        for bad in [
            doc! {"_id": "pending", "count": -1},
            doc! {"_id": "pending", "count": "7"},
            doc! {"count": 1},
        ] {
            assert!(decode_summary(vec![bad], None).is_err());
        }
    }
    #[tokio::test]
    async fn query_error_timeout_and_mutation_timeout_are_not_retried_or_empty_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        for operation in [
            "task item query/read",
            "task nonempty probe",
            "task status aggregation",
            "task latest heartbeat",
            "task control write",
            "initial task copy",
            "task archive rename",
        ] {
            let before = calls.load(Ordering::SeqCst);
            let result: Result<(), String> = bounded(operation, Duration::from_millis(1), async {
                calls.fetch_add(1, Ordering::SeqCst);
                // A write may have reached the server here. Timeout supplies no
                // negative acceptance proof and cannot authorize a second call.
                std::future::pending::<Result<(), mongodb::error::Error>>().await
            })
            .await;
            let error = result.unwrap_err();
            assert!(
                error.contains(operation)
                    && error.contains("timeout")
                    && error.contains("not retried")
            );
            assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        }
        let failure: Result<bool, String> = bounded("task nonempty probe", QUERY_BUDGET, async {
            Err(mongodb::error::Error::from(std::io::Error::new(
                std::io::ErrorKind::Other,
                "sentinel cursor failure",
            )))
        })
        .await;
        assert!(failure.unwrap_err().contains("sentinel cursor failure"));
    }
}
