//! Desktop Tauri command adapters for Data Panel and Alpha List.
//! Core DTOs, filters, RA aggregation, correlation enrichment, and queries live in
//! `quantnight_task_core::data_panel`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant};

#[path = "alpha_search_plan.rs"]
mod alpha_search_plan;

use chrono::{DateTime, Duration, Utc};
use futures::join;
use futures_util::stream::TryStreamExt;
use mongodb::{
    bson::{doc, Bson, Document},
    options::{CountOptions, FindOptions, Hint, ListIndexesOptions},
};
use serde_json::Value as JsonValue;
use tauri::{command, State};

use crate::config::{run_python_command, AppConfig};
use crate::mongo_manager::MongoClients;

pub use quantnight_task_core::alpha_query::{category_aliases, parse_query_to_bson, SortField};
pub use quantnight_task_core::data_panel::*;

#[command]
pub async fn get_datafields(
    params: DatafieldQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PagedResult<Datafield>, String> {
    query_datafields(&clients.local, &config, &params).await
}

#[command]
pub async fn get_datasets(
    params: DatasetQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PagedResult<Dataset>, String> {
    query_datasets(&clients.local, &config, &params).await
}

#[cfg(test)]
mod prod_correlation_tests {
    use super::*;
    use mongodb::bson::DateTime as BsonDateTime;

    fn result(id: &str, current: Option<JsonValue>) -> AlphaResult {
        let mut result = parse_alpha_document(
            doc! { "id": id, "type": "REGULAR", "settings": { "region": "USA" } },
            None,
        )
        .unwrap();
        result.current_prod_correlation = current;
        result
    }

    fn snapshot(value: f64, updated_at: &str) -> JsonValue {
        serde_json::json!({ "value": value, "updatedAt": updated_at })
    }

    #[test]
    fn batch_filter_includes_page_and_ra_children_but_no_other_ids() {
        let mut parent = result("parent", None);
        parent.children = Some(vec![result("child", None)]);
        let results = vec![parent, result("regular", None)];
        let ids = prod_correlation_ids(&results);
        assert_eq!(ids.len(), 3);
        let filter = prod_correlation_filter(ids);
        assert_eq!(filter.get_str("mode").unwrap(), "prod");
        let ids = filter.get_document("id").unwrap().get_array("$in").unwrap();
        for id in ["parent", "child", "regular"] {
            assert!(ids.iter().any(|item| item.as_str() == Some(id)));
        }
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn joins_only_existing_rows_and_prod_mode() {
        let mut results = vec![result("present", None)];
        merge_prod_correlations(
            &mut results,
            vec![
                doc! { "id": "present", "mode": "self", "value": 0.9 },
                doc! { "id": "absent", "mode": "prod", "value": 0.8 },
                doc! { "id": "present", "mode": "prod", "value": 0.0,
                "updatedAt": BsonDateTime::from_millis(1_700_000_000_000_i64) },
            ],
        );
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].current_prod_correlation.as_ref().unwrap()["value"],
            0.0
        );
        assert!(results[0].current_prod_correlation.as_ref().unwrap()["updatedAt"].is_string());
    }

    #[test]
    fn newest_valid_snapshot_wins_and_equal_or_unknown_time_keeps_existing() {
        let old = "2024-01-01T00:00:00Z";
        let new = "2024-01-02T00:00:00Z";
        let mut results = vec![
            result("newer_fallback", Some(snapshot(0.1, old))),
            result("older_fallback", Some(snapshot(0.2, new))),
            result("equal", Some(snapshot(0.3, new))),
            result("unknown", Some(serde_json::json!({"value": 0.4}))),
            result("missing", None),
        ];
        merge_prod_correlations(
            &mut results,
            vec![
                doc! { "id": "newer_fallback", "mode": "prod", "value": 0.5, "updatedAt": new },
                doc! { "id": "older_fallback", "mode": "prod", "value": 0.6, "updatedAt": old },
                doc! { "id": "equal", "mode": "prod", "value": 0.7, "updatedAt": new },
                doc! { "id": "unknown", "mode": "prod", "value": 0.8, "updatedAt": new },
                doc! { "id": "missing", "mode": "prod", "value": 0.9, "updatedAt": old },
            ],
        );
        let values: Vec<_> = results
            .iter()
            .map(|r| {
                r.current_prod_correlation.as_ref().unwrap()["value"]
                    .as_f64()
                    .unwrap()
            })
            .collect();
        assert_eq!(values, vec![0.5, 0.2, 0.3, 0.4, 0.9]);
        assert_eq!(
            correlation_timestamp(
                &serde_json::json!({"updatedAt": {"$date": {"$numberLong": "1704067200000"}}})
            ),
            Some(1_704_067_200_000)
        );
    }

    #[test]
    fn existing_bson_timestamp_is_comparable_to_fallback() {
        let mut results = vec![parse_alpha_document(
            doc! {
                "id": "bson", "type": "REGULAR", "settings": { "region": "USA" },
                "currentProdCorrelation": { "value": 0.2,
                    "updatedAt": BsonDateTime::from_millis(1_704_067_200_000) }
            },
            None,
        )
        .unwrap()];
        merge_prod_correlations(
            &mut results,
            vec![doc! { "id": "bson", "mode": "prod", "value": 0.4,
            "updatedAt": BsonDateTime::from_millis(1_704_153_600_000) }],
        );
        assert_eq!(
            results[0].current_prod_correlation.as_ref().unwrap()["value"],
            0.4
        );
    }

    #[test]
    fn duplicate_fallback_docs_choose_newest_independent_of_cursor_order() {
        let earlier = doc! { "id": "dup", "mode": "prod", "value": 0.1,
        "updatedAt": BsonDateTime::from_millis(1_704_067_200_000) };
        let later = doc! { "id": "dup", "mode": "prod", "value": 0.9,
        "updatedAt": BsonDateTime::from_millis(1_704_153_600_000) };
        for docs in [
            vec![later.clone(), earlier.clone()],
            vec![earlier.clone(), later.clone()],
        ] {
            let mut results = vec![result("dup", None)];
            merge_prod_correlations(&mut results, docs);
            assert_eq!(
                results[0].current_prod_correlation.as_ref().unwrap()["value"],
                0.9
            );
        }
    }

    #[test]
    fn children_are_enriched_and_null_fallback_does_not_erase_existing() {
        let mut parent = result("parent", None);
        parent.children = Some(vec![result("child", None)]);
        let mut results = vec![
            parent,
            result("already", Some(snapshot(0.2, "2024-01-01T00:00:00Z"))),
        ];
        merge_prod_correlations(
            &mut results,
            vec![
                doc! { "id": "child", "mode": "prod", "value": 0.7 },
                doc! { "id": "already", "mode": "prod", "value": Bson::Null },
            ],
        );
        assert_eq!(
            results[0].children.as_ref().unwrap()[0]
                .current_prod_correlation
                .as_ref()
                .unwrap()["value"],
            0.7
        );
        assert_eq!(
            results[1].current_prod_correlation.as_ref().unwrap()["value"],
            0.2
        );
    }
}

#[cfg(test)]
#[path = "ra_message_tests.rs"]
mod ra_message_tests;

#[cfg(test)]
#[path = "ra_parent_page_tests.rs"]
mod ra_parent_page_tests;

#[cfg(test)]
#[path = "alpha_page_tests.rs"]
mod alpha_page_tests;

// Read-only metadata cache for the one search shape confirmed by stack/profile
// evidence. Does not impose a new deadline on the user's actual list query.
type SearchIndexSnapshot = (Instant, Vec<mongodb::IndexModel>);
static SEARCH_INDEX_SNAPSHOTS: OnceLock<Mutex<HashMap<String, SearchIndexSnapshot>>> =
    OnceLock::new();

async fn measured_search_hint(
    collection: &mongodb::Collection<Document>,
    filter: &Document,
    sort: &Document,
) -> Option<Hint> {
    if alpha_search_plan::simple_keyword_scope(filter) != Some(true)
        || sort.len() != 1
        || !sort.get("is.sharpe").is_some_and(alpha_search_plan::index_direction)
    {
        return None;
    }
    let key = collection.namespace().to_string();
    let cache = SEARCH_INDEX_SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let entries = cache.lock().ok()?;
        if let Some((created, indexes)) = entries.get(&key) {
            if created.elapsed() < StdDuration::from_secs(60) {
                return alpha_search_plan::select_keyword_hint(filter, sort, indexes);
            }
        }
    }
    let read = async {
        let options = ListIndexesOptions::builder()
            .max_time(StdDuration::from_millis(700))
            .build();
        collection.list_indexes(options).await?.try_collect::<Vec<_>>().await
    };
    let indexes = match tokio::time::timeout(StdDuration::from_secs(1), read).await {
        Ok(Ok(indexes)) => indexes,
        _ => return None, // Unknown metadata preserves the original optimizer.
    };
    let hint = alpha_search_plan::select_keyword_hint(filter, sort, &indexes);
    if let Ok(mut entries) = cache.lock() {
        if entries.len() >= 32 {
            entries.clear();
        }
        entries.insert(key, (Instant::now(), indexes));
    }
    hint
}

/// Exact non-RA count, deliberately independent from page retrieval.
#[command]
pub async fn get_alpha_count(
    params: AlphaQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<u64, String> {
    if is_ra_query(&params) {
        return Err("get_alpha_count does not support RA/RA_PARENT; use get_alpha_results for eligibility totals".to_string());
    }
    let db = clients.local.database(&config.mongodb.databases.alpha);
    let collection = db.collection::<Document>(&alpha_collection_name(&params));
    let filter = build_alpha_filter(&params);
    let options = alpha_count_options(
        &filter,
        CountOptions::builder()
            .max_time(std::time::Duration::from_secs(30))
            .build()
    );
    collection
        .count_documents(filter, options)
        .await
        .map_err(|e| format!("DB count error: {e}"))
}

#[command]
pub async fn get_alpha_results(
    params: AlphaQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PagedResult<AlphaResult>, String> {
    let client = &clients.local;
    let db = client.database(&config.mongodb.databases.alpha);

    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(50);
    let skip = ((page - 1) * page_size) as u64;

    let coll_name = alpha_collection_name(&params);
    let collection = db.collection::<mongodb::bson::Document>(&coll_name);

    let t_filter = std::time::Instant::now();
    let is_ra_query = is_ra_query(&params);
    let filter = build_alpha_filter(&params);
    println!("Final Mongo filter: {:?}", filter);
    println!("⏱ filter build took {:?}", t_filter.elapsed());

    let t_count = std::time::Instant::now();
    let t_find = std::time::Instant::now();

    let sort_doc = if let Some(field) = &params.sort_field {
        let order = params.sort_order.unwrap_or(-1);
        doc! { field.mongo_field(): order }
    } else {
        doc! { "dateCreated": -1 }
    };

    let (total, documents) = if is_ra_query {
        let pipeline = ra_parent_page_pipeline(
            filter,
            &coll_name,
            params.messages_nin.as_deref(),
            sort_doc,
            skip,
            page_size,
        );
        let mut cursor = collection
            .aggregate(pipeline, None)
            .await
            .map_err(|e| format!("RA query error: {e}"))?;
        let facet = cursor
            .try_next()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Missing RA page result".to_string())?;
        let counts = facet.get_array("total").map_err(|e| e.to_string())?;
        let total = counts
            .first()
            .and_then(Bson::as_document)
            .map(|count| {
                count
                    .get_i64("count")
                    .or_else(|_| count.get_i32("count").map(i64::from))
                    .unwrap_or(0) as u64
            })
            .unwrap_or(0);
        let documents = facet
            .get_array("data")
            .map_err(|e| e.to_string())?
            .iter()
            .filter_map(Bson::as_document)
            .cloned()
            .collect::<Vec<_>>();
        (total, documents)
    } else {
        let hint = measured_search_hint(&collection, &filter, &sort_doc).await;
        eprintln!("alpha.page collection={} sort_hint={:?}", coll_name, hint);
        let find_options = FindOptions::builder()
            .projection(get_alpha_projection())
            .hint(hint)
            .sort(sort_doc)
            .skip(skip)
            .limit(page_size as i64)
            .build();

        let (total, cursor_res) = if should_include_total(&params) {
            let (count_res, cursor_res) = join!(
                collection.count_documents(
                    filter.clone(),
                    alpha_count_options(&filter, CountOptions::default())
                ),
                collection.find(filter, find_options)
            );
            // Keep legacy page behavior for old clients, including count fallback.
            println!("⏱ count_documents took {:?}", t_count.elapsed());
            (count_res.unwrap_or(0), cursor_res)
        } else {
            // Explicit omitted-total placeholder. Do not schedule any count.
            (0, collection.find(filter, find_options).await)
        };

        let mut cursor = match cursor_res {
            Ok(cursor) => cursor,
            Err(e) => {
                // A concurrently dropped/hidden index must not poison future
                // searches for the entire metadata TTL. No automatic retry.
                if let Some(cache) = SEARCH_INDEX_SNAPSHOTS.get() {
                    if let Ok(mut entries) = cache.lock() {
                        entries.remove(&collection.namespace().to_string());
                    }
                }
                println!("❌ collection.find() error: {}", e);
                return Err(format!("DB query error: {}", e));
            }
        };
        println!("⏱ collection.find() took {:?}", t_find.elapsed());

        let mut documents = Vec::new();
        while let Some(document) = cursor.try_next().await.map_err(|e| e.to_string())? {
            documents.push(document);
        }
        (total, documents)
    };
    let mut results: Vec<AlphaResult> = documents
        .into_iter()
        .filter_map(|document| parse_alpha_document(document, params.embedding.as_deref()))
        .collect();

    let mut all_child_ids: Vec<String> = Vec::new();
    for r in &results {
        if let Some(cids) = &r.children_ids {
            for cid in cids {
                all_child_ids.push(cid.clone());
            }
        }
    }

    if !all_child_ids.is_empty() {
        let child_filter = doc! { "id": { "$in": &all_child_ids } };
        let child_find_options = FindOptions::builder()
            .projection(get_alpha_projection())
            .build();

        let mut child_cursor = collection
            .find(child_filter, child_find_options)
            .await
            .map_err(|e| format!("RA child query error: {e}"))?;
        let mut child_map: HashMap<String, AlphaResult> = HashMap::new();
        let mut child_fails_map: HashMap<String, Vec<String>> = HashMap::new();
        while let Some(child_doc) = child_cursor.try_next().await.map_err(|e| e.to_string())? {
            if let Some((child_result, fails)) = parse_ra_child(
                child_doc,
                params.embedding.as_deref(),
                params.messages_nin.as_deref(),
            ) {
                let cid = child_result.id.clone();
                child_fails_map.insert(cid.clone(), fails);
                child_map.insert(cid, child_result);
            }
        }

        attach_ra_children(&mut results, &child_map, &child_fails_map);
    }

    let ids = prod_correlation_ids(&results);
    if !ids.is_empty() {
        let corr_collection = db.collection::<Document>("alpha_correlations");
        let options = FindOptions::builder()
            .projection(doc! { "_id": 0, "id": 1, "mode": 1, "value": 1,
                "updatedAt": 1, "requestId": 1, "rawResponseHash": 1, "source": 1 })
            .build();
        match corr_collection.find(prod_correlation_filter(ids), options).await {
            Ok(cursor) => match cursor.try_collect::<Vec<Document>>().await {
                Ok(docs) => merge_prod_correlations(&mut results, docs),
                Err(err) => eprintln!("Prod correlation cursor error: {err}"),
            },
            Err(err) => eprintln!("Prod correlation query error: {err}"),
        }
    }

    Ok(PagedResult {
        data: results,
        total,
        page,
        page_size,
    })
}

// pub struct PnlPoint and PnlResponse are shared in quantnight_task_core::data_panel.

#[command]
pub async fn get_pnl_by_id(
    query: PnlQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PnlResponse, String> {
    query_pnl_by_id(&clients.local, &config, &query).await
}

#[command]
pub async fn compute_correlation(
    alpha_ids: Vec<String>,
    config: State<'_, AppConfig>,
) -> Result<HashMap<String, CorrResult>, String> {
    let alpha_str = alpha_ids.join(",");
    let mut replacements = HashMap::new();
    replacements.insert("{alpha_ids}", alpha_str.as_str());

    let stdout = run_python_command(&config, "alpha_correlation", &replacements).await?;

    serde_json::from_str(&stdout).map_err(|e| format!("解析 JSON 失败: {}", e))
}

#[command]
pub async fn get_submission_stats(
    collection: Option<String>,
    query: Option<String>,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<Vec<mongodb::bson::Document>, String> {
    query_submission_stats(&clients.local, &config, collection, query).await
}

#[command]
pub async fn search_alpha_in_all_collections(
    query: SearchAlphaInAllCollectionsQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<Option<AlphaInCollectionResult>, String> {
    query_alpha_in_all_collections(&clients.local, &config, &query).await
}
