//! Shared Data Panel (Datasets / Datafields) and Alpha List / PnL query implementation.
//! Reused by both Tauri desktop commands (`src-tauri/src/datas.rs`) and `taskctl` / `qn` CLI.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Duration, Utc};
use futures_util::join;
use futures_util::stream::TryStreamExt;
use mongodb::{
    bson::{doc, Bson, Document},
    options::{CountOptions, FindOneOptions, FindOptions, Hint, ListIndexesOptions},
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value as JsonValue};

use crate::alpha_query::{category_aliases, parse_query_to_bson, SortField};
use crate::alpha_search_plan;
use crate::cleanup_sync::TaskOutcome;
use crate::config::AppConfig;

/// Flexible Option<f64> deserializer that handles numeric strings or non-numeric placeholders (e.g. "RAM").
pub fn deserialize_option_f64_flexible<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: serde_json::Value =
        serde::Deserialize::deserialize(deserializer).unwrap_or(serde_json::Value::Null);
    match v {
        serde_json::Value::Number(n) => Ok(n.as_f64()),
        serde_json::Value::String(s) => Ok(s.parse::<f64>().ok()),
        _ => Ok(None),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbPyramid {
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbCheck {
    pub name: Option<String>,
    pub result: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub limit: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pyramids: Option<Vec<DbPyramid>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbIsMetrics {
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub sharpe: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub fitness: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub drawdown: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub returns: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub turnover: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub margin: Option<f64>,
    pub long_count: Option<i32>,
    pub short_count: Option<i32>,
    pub checks: Option<Vec<DbCheck>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbOsMetrics {
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub sharpe: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub fitness: Option<f64>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbCode {
    pub code: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbAnalysisEmbedding {
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub x: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub y: Option<f64>,
    pub cluster: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbAnalysis {
    pub embeddings: Option<HashMap<String, DbAnalysisEmbedding>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbClassification {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbAlphaDocument {
    pub id: Option<String>,
    pub r#type: Option<String>,
    pub settings: Option<serde_json::Value>,
    pub regular: Option<DbCode>,
    pub selection: Option<DbCode>,
    pub combo: Option<DbCode>,
    pub is: Option<DbIsMetrics>,
    pub os: Option<DbOsMetrics>,
    #[serde(rename = "pnl_score", default, deserialize_with = "deserialize_option_f64_flexible")]
    pub pnl_score: Option<f64>,
    pub date_created: Option<String>,
    pub date_submitted: Option<String>,
    pub classifications: Option<Vec<DbClassification>>,
    pub current_prod_correlation: Option<serde_json::Value>,
    pub analysis: Option<DbAnalysis>,
    pub children: Option<Vec<String>>,
    pub parent: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AlphaResult {
    pub id: String,
    pub region: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_type: Option<String>,
    pub settings: Option<serde_json::Value>,
    pub code: Option<String>,
    pub sharpe: Option<f64>,
    pub fitness: Option<f64>,
    pub drawdown: Option<f64>,
    pub returns: Option<f64>,
    pub turnover: Option<f64>,
    pub margin: Option<f64>,
    pub long_count: Option<i32>,
    pub short_count: Option<i32>,
    pub sub_universe_sharpe: Option<f64>,
    pub message: Option<String>,
    pub date_created: Option<String>,
    pub date_submitted: Option<String>,
    pub pnl_score: Option<f64>,
    pub os_sharpe: Option<f64>,
    pub os_fitness: Option<f64>,
    pub cluster_x: Option<f64>,
    pub cluster_y: Option<f64>,
    pub cluster_id: Option<serde_json::Value>,
    pub classifications: Option<Vec<serde_json::Value>>,
    #[serde(rename = "currentProdCorrelation")]
    pub current_prod_correlation: Option<serde_json::Value>,
    pub self_category: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<AlphaResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checks: Option<Vec<DbCheck>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pnl_scores: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub submitted: Option<bool>,
}

impl DbAlphaDocument {
    pub fn into_alpha_result(self, embedding_key: Option<&str>) -> Option<AlphaResult> {
        self.into_alpha_result_with_options(embedding_key, false)
    }

    pub fn into_alpha_result_with_options(
        self,
        embedding_key: Option<&str>,
        include_checks: bool,
    ) -> Option<AlphaResult> {
        let id = self.id?;
        let r#type = self.r#type.unwrap_or_else(|| "UNKNOWN".to_string());

        let settings_val = self.settings.unwrap_or_else(|| serde_json::json!({}));
        let region = settings_val
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let code = match r#type.as_str() {
            "REGULAR" | "RA_PARENT" | "RA_CHILD" => self.regular.and_then(|r| r.code),
            "SUPER" => {
                let selection_code = self.selection.and_then(|r| r.code).unwrap_or_default();
                let combo_code = self.combo.and_then(|r| r.code).unwrap_or_default();
                Some(format!("{}\n{}", selection_code, combo_code))
            }
            _ => self.regular.and_then(|r| r.code),
        };

        let mut sub_universe_sharpe = None;
        let mut message = None;
        let mut pyramid_names = Vec::new();
        let mut raw_checks = None;

        if let Some(is_metrics) = &self.is {
            if let Some(checks) = &is_metrics.checks {
                if include_checks {
                    raw_checks = Some(checks.clone());
                }
                let mut msg_parts = Vec::new();
                for check in checks {
                    let name = check.name.as_deref().unwrap_or("");
                    let result = check.result.as_deref().unwrap_or("");

                    if name == "MATCHES_PYRAMID" {
                        if let Some(pyramids) = &check.pyramids {
                            for p in pyramids {
                                if let Some(pname) = &p.name {
                                    let short_name =
                                        pname.split('/').last().unwrap_or(pname).to_string();
                                    pyramid_names.push(short_name);
                                }
                            }
                        }
                    }

                    if name == "LOW_SUB_UNIVERSE_SHARPE" {
                        sub_universe_sharpe = check.value;
                    }

                    if (result == "FAIL" || result == "WARNING") && !name.is_empty() {
                        let part = if let (Some(l), Some(v)) = (check.limit, check.value) {
                            format!("{}({:.2}/{:.2})", name, v, l)
                        } else if let Some(m) = &check.message {
                            format!("{}:{}", name, m)
                        } else {
                            name.to_string()
                        };
                        msg_parts.push(part);
                    }
                }
                if !msg_parts.is_empty() {
                    message = Some(msg_parts.join(","));
                }
            }
        }

        let classifications = self.classifications.map(|arr| {
            arr.into_iter()
                .filter_map(|c| {
                    if let (Some(cid), Some(cname)) = (c.id, c.name) {
                        Some(serde_json::json!({ "id": cid, "name": cname }))
                    } else {
                        None
                    }
                })
                .collect()
        });

        let mut cluster_x = None;
        let mut cluster_y = None;
        let mut cluster_id = None;

        if let Some(analysis) = self.analysis {
            if let Some(embeddings) = analysis.embeddings {
                if let Some(key) = embedding_key {
                    if let Some(umap) = embeddings.get(key) {
                        cluster_x = umap.x;
                        cluster_y = umap.y;
                        if let Some(cluster_val) = &umap.cluster {
                            if let Some(obj) = cluster_val.as_object() {
                                if let Some(id_val) = obj.get("id") {
                                    cluster_id = Some(id_val.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Some(AlphaResult {
            id,
            region,
            alpha_type: Some(r#type),
            settings: Some(settings_val),
            code,
            sharpe: self.is.as_ref().and_then(|is| is.sharpe),
            fitness: self.is.as_ref().and_then(|is| is.fitness),
            drawdown: self.is.as_ref().and_then(|is| is.drawdown),
            returns: self.is.as_ref().and_then(|is| is.returns),
            turnover: self.is.as_ref().and_then(|is| is.turnover),
            margin: self.is.as_ref().and_then(|is| is.margin),
            long_count: self.is.as_ref().and_then(|is| is.long_count),
            short_count: self.is.as_ref().and_then(|is| is.short_count),
            sub_universe_sharpe,
            message,
            date_created: self.date_created,
            date_submitted: self.date_submitted,
            pnl_score: self.pnl_score,
            os_sharpe: self.os.as_ref().and_then(|os| os.sharpe),
            os_fitness: self.os.as_ref().and_then(|os| os.fitness),
            cluster_x,
            cluster_y,
            cluster_id,
            classifications,
            current_prod_correlation: self.current_prod_correlation,
            self_category: if !pyramid_names.is_empty() {
                Some(pyramid_names)
            } else {
                None
            },
            parent: self.parent,
            children: None,
            children_ids: self.children,
            checks: raw_checks,
            pnl_scores: None,
            submitted: None,
        })
    }
}

pub fn parse_alpha_document(
    doc: mongodb::bson::Document,
    embedding_key: Option<&str>,
) -> Option<AlphaResult> {
    parse_alpha_document_with_options(doc, embedding_key, false)
}

pub fn parse_alpha_document_with_options(
    doc: mongodb::bson::Document,
    embedding_key: Option<&str>,
    include_checks: bool,
) -> Option<AlphaResult> {
    match mongodb::bson::from_document::<DbAlphaDocument>(doc) {
        Ok(db_alpha) => db_alpha.into_alpha_result_with_options(embedding_key, include_checks),
        Err(e) => {
            println!("Error deserializing alpha document: {}", e);
            None
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AlphaQuery {
    pub query: Option<String>,
    pub exclude_query: Option<String>,
    pub id: Option<String>,
    pub messages_in: Option<Vec<String>>,
    pub messages_nin: Option<Vec<String>>,
    pub classifications_in: Option<Vec<String>>,
    pub region: Option<String>,
    pub universe: Option<String>,
    pub days_within: Option<u32>,
    #[serde(default)]
    pub min_sharpe: Option<f64>,
    #[serde(default)]
    pub min_fitness: Option<f64>,
    #[serde(default)]
    pub max_sharpe: Option<f64>,
    #[serde(default)]
    pub max_fitness: Option<f64>,
    pub min_turnover: Option<f64>,
    pub max_turnover: Option<f64>,
    pub min_margin: Option<f64>,
    pub delay: Option<u32>,
    #[serde(default)]
    pub language: Option<String>,
    pub min_returns: Option<f64>,
    pub alpha_type: Option<String>,
    pub self_categories: Option<Vec<String>>,
    pub collection: Option<String>,
    pub embedding: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    /// Missing preserves legacy totals; false omits only non-RA totals.
    pub include_total: Option<bool>,
    pub sort_field: Option<SortField>,
    pub sort_order: Option<i32>,
    #[serde(default)]
    pub include_checks: Option<bool>,
    #[serde(default)]
    pub include_pnl_scores: Option<bool>,
    #[serde(default)]
    pub output: Option<PathBuf>,
}

pub fn sanitize_collection_name(name: &str) -> Option<String> {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(name.to_string())
    } else {
        None
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PagedResult<T> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DatasetRegionData {
    pub region: String,
    pub delay: i32,
    pub universe: String,
    pub coverage: Option<f64>,
    pub date_coverage: Option<f64>,
    pub field_count: Option<f64>,
    pub alpha_count: Option<f64>,
    pub user_count: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Dataset {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<JsonValue>,
    pub subcategory: Option<JsonValue>,
    pub data: Vec<DatasetRegionData>,
    #[serde(rename = "dateUpdated", default)]
    pub date_updated: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DatasetQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub field_id: Option<String>,
    pub category: Option<String>,
    pub region: Option<String>,
    pub universe: Option<String>,
    pub delay: Option<i32>,
    pub sort_field: Option<String>,
    pub sort_order: Option<i32>,
    #[serde(default)]
    pub output: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DatafieldRegionData {
    pub region: String,
    pub delay: i32,
    pub universe: String,
    pub coverage: Option<f64>,
    pub date_coverage: Option<f64>,
    pub alpha_count: Option<f64>,
    pub user_count: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Datafield {
    pub id: String,
    pub description: Option<String>,
    pub r#type: Option<String>,
    pub category: Option<JsonValue>,
    pub subcategory: Option<JsonValue>,
    pub dataset: Option<JsonValue>,
    pub data: Vec<DatafieldRegionData>,
    #[serde(rename = "dateCreated", default)]
    pub date_created: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DatafieldQuery {
    #[serde(default)]
    pub dataset_id: String,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    #[serde(default)]
    pub field_type: Option<String>,
    pub region: Option<String>,
    pub universe: Option<String>,
    pub delay: Option<i32>,
    pub sort_field: Option<String>,
    pub sort_order: Option<i32>,
    #[serde(default)]
    pub output: Option<PathBuf>,
}

pub async fn query_datafields(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &DatafieldQuery,
) -> Result<PagedResult<Datafield>, String> {
    let db_name = config.mongodb.databases.datafield.as_deref().unwrap_or("data_db");
    let coll_name = config
        .mongodb
        .databases
        .datafield_collection
        .as_deref()
        .unwrap_or("datafields_all");

    let db = client.database(db_name);
    let collection = db.collection::<mongodb::bson::Document>(coll_name);

    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).max(1);
    let skip = ((page - 1) * page_size) as u64;

    let mut filters = vec![];
    if !params.dataset_id.trim().is_empty() {
        filters.push(doc! { "dataset.id": params.dataset_id.trim() });
    }

    if let Some(ft) = &params.field_type {
        if !ft.trim().is_empty() {
            filters.push(doc! { "type": ft.trim().to_uppercase() });
        }
    }

    if let Some(search) = &params.search {
        if !search.is_empty() {
            let escaped = regex::escape(search.trim());
            filters.push(doc! {
                "$or": [
                    { "id": { "$regex": &escaped, "$options": "i" } },
                    { "description": { "$regex": &escaped, "$options": "i" } }
                ]
            });
        }
    }

    let mut data_filters = vec![];
    if let Some(region) = &params.region {
        if !region.is_empty() {
            data_filters.push(doc! { "region": region });
        }
    }
    if let Some(universe) = &params.universe {
        if !universe.is_empty() {
            data_filters.push(doc! { "universe": { "$regex": regex::escape(universe.trim()), "$options": "i" } });
        }
    }
    if let Some(delay) = params.delay {
        data_filters.push(doc! { "delay": delay });
    }

    if !data_filters.is_empty() {
        filters.push(doc! { "data": { "$elemMatch": { "$and": data_filters } } });
    }

    let filter = if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    };

    let sort_field = params.sort_field.as_deref().unwrap_or("id");
    let sort_order = params.sort_order.unwrap_or(1);

    let mut add_fields = doc! {};
    let mongo_sort_field;

    match sort_field {
        "alphaCount" | "coverage" | "dateCoverage" => {
            let mut filter_conds = vec![];
            if let Some(reg) = &params.region {
                if !reg.is_empty() {
                    filter_conds.push(doc! { "$eq": ["$$this.region", reg] });
                }
            }
            if let Some(univ) = &params.universe {
                if !univ.is_empty() {
                    filter_conds.push(doc! { "$regexMatch": { "input": "$$this.universe", "regex": regex::escape(univ.trim()), "options": "i" } });
                }
            }
            if let Some(delay) = params.delay {
                filter_conds.push(doc! { "$eq": ["$$this.delay", delay] });
            }

            if filter_conds.is_empty() {
                if sort_field == "alphaCount" {
                    add_fields.insert("sortValue", doc! { "$sum": "$data.alphaCount" });
                } else {
                    let field_name = if sort_field == "coverage" {
                        "coverage"
                    } else {
                        "dateCoverage"
                    };
                    add_fields.insert("sortValue", doc! { "$max": format!("$data.{}", field_name) });
                }
            } else {
                let field_name = if sort_field == "alphaCount" {
                    "alphaCount"
                } else if sort_field == "coverage" {
                    "coverage"
                } else {
                    "dateCoverage"
                };
                add_fields.insert(
                    "sortValue",
                    doc! {
                        "$reduce": {
                            "input": {
                                "$filter": {
                                    "input": "$data",
                                    "cond": { "$and": filter_conds }
                                }
                            },
                            "initialValue": 0.0,
                            "in": { "$add": ["$$value", { "$ifNull": [format!("$$this.{}", field_name), 0.0] }] }
                        }
                    },
                );
            }
            mongo_sort_field = "sortValue";
        }
        "id" => {
            mongo_sort_field = "id";
        }
        "dateCreated" => {
            mongo_sort_field = "dateCreated";
        }
        _ => {
            mongo_sort_field = "id";
        }
    }

    let mut pipeline = vec![doc! { "$match": filter.clone() }];

    if !add_fields.is_empty() {
        pipeline.push(doc! { "$addFields": add_fields });
    }

    pipeline.push(doc! {
        "$project": {
            "id": 1,
            "description": 1,
            "type": 1,
            "category": 1,
            "subcategory": 1,
            "dataset": 1,
            "data": 1,
            "sortValue": 1,
            "dateCreated": 1,
        }
    });
    pipeline.push(doc! { "$sort": { mongo_sort_field: sort_order } });
    pipeline.push(doc! { "$skip": skip as i64 });
    pipeline.push(doc! { "$limit": page_size as i64 });

    let total = collection
        .count_documents(filter, None)
        .await
        .map_err(|e| e.to_string())?;
    let mut cursor = collection
        .aggregate(pipeline, None)
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        if let Ok(field) = mongodb::bson::from_document::<Datafield>(doc) {
            results.push(field);
        }
    }

    Ok(PagedResult {
        data: results,
        total,
        page,
        page_size,
    })
}

pub async fn query_datasets(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &DatasetQuery,
) -> Result<PagedResult<Dataset>, String> {
    let db_name = config.mongodb.databases.dataset.as_deref().unwrap_or("dataset_db");
    let coll_name = config
        .mongodb
        .databases
        .dataset_collection
        .as_deref()
        .unwrap_or("datasets_all");

    let db = client.database(db_name);
    let collection = db.collection::<mongodb::bson::Document>(coll_name);

    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).max(1);
    let skip = ((page - 1) * page_size) as u64;

    let mut filters = vec![];

    if let Some(search) = &params.search {
        if !search.is_empty() {
            let escaped = regex::escape(search.trim());
            filters.push(doc! {
                "$or": [
                    { "name": { "$regex": &escaped, "$options": "i" } },
                    { "id": { "$regex": &escaped, "$options": "i" } },
                    { "description": { "$regex": &escaped, "$options": "i" } }
                ]
            });
        }
    }

    if let Some(id) = &params.id {
        if !id.is_empty() {
            filters.push(doc! { "id": { "$regex": regex::escape(id.trim()), "$options": "i" } });
        }
    }

    if let Some(field_id) = &params.field_id {
        if !field_id.is_empty() {
            let df_db_name = config.mongodb.databases.datafield.as_deref().unwrap_or("data_db");
            let df_coll_name = config
                .mongodb
                .databases
                .datafield_collection
                .as_deref()
                .unwrap_or("datafields_all");
            let df_coll = client
                .database(df_db_name)
                .collection::<mongodb::bson::Document>(df_coll_name);

            let df_filter =
                doc! { "id": { "$regex": regex::escape(field_id.trim()), "$options": "i" } };
            let mut cursor = df_coll.find(df_filter, None).await.map_err(|e| e.to_string())?;

            let mut dataset_ids = Vec::new();
            while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
                if let Ok(ds) = doc.get_document("dataset") {
                    if let Ok(ds_id) = ds.get_str("id") {
                        dataset_ids.push(ds_id.to_string());
                    }
                }
            }

            if dataset_ids.is_empty() {
                filters.push(doc! { "id": "__NONE__" });
            } else {
                filters.push(doc! { "id": { "$in": dataset_ids } });
            }
        }
    }

    if let Some(name) = &params.name {
        if !name.is_empty() {
            filters
                .push(doc! { "name": { "$regex": regex::escape(name.trim()), "$options": "i" } });
        }
    }

    if let Some(cat) = &params.category {
        if !cat.is_empty() {
            let aliases = category_aliases(cat.trim());
            let or_conds: Vec<Document> = aliases
                .iter()
                .map(|a| doc! { "category.name": { "$regex": regex::escape(a), "$options": "i" } })
                .collect();
            filters.push(doc! { "$or": or_conds });
        }
    }

    let mut data_filters = vec![];
    if let Some(region) = &params.region {
        if !region.is_empty() {
            data_filters.push(doc! { "region": region });
        }
    }
    if let Some(universe) = &params.universe {
        if !universe.is_empty() {
            data_filters.push(doc! { "universe": { "$regex": regex::escape(universe.trim()), "$options": "i" } });
        }
    }
    if let Some(delay) = params.delay {
        data_filters.push(doc! { "delay": delay });
    }

    if !data_filters.is_empty() {
        filters.push(doc! { "data": { "$elemMatch": { "$and": data_filters } } });
    }

    let filter = if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    };

    let sort_field = params.sort_field.as_deref().unwrap_or("id");
    let sort_order = params.sort_order.unwrap_or(1);

    let mongo_sort_field = match sort_field {
        "id" => "id",
        "name" => "name",
        "totalFieldCount" => "totalFieldCount",
        "totalAlphaCount" => "totalAlphaCount",
        "dateUpdated" => "dateUpdated",
        _ => "id",
    };

    let pipeline = vec![
        doc! { "$match": filter.clone() },
        doc! {
            "$addFields": {
                "totalFieldCount": { "$sum": "$data.fieldCount" },
                "totalAlphaCount": { "$sum": "$data.alphaCount" }
            }
        },
        doc! { "$project": { "description_embedding": 0, "embedding": 0 } },
        doc! { "$sort": { mongo_sort_field: sort_order } },
        doc! { "$skip": skip as i64 },
        doc! { "$limit": page_size as i64 },
    ];

    let total = collection
        .count_documents(filter, None)
        .await
        .map_err(|e| e.to_string())?;
    let mut cursor = collection
        .aggregate(pipeline, None)
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        if let Ok(dataset) = mongodb::bson::from_document::<Dataset>(doc) {
            results.push(dataset);
        }
    }

    Ok(PagedResult {
        data: results,
        total,
        page,
        page_size,
    })
}

pub fn prod_correlation_ids(results: &[AlphaResult]) -> Vec<String> {
    let mut ids = HashSet::new();
    for result in results {
        ids.insert(result.id.clone());
        if let Some(children) = &result.children {
            ids.extend(children.iter().map(|child| child.id.clone()));
        }
    }
    ids.into_iter().collect()
}

pub fn correlation_timestamp(value: &JsonValue) -> Option<i64> {
    let date = value.get("updatedAt")?;
    if let Some(s) = date.as_str() {
        return DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.timestamp_millis());
    }
    let date = date.get("$date")?;
    if let Some(s) = date.as_str() {
        return DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.timestamp_millis());
    }
    date.get("$numberLong")?.as_str()?.parse::<i64>().ok()
}

pub fn has_correlation_value(snapshot: &JsonValue) -> bool {
    snapshot.get("value").is_some_and(|v| !v.is_null())
}

pub fn prefer_prod_correlation(existing: Option<&JsonValue>, fallback: &JsonValue) -> bool {
    if !has_correlation_value(fallback) {
        return false;
    }
    match existing.filter(|v| has_correlation_value(v)) {
        None => true,
        Some(current) => matches!(
            (correlation_timestamp(current), correlation_timestamp(fallback)),
            (Some(old), Some(new)) if new > old
        ),
    }
}

pub fn prod_correlation_filter(ids: Vec<String>) -> Document {
    doc! { "id": { "$in": ids }, "mode": "prod" }
}

pub fn merge_prod_correlations(results: &mut [AlphaResult], docs: Vec<Document>) {
    let mut by_id: HashMap<String, JsonValue> = HashMap::new();
    for mut doc in docs {
        if doc.get_str("mode") != Ok("prod") {
            continue;
        }
        let Ok(id) = doc.get_str("id").map(str::to_owned) else {
            continue;
        };
        doc.remove("_id");
        doc.remove("id");
        if let Some(Bson::DateTime(date)) = doc.get("updatedAt") {
            let iso = DateTime::<Utc>::from(date.to_system_time()).to_rfc3339();
            doc.insert("updatedAt", iso);
        }
        let Ok(snapshot) = mongodb::bson::from_document::<JsonValue>(doc) else {
            continue;
        };
        if !has_correlation_value(&snapshot) {
            continue;
        }
        let entry = by_id.entry(id).or_insert(JsonValue::Null);
        if prefer_prod_correlation(Some(entry), &snapshot) {
            *entry = snapshot;
        }
    }
    for result in results {
        if let Some(snapshot) = by_id.get(&result.id) {
            if prefer_prod_correlation(result.current_prod_correlation.as_ref(), snapshot) {
                result.current_prod_correlation = Some(snapshot.clone());
            }
        }
        if let Some(children) = &mut result.children {
            for child in children {
                if let Some(snapshot) = by_id.get(&child.id) {
                    if prefer_prod_correlation(child.current_prod_correlation.as_ref(), snapshot) {
                        child.current_prod_correlation = Some(snapshot.clone());
                    }
                }
            }
        }
    }
}

pub fn get_alpha_projection() -> mongodb::bson::Document {
    doc! {
        "id": 1,
        "type": 1,
        "settings": 1,
        "regular.code": 1,
        "selection.code": 1,
        "combo.code": 1,
        "children": 1,
        "parent": 1,
        "is": {
            "sharpe": 1,
            "fitness": 1,
            "drawdown": 1,
            "turnover": 1,
            "margin": 1,
            "longCount": 1,
            "shortCount": 1,
            "returns": 1,
            "checks": {
                "name": 1,
                "result": 1,
                "limit": 1,
                "value": 1,
                "message": 1,
                "pyramids": 1,
            },
        },
        "os": {
            "sharpe": 1,
            "fitness": 1,
        },
        "pnl_score": 1,
        "dateCreated": 1,
        "dateSubmitted": 1,
        "classifications": 1,
        "analysis.embeddings": 1,
        "currentProdCorrelation": 1,
    }
}

pub fn parse_ra_child(
    child_doc: Document,
    embedding_key: Option<&str>,
    excluded_messages: Option<&[String]>,
) -> Option<(AlphaResult, Vec<String>)> {
    parse_ra_child_with_options(child_doc, embedding_key, excluded_messages, false)
}

pub fn parse_ra_child_with_options(
    child_doc: Document,
    embedding_key: Option<&str>,
    excluded_messages: Option<&[String]>,
    include_checks: bool,
) -> Option<(AlphaResult, Vec<String>)> {
    let mut fails = Vec::new();
    if let Ok(is_doc) = child_doc.get_document("is") {
        if let Ok(checks) = is_doc.get_array("checks") {
            for check in checks {
                if let Some(check) = check.as_document() {
                    let name = check.get_str("name").unwrap_or("");
                    let result = check.get_str("result").unwrap_or("");
                    if (result == "FAIL" || result == "WARNING") && !name.is_empty() {
                        fails.push(name.to_string());
                    }
                }
            }
        }
    }
    if let Some(excluded) = excluded_messages {
        if fails.iter().any(|name| excluded.contains(name)) {
            return None;
        }
    }
    parse_alpha_document_with_options(child_doc, embedding_key, include_checks)
        .map(|child| (child, fails))
}

pub fn ra_parent_page_pipeline(
    filter: Document,
    coll_name: &str,
    excluded_messages: Option<&[String]>,
    sort: Document,
    skip: u64,
    page_size: u32,
) -> Vec<Document> {
    let mut child_pipeline = vec![doc! { "$match": { "id": { "$type": "string" } } }];
    if let Some(messages) = excluded_messages.filter(|messages| !messages.is_empty()) {
        child_pipeline.push(doc! {
            "$match": {
                "is.checks": {
                    "$not": {
                        "$elemMatch": {
                            "name": { "$in": messages },
                            "result": { "$in": ["FAIL", "WARNING"] }
                        }
                    }
                }
            }
        });
    }
    child_pipeline.push(doc! { "$limit": 2 });
    child_pipeline.push(doc! { "$project": { "_id": 0, "id": 1 } });
    vec![
        doc! { "$match": filter },
        doc! { "$lookup": {
            "from": coll_name,
            "localField": "children",
            "foreignField": "id",
            "pipeline": child_pipeline,
            "as": "_matchingRaChildren"
        } },
        doc! { "$match": { "_matchingRaChildren.1": { "$exists": true } } },
        doc! { "$project": { "_matchingRaChildren": 0 } },
        doc! { "$facet": {
            "data": [
                { "$sort": sort },
                { "$skip": skip as i64 },
                { "$limit": page_size as i64 },
                { "$project": get_alpha_projection() }
            ],
            "total": [{ "$count": "count" }]
        } },
    ]
}

pub fn attach_ra_children(
    results: &mut [AlphaResult],
    child_map: &HashMap<String, AlphaResult>,
    child_fails_map: &HashMap<String, Vec<String>>,
) {
    for parent in results {
        if let Some(cids) = &parent.children_ids {
            let mut children = Vec::new();
            let mut check_fail_counts: HashMap<String, usize> = HashMap::new();
            for cid in cids {
                if let Some(child) = child_map.get(cid) {
                    children.push(child.clone());
                }
                if let Some(fails) = child_fails_map.get(cid) {
                    for f in fails {
                        *check_fail_counts.entry(f.clone()).or_insert(0) += 1;
                    }
                }
            }
            parent.sharpe = None;
            parent.fitness = None;
            parent.returns = None;
            parent.turnover = None;
            parent.margin = None;
            parent.drawdown = None;
            parent.pnl_score = None;
            parent.sub_universe_sharpe = None;
            parent.os_sharpe = None;
            parent.os_fitness = None;
            parent.long_count = None;
            parent.short_count = None;
            parent.message = None;
            if !children.is_empty() {
                let valid_sharpes: Vec<f64> = children.iter().filter_map(|c| c.sharpe).collect();
                if !valid_sharpes.is_empty() {
                    parent.sharpe =
                        Some(valid_sharpes.iter().sum::<f64>() / valid_sharpes.len() as f64);
                }

                let valid_fitness: Vec<f64> = children.iter().filter_map(|c| c.fitness).collect();
                if !valid_fitness.is_empty() {
                    parent.fitness =
                        Some(valid_fitness.iter().sum::<f64>() / valid_fitness.len() as f64);
                }

                let valid_returns: Vec<f64> = children.iter().filter_map(|c| c.returns).collect();
                if !valid_returns.is_empty() {
                    parent.returns =
                        Some(valid_returns.iter().sum::<f64>() / valid_returns.len() as f64);
                }

                let valid_turnover: Vec<f64> = children.iter().filter_map(|c| c.turnover).collect();
                if !valid_turnover.is_empty() {
                    parent.turnover =
                        Some(valid_turnover.iter().sum::<f64>() / valid_turnover.len() as f64);
                }

                let valid_margin: Vec<f64> = children.iter().filter_map(|c| c.margin).collect();
                if !valid_margin.is_empty() {
                    parent.margin =
                        Some(valid_margin.iter().sum::<f64>() / valid_margin.len() as f64);
                }

                let valid_dd: Vec<f64> = children.iter().filter_map(|c| c.drawdown).collect();
                if !valid_dd.is_empty() {
                    parent.drawdown = valid_dd.iter().cloned().reduce(f64::max);
                }

                let valid_score: Vec<f64> = children.iter().filter_map(|c| c.pnl_score).collect();
                if !valid_score.is_empty() {
                    parent.pnl_score =
                        Some(valid_score.iter().sum::<f64>() / valid_score.len() as f64);
                }

                let valid_sub_u: Vec<f64> = children
                    .iter()
                    .filter_map(|c| c.sub_universe_sharpe)
                    .collect();
                if !valid_sub_u.is_empty() {
                    parent.sub_universe_sharpe =
                        Some(valid_sub_u.iter().sum::<f64>() / valid_sub_u.len() as f64);
                }

                let valid_os_s: Vec<f64> = children.iter().filter_map(|c| c.os_sharpe).collect();
                if !valid_os_s.is_empty() {
                    parent.os_sharpe =
                        Some(valid_os_s.iter().sum::<f64>() / valid_os_s.len() as f64);
                }

                let valid_os_f: Vec<f64> = children.iter().filter_map(|c| c.os_fitness).collect();
                if !valid_os_f.is_empty() {
                    parent.os_fitness =
                        Some(valid_os_f.iter().sum::<f64>() / valid_os_f.len() as f64);
                }

                let long_cnts: Vec<i32> = children.iter().filter_map(|c| c.long_count).collect();
                if !long_cnts.is_empty() {
                    parent.long_count = Some(long_cnts.iter().sum());
                }

                let short_cnts: Vec<i32> = children.iter().filter_map(|c| c.short_count).collect();
                if !short_cnts.is_empty() {
                    parent.short_count = Some(short_cnts.iter().sum());
                }

                if !check_fail_counts.is_empty() {
                    let mut sorted_checks: Vec<(String, usize)> =
                        check_fail_counts.into_iter().collect();
                    sorted_checks.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                    let msg_items: Vec<String> = sorted_checks
                        .into_iter()
                        .map(|(name, count)| format!("{}({})", name, count))
                        .collect();
                    parent.message = Some(msg_items.join(", "));
                }
            }
            parent.children = Some(children);
        }
    }
}

pub fn is_ra_query(params: &AlphaQuery) -> bool {
    matches!(params.alpha_type.as_deref(), Some("RA" | "RA_PARENT"))
}

pub fn should_include_total(params: &AlphaQuery) -> bool {
    is_ra_query(params) || params.include_total.unwrap_or(true)
}

pub fn alpha_collection_name(params: &AlphaQuery) -> String {
    params
        .collection
        .as_deref()
        .and_then(sanitize_collection_name)
        .unwrap_or_else(|| "alpha_results".to_string())
}

pub fn build_alpha_filter(params: &AlphaQuery) -> Document {
    let mut filters = vec![];
    let is_ra_query = is_ra_query(params);

    if let Some(q) = &params.query {
        if let Some(parsed_doc) = parse_query_to_bson(q) {
            filters.push(parsed_doc);
        } else if !q.trim().is_empty() {
            filters.push(doc! {
                "$or": [
                    { "regular.code": { "$regex": q.trim(), "$options": "i" } },
                    { "selection.code": { "$regex": q.trim(), "$options": "i" } },
                    { "combo.code": { "$regex": q.trim(), "$options": "i" } }
                ]
            });
        }
    }

    if let Some(ex_q) = &params.exclude_query {
        if !ex_q.trim().is_empty() {
            filters.push(doc! {
                "$and": [
                    { "regular.code": { "$not": { "$regex": ex_q.trim(), "$options": "i" } } },
                    { "selection.code": { "$not": { "$regex": ex_q.trim(), "$options": "i" } } },
                    { "combo.code": { "$not": { "$regex": ex_q.trim(), "$options": "i" } } }
                ]
            });
        }
    }

    if let Some(messages) = &params.messages_in {
        if !messages.is_empty() {
            filters.push(doc! {
                "is.checks": {
                    "$elemMatch": {
                        "name": { "$in": messages },
                        "result": { "$in": ["FAIL", "WARNING"] }
                    }
                }
            });
        }
    }

    if !is_ra_query {
        if let Some(messages) = &params.messages_nin {
            if !messages.is_empty() {
                filters.push(doc! {
                    "is.checks": {
                        "$not": {
                            "$elemMatch": {
                                "name": { "$in": messages },
                                "result": { "$in": ["FAIL", "WARNING"] }
                            }
                        }
                    }
                });
            }
        }
    }

    if let Some(classifications) = &params.classifications_in {
        if !classifications.is_empty() {
            filters.push(doc! {
                "classifications": {
                    "$elemMatch": {
                        "id": { "$in": classifications }
                    }
                }
            });
        }
    }

    if let Some(id) = &params.id {
        if is_ra_query {
            filters.push(doc! {
                "$or": [
                    { "id": id },
                    { "children": id }
                ]
            });
        } else {
            filters.push(doc! { "id": id });
        }
    }
    if let Some(region) = &params.region {
        filters.push(doc! { "settings.region": region });
    }
    if let Some(universe) = &params.universe {
        if !universe.trim().is_empty() {
            filters.push(doc! {
                "settings.universe": { "$regex": regex::escape(universe.trim()), "$options": "i" }
            });
        }
    }
    if let Some(delay) = params.delay {
        filters.push(doc! { "settings.delay": delay as i32 });
    }
    if let Some(days) = params.days_within {
        let since = Utc::now() - Duration::days(days as i64);
        filters.push(doc! { "dateCreated": { "$gte": since.to_rfc3339() } });
    }
    if let Some(min_sharpe) = params.min_sharpe {
        if !is_ra_query {
            filters.push(doc! { "is.sharpe": { "$gte": min_sharpe } });
        }
    }
    if let Some(min_fitness) = params.min_fitness {
        if !is_ra_query {
            filters.push(doc! { "is.fitness": { "$gte": min_fitness } });
        }
    }
    if let Some(max_sharpe) = params.max_sharpe {
        if !is_ra_query {
            filters.push(doc! { "is.sharpe": { "$lte": max_sharpe } });
        }
    }
    if let Some(max_fitness) = params.max_fitness {
        if !is_ra_query {
            filters.push(doc! { "is.fitness": { "$lte": max_fitness } });
        }
    }
    if let Some(language) = &params.language {
        if !language.trim().is_empty() {
            filters.push(doc! { "settings.language": language.trim() });
        }
    }
    if let Some(min) = params.min_turnover {
        filters.push(doc! { "is.turnover": { "$gte": min } });
    }
    if let Some(max) = params.max_turnover {
        filters.push(doc! { "is.turnover": { "$lte": max } });
    }
    if let Some(min_margin) = params.min_margin {
        filters.push(doc! {
            "$expr": { "$gte": [ { "$abs": "$is.margin" }, min_margin ] }
        });
    }
    if let Some(min_returns) = params.min_returns {
        filters.push(doc! {
            "$expr": { "$gte": [ { "$abs": "$is.returns" }, min_returns ] }
        });
    }
    if let Some(alpha_type) = &params.alpha_type {
        if alpha_type == "RA" {
            filters.push(doc! { "type": "RA_PARENT" });
        } else {
            filters.push(doc! { "type": alpha_type });
        }
    }
    if let Some(categories) = &params.self_categories {
        if !categories.is_empty() {
            let mut and_conds = Vec::new();
            for cat in categories {
                let aliases = category_aliases(cat.trim());
                let mut or_conds: Vec<Document> = Vec::new();
                for a in aliases {
                    let pattern = format!("{}$", regex::escape(&a));
                    or_conds.push(doc! {
                        "is.checks": {
                            "$elemMatch": {
                                "name": "MATCHES_PYRAMID",
                                "pyramids": {
                                    "$elemMatch": {
                                        "name": { "$regex": pattern, "$options": "i" }
                                    }
                                }
                            }
                        }
                    });
                }
                and_conds.push(doc! { "$or": or_conds });
            }
            filters.push(doc! { "$and": and_conds });
        }
    }

    if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    }
}

type SearchIndexSnapshot = (Instant, Vec<mongodb::IndexModel>);
static CORE_SEARCH_INDEX_SNAPSHOTS: OnceLock<Mutex<HashMap<String, SearchIndexSnapshot>>> =
    OnceLock::new();

pub async fn measured_search_hint(
    collection: &mongodb::Collection<Document>,
    filter: &Document,
    sort: &Document,
) -> Option<Hint> {
    if alpha_search_plan::simple_keyword_scope(filter) != Some(true)
        || sort.len() != 1
        || !sort
            .get("is.sharpe")
            .is_some_and(alpha_search_plan::index_direction)
    {
        return None;
    }
    let key = collection.namespace().to_string();
    let cache = CORE_SEARCH_INDEX_SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()));
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
        collection
            .list_indexes(options)
            .await?
            .try_collect::<Vec<_>>()
            .await
    };
    let indexes = match tokio::time::timeout(StdDuration::from_secs(1), read).await {
        Ok(Ok(indexes)) => indexes,
        _ => return None,
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

pub fn evict_search_index_snapshot(namespace: &str) {
    if let Some(cache) = CORE_SEARCH_INDEX_SNAPSHOTS.get() {
        if let Ok(mut entries) = cache.lock() {
            entries.remove(namespace);
        }
    }
}

pub fn alpha_count_options(filter: &Document, mut options: CountOptions) -> CountOptions {
    if filter.is_empty() {
        options.hint = Some(Hint::Name("_id_".to_string()));
    }
    options
}

pub async fn query_alpha_count(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &AlphaQuery,
) -> Result<u64, String> {
    if is_ra_query(params) {
        return Err("get_alpha_count does not support RA/RA_PARENT; use get_alpha_results for eligibility totals".to_string());
    }
    let db = client.database(&config.mongodb.databases.alpha);
    let collection = db.collection::<Document>(&alpha_collection_name(params));
    let filter = build_alpha_filter(params);
    let options = alpha_count_options(
        &filter,
        CountOptions::builder()
            .max_time(std::time::Duration::from_secs(30))
            .build(),
    );
    collection
        .count_documents(filter, options)
        .await
        .map_err(|e| format!("DB count error: {e}"))
}

async fn enrich_pnl_scores_and_submitted(
    db: &mongodb::Database,
    results: &mut [AlphaResult],
    ids: &[String],
) {
    if ids.is_empty() {
        return;
    }
    let pnl_coll = db.collection::<Document>("alpha_pnls");
    let sub_coll = db.collection::<Document>("alpha_submitted");
    let pnl_opts = FindOptions::builder()
        .projection(doc! { "_id": 0, "id": 1, "pnl_scores": 1 })
        .build();
    let sub_opts = FindOptions::builder()
        .projection(doc! { "_id": 0, "id": 1 })
        .build();

    let mut scores_by_id: HashMap<String, JsonValue> = HashMap::new();
    if let Ok(cursor) = pnl_coll.find(doc! { "id": { "$in": ids } }, pnl_opts).await {
        if let Ok(docs) = cursor.try_collect::<Vec<Document>>().await {
            for doc in docs {
                if let (Ok(id), Some(scores)) = (doc.get_str("id"), doc.get("pnl_scores")) {
                    if let Ok(val) = mongodb::bson::from_bson::<JsonValue>(scores.clone()) {
                        if !val.is_null() {
                            scores_by_id.insert(id.to_string(), val);
                        }
                    }
                }
            }
        }
    }

    let mut submitted_ids: HashSet<String> = HashSet::new();
    if let Ok(cursor) = sub_coll.find(doc! { "id": { "$in": ids } }, sub_opts).await {
        if let Ok(docs) = cursor.try_collect::<Vec<Document>>().await {
            for doc in docs {
                if let Ok(id) = doc.get_str("id") {
                    submitted_ids.insert(id.to_string());
                }
            }
        }
    }

    for result in results {
        result.submitted = Some(submitted_ids.contains(&result.id));
        if let Some(scores) = scores_by_id.get(&result.id) {
            result.pnl_scores = Some(scores.clone());
        }
        if let Some(children) = &mut result.children {
            for child in children {
                child.submitted = Some(submitted_ids.contains(&child.id));
                if let Some(scores) = scores_by_id.get(&child.id) {
                    child.pnl_scores = Some(scores.clone());
                }
            }
        }
    }
}

pub async fn query_alpha_results(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &AlphaQuery,
) -> Result<PagedResult<AlphaResult>, String> {
    let db = client.database(&config.mongodb.databases.alpha);

    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).max(1);
    let skip = ((page - 1) * page_size) as u64;
    let include_checks = params.include_checks.unwrap_or(false);

    let coll_name = alpha_collection_name(params);
    let collection = db.collection::<mongodb::bson::Document>(&coll_name);

    let is_ra_query = is_ra_query(params);
    let filter = build_alpha_filter(params);

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
        let find_options = FindOptions::builder()
            .projection(get_alpha_projection())
            .hint(hint)
            .sort(sort_doc)
            .skip(skip)
            .limit(page_size as i64)
            .build();

        let (total, cursor_res) = if should_include_total(params) {
            let (count_res, cursor_res) = join!(
                collection.count_documents(
                    filter.clone(),
                    alpha_count_options(&filter, CountOptions::default())
                ),
                collection.find(filter, find_options)
            );
            (count_res.unwrap_or(0), cursor_res)
        } else {
            (0, collection.find(filter, find_options).await)
        };

        let mut cursor = match cursor_res {
            Ok(cursor) => cursor,
            Err(e) => {
                // No automatic retry.
                evict_search_index_snapshot(&collection.namespace().to_string());
                return Err(format!("DB query error: {}", e));
            }
        };

        let mut documents = Vec::new();
        while let Some(document) = cursor.try_next().await.map_err(|e| e.to_string())? {
            documents.push(document);
        }
        (total, documents)
    };

    let mut results: Vec<AlphaResult> = documents
        .into_iter()
        .filter_map(|document| {
            parse_alpha_document_with_options(document, params.embedding.as_deref(), include_checks)
        })
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
            if let Some((child_result, fails)) = parse_ra_child_with_options(
                child_doc,
                params.embedding.as_deref(),
                params.messages_nin.as_deref(),
                include_checks,
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
        match corr_collection
            .find(prod_correlation_filter(ids.clone()), options)
            .await
        {
            Ok(cursor) => match cursor.try_collect::<Vec<Document>>().await {
                Ok(docs) => merge_prod_correlations(&mut results, docs),
                Err(err) => eprintln!("Prod correlation cursor error: {err}"),
            },
            Err(err) => eprintln!("Prod correlation query error: {err}"),
        }

        if params.include_pnl_scores.unwrap_or(false) {
            enrich_pnl_scores_and_submitted(&db, &mut results, &ids).await;
        }
    }

    Ok(PagedResult {
        data: results,
        total,
        page,
        page_size,
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PnlPoint {
    pub date: String,
    pub pnl: f64,
    pub risk_neutralized_pnl: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PnlChildSeries {
    pub id: String,
    pub name: String,
    pub pnl_series: Vec<PnlPoint>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PnlResponse {
    pub pnl_series: Vec<PnlPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_series: Option<Vec<PnlChildSeries>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pnl_scores: Option<JsonValue>,
}

pub fn parse_pnl_records(records: &mongodb::bson::Array) -> Vec<PnlPoint> {
    records
        .iter()
        .filter_map(|entry| {
            entry.as_array().and_then(|arr| {
                if arr.len() >= 2 {
                    let date = arr.get(0)?.as_str()?.to_string();
                    let pnl = arr
                        .get(1)?
                        .as_f64()
                        .or(arr.get(1)?.as_i32().map(|v| v as f64))?;
                    let risk_neutralized_pnl = if arr.len() > 2 {
                        arr.get(2)?
                            .as_f64()
                            .or(arr.get(2)?.as_i32().map(|v| v as f64))
                    } else {
                        None
                    };
                    Some(PnlPoint {
                        date,
                        pnl,
                        risk_neutralized_pnl,
                    })
                } else {
                    None
                }
            })
        })
        .collect()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PnlQuery {
    pub id: String,
    pub collection: Option<String>,
    #[serde(default)]
    pub output: Option<PathBuf>,
}

pub async fn query_pnl_by_id(
    client: &mongodb::Client,
    config: &AppConfig,
    query: &PnlQuery,
) -> Result<PnlResponse, String> {
    let db = client.database(&config.mongodb.databases.alpha);

    let filter = doc! { "id": &query.id };
    let pnls_coll = db.collection::<mongodb::bson::Document>("alpha_pnls");
    let source_coll_name = query
        .collection
        .as_deref()
        .and_then(sanitize_collection_name)
        .unwrap_or_else(|| "alpha_results".to_string());
    let source_coll = db.collection::<mongodb::bson::Document>(&source_coll_name);

    // 1. Check if this is an RA_PARENT with children
    if let Ok(Some(parent_doc)) = source_coll.find_one(filter.clone(), None).await {
        if let Ok(children_arr) = parent_doc.get_array("children") {
            let child_ids: Vec<String> = children_arr
                .iter()
                .filter_map(|b| b.as_str().map(|s| s.to_string()))
                .collect();

            if !child_ids.is_empty() {
                let mut child_region_map: HashMap<String, String> = HashMap::new();
                if let Ok(mut meta_cursor) = source_coll
                    .find(doc! { "id": { "$in": &child_ids } }, None)
                    .await
                {
                    while let Ok(Some(cdoc)) = meta_cursor.try_next().await {
                        if let Ok(cid) = cdoc.get_str("id") {
                            let region = cdoc
                                .get_document("settings")
                                .ok()
                                .and_then(|s| s.get_str("region").ok())
                                .unwrap_or("Unknown");
                            child_region_map.insert(cid.to_string(), region.to_string());
                        }
                    }
                }

                let mut children_series = Vec::new();
                let mut date_map: BTreeMap<String, (f64, f64, bool)> = BTreeMap::new();

                if let Ok(mut pnl_cursor) = pnls_coll
                    .find(doc! { "id": { "$in": &child_ids } }, None)
                    .await
                {
                    while let Ok(Some(cpnl_doc)) = pnl_cursor.try_next().await {
                        if let Ok(cid) = cpnl_doc.get_str("id") {
                            if let Ok(pnl_obj) = cpnl_doc.get_document("pnl") {
                                if let Ok(records) = pnl_obj.get_array("records") {
                                    let pts = parse_pnl_records(records);
                                    for pt in &pts {
                                        let entry = date_map
                                            .entry(pt.date.clone())
                                            .or_insert((0.0, 0.0, false));
                                        entry.0 += pt.pnl;
                                        if let Some(rn) = pt.risk_neutralized_pnl {
                                            entry.1 += rn;
                                            entry.2 = true;
                                        }
                                    }
                                    let region = child_region_map
                                        .get(cid)
                                        .cloned()
                                        .unwrap_or_else(|| "Sub".to_string());
                                    children_series.push(PnlChildSeries {
                                        id: cid.to_string(),
                                        name: format!("{} ({})", region, cid),
                                        pnl_series: pts,
                                    });
                                }
                            }
                        }
                    }
                }

                if !children_series.is_empty() {
                    let total_pnl_series: Vec<PnlPoint> = date_map
                        .into_iter()
                        .map(|(date, (pnl, rn_pnl, has_rn))| PnlPoint {
                            date,
                            pnl,
                            risk_neutralized_pnl: if has_rn { Some(rn_pnl) } else { None },
                        })
                        .collect();

                    return Ok(PnlResponse {
                        pnl_series: total_pnl_series,
                        children_series: Some(children_series),
                        pnl_scores: None,
                    });
                }
            }
        }
    }

    // 2. Normal single-alpha logic
    let mut doc_opt = pnls_coll.find_one(filter.clone(), None).await.ok().flatten();
    if doc_opt.is_none() && source_coll_name != "alpha_pnls" {
        doc_opt = source_coll.find_one(filter, None).await.ok().flatten();
    }

    let doc = doc_opt.ok_or_else(|| {
        "PNL document not found in alpha_pnls or source collection".to_string()
    })?;
    let pnl_scores = doc
        .get("pnl_scores")
        .and_then(|v| mongodb::bson::from_bson::<JsonValue>(v.clone()).ok())
        .filter(|v| !v.is_null());
    let pnl_obj = doc.get_document("pnl").map_err(|_| "No pnl field found")?;
    let records = pnl_obj
        .get_array("records")
        .map_err(|_| "No records array found")?;
    let pnl_series = parse_pnl_records(records);

    Ok(PnlResponse {
        pnl_series,
        children_series: None,
        pnl_scores,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorrResult {
    pub ppac_correlation: Option<String>,
    pub os_correlation: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchAlphaInAllCollectionsQuery {
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AlphaInCollectionResult {
    pub id: String,
    pub collection: String,
    pub region: String,
    pub code: Option<String>,
    pub sharpe: Option<f64>,
    pub fitness: Option<f64>,
    pub drawdown: Option<f64>,
    pub returns: Option<f64>,
    pub turnover: Option<f64>,
    pub margin: Option<f64>,
    pub date_created: Option<String>,
    pub date_submitted: Option<String>,
    pub os_sharpe: Option<f64>,
    pub os_fitness: Option<f64>,
    pub sub_universe_sharpe: Option<f64>,
    pub message: Option<String>,
    pub cluster_x: Option<f64>,
    pub cluster_y: Option<f64>,
    pub cluster_id: Option<serde_json::Value>,
}

pub async fn query_submission_stats(
    client: &mongodb::Client,
    config: &AppConfig,
    collection: Option<String>,
    query: Option<String>,
) -> Result<Vec<mongodb::bson::Document>, String> {
    let db = client.database(&config.mongodb.databases.alpha);

    let coll_name = collection
        .as_deref()
        .and_then(sanitize_collection_name)
        .unwrap_or_else(|| "alpha_results".to_string());
    let coll = db.collection::<mongodb::bson::Document>(&coll_name);

    let mut filters = vec![];
    if let Some(q) = query {
        if let Some(parsed_doc) = parse_query_to_bson(&q) {
            filters.push(parsed_doc);
        }
    }

    let filter = if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    };

    let pipeline = vec![
        doc! { "$match": filter },
        doc! {
            "$project": {
                "region": { "$ifNull": ["$settings.region", "Unknown"] },
                "delay": { "$ifNull": ["$settings.delay", 0] },
                "month": { "$substr": [{ "$ifNull": ["$dateSubmitted", "$dateCreated"] }, 0, 7] },
                "alpha_type": { "$ifNull": ["$type", "REGULAR"] },
                "sharpe": "$is.sharpe",
                "fitness": "$is.fitness",
                "drawdown": "$is.drawdown",
                "turnover": "$is.turnover",
                "returns": "$is.returns",
                "margin": "$is.margin",
            }
        },
        doc! {
            "$group": {
                "_id": { "month": "$month", "region": "$region", "delay": "$delay" },
                "count": {
                    "$sum": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, 1, 0] }
                },
                "super_count": {
                    "$sum": { "$cond": [{ "$eq": ["$alpha_type", "SUPER"] }, 1, 0] }
                },
                "avg_sharpe": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$sharpe", "$$REMOVE"] }
                },
                "avg_fitness": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$fitness", "$$REMOVE"] }
                },
                "avg_drawdown": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$drawdown", "$$REMOVE"] }
                },
                "avg_turnover": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$turnover", "$$REMOVE"] }
                },
                "avg_returns": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$returns", "$$REMOVE"] }
                },
                "avg_margin": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$margin", "$$REMOVE"] }
                },
            }
        },
        doc! {
            "$sort": { "_id.month": -1, "_id.region": 1, "_id.delay": 1 }
        },
    ];

    let mut cursor = coll
        .aggregate(pipeline, None)
        .await
        .map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        results.push(doc);
    }
    Ok(results)
}

pub async fn query_alpha_in_all_collections(
    client: &mongodb::Client,
    config: &AppConfig,
    query: &SearchAlphaInAllCollectionsQuery,
) -> Result<Option<AlphaInCollectionResult>, String> {
    let db = client.database(&config.mongodb.databases.alpha);
    let collection_names = db
        .list_collection_names(None)
        .await
        .map_err(|e| format!("Failed to list collections: {}", e))?;

    for coll_name in collection_names {
        if coll_name.starts_with("system.") {
            continue;
        }
        let collection = db.collection::<mongodb::bson::Document>(&coll_name);
        let filter = doc! { "id": &query.id };
        if let Ok(Some(doc)) = collection
            .find_one(
                filter,
                FindOneOptions::builder()
                    .projection(get_alpha_projection())
                    .build(),
            )
            .await
        {
            if let Some(result) = parse_alpha_document(doc, None) {
                return Ok(Some(AlphaInCollectionResult {
                    id: result.id,
                    collection: coll_name,
                    region: result.region,
                    code: result.code,
                    sharpe: result.sharpe,
                    fitness: result.fitness,
                    drawdown: result.drawdown,
                    returns: result.returns,
                    turnover: result.turnover,
                    margin: result.margin,
                    date_created: result.date_created,
                    date_submitted: result.date_submitted,
                    os_sharpe: result.os_sharpe,
                    os_fitness: result.os_fitness,
                    sub_universe_sharpe: result.sub_universe_sharpe,
                    message: result.message,
                    cluster_x: result.cluster_x,
                    cluster_y: result.cluster_y,
                    cluster_id: result.cluster_id,
                }));
            }
        }
    }
    Ok(None)
}

fn write_optional_output(output: Option<&Path>, payload: &JsonValue) -> Result<(), String> {
    let Some(path) = output else {
        return Ok(());
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create output directory: {e}"))?;
    }
    let bytes =
        serde_json::to_vec_pretty(payload).map_err(|e| format!("JSON encode error: {e}"))?;
    fs::write(path, bytes).map_err(|e| format!("failed to write output file {}: {e}", path.display()))
}

fn command_error(command: &str, error: String) -> TaskOutcome {
    TaskOutcome {
        value: json!({
            "schemaVersion": 1,
            "command": command,
            "ok": false,
            "errorType": "RuntimeError",
            "error": error,
        }),
        exit_code: 1,
    }
}

pub async fn cmd_alphas(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &AlphaQuery,
) -> TaskOutcome {
    match query_alpha_results(client, config, params).await {
        Ok(paged) => {
            let mut value = json!({
                "schemaVersion": 1,
                "command": "alphas",
                "ok": true,
                "collection": alpha_collection_name(params),
                "total": paged.total,
                "page": paged.page,
                "pageSize": paged.page_size,
                "count": paged.data.len(),
                "data": paged.data,
            });
            if let Some(out) = &params.output {
                value["output"] = json!(out.display().to_string());
                if let Err(e) = write_optional_output(Some(out), &value) {
                    return command_error("alphas", e);
                }
            }
            TaskOutcome {
                value,
                exit_code: 0,
            }
        }
        Err(e) => command_error("alphas", e),
    }
}

pub async fn cmd_alpha_pnl(
    client: &mongodb::Client,
    config: &AppConfig,
    query: &PnlQuery,
) -> TaskOutcome {
    match query_pnl_by_id(client, config, query).await {
        Ok(resp) => {
            let mut value = json!({
                "schemaVersion": 1,
                "command": "alpha-pnl",
                "ok": true,
                "id": query.id,
                "points": resp.pnl_series.len(),
                "pnlSeries": resp.pnl_series,
                "childrenSeries": resp.children_series,
                "pnlScores": resp.pnl_scores,
            });
            if let Some(out) = &query.output {
                value["output"] = json!(out.display().to_string());
                if let Err(e) = write_optional_output(Some(out), &value) {
                    return command_error("alpha-pnl", e);
                }
            }
            TaskOutcome {
                value,
                exit_code: 0,
            }
        }
        Err(e) => command_error("alpha-pnl", e),
    }
}

pub async fn cmd_datasets(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &DatasetQuery,
) -> TaskOutcome {
    match query_datasets(client, config, params).await {
        Ok(paged) => {
            let mut value = json!({
                "schemaVersion": 1,
                "command": "datasets",
                "ok": true,
                "total": paged.total,
                "page": paged.page,
                "pageSize": paged.page_size,
                "count": paged.data.len(),
                "data": paged.data,
            });
            if let Some(out) = &params.output {
                value["output"] = json!(out.display().to_string());
                if let Err(e) = write_optional_output(Some(out), &value) {
                    return command_error("datasets", e);
                }
            }
            TaskOutcome {
                value,
                exit_code: 0,
            }
        }
        Err(e) => command_error("datasets", e),
    }
}

pub async fn cmd_datafields(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &DatafieldQuery,
) -> TaskOutcome {
    match query_datafields(client, config, params).await {
        Ok(paged) => {
            let mut value = json!({
                "schemaVersion": 1,
                "command": "datafields",
                "ok": true,
                "datasetId": if params.dataset_id.is_empty() { JsonValue::Null } else { json!(params.dataset_id) },
                "total": paged.total,
                "page": paged.page,
                "pageSize": paged.page_size,
                "count": paged.data.len(),
                "data": paged.data,
            });
            if let Some(out) = &params.output {
                value["output"] = json!(out.display().to_string());
                if let Err(e) = write_optional_output(Some(out), &value) {
                    return command_error("datafields", e);
                }
            }
            TaskOutcome {
                value,
                exit_code: 0,
            }
        }
        Err(e) => command_error("datafields", e),
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SubmissionStatsQuery {
    pub collection: Option<String>,
    pub query: Option<String>,
    pub region: Option<String>,
    pub delay: Option<i32>,
    pub since: Option<String>,
    pub window_months: Option<u32>,
    pub recent_days: Option<u32>,
    #[serde(default)]
    pub output: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmissionRegionDelayRow {
    pub month: String,
    pub region: String,
    pub delay: i32,
    pub count: i64,
    pub super_count: i64,
    pub avg_sharpe: Option<f64>,
    pub avg_fitness: Option<f64>,
    pub avg_drawdown: Option<f64>,
    pub avg_turnover: Option<f64>,
    pub avg_returns: Option<f64>,
    pub avg_margin: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmissionSummaryRow {
    pub month: String,
    pub count: i64,
    pub super_count: i64,
    pub avg_sharpe: Option<f64>,
    pub avg_fitness: Option<f64>,
    pub avg_drawdown: Option<f64>,
    pub avg_turnover: Option<f64>,
    pub avg_returns: Option<f64>,
    pub avg_margin: Option<f64>,
    pub children: Vec<SubmissionRegionDelayRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OwnSuperRegionRow {
    pub region: String,
    pub own_super_count: usize,
    pub other_super_count: usize,
    pub total_super_count: usize,
    pub target: usize,
    pub gap_to_target: usize,
    pub latest_submitted_date: Option<String>,
    pub own_super_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailySubmissionRow {
    pub date: String,
    pub total_count: usize,
    pub regular_count: usize,
    pub super_count: usize,
    pub own_super_count: usize,
    pub glb_count: usize,
    pub glb_atom_count: usize,
    pub glb_hard_gate_pass_count: usize,
    pub alpha_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PyramidSummaryRow {
    pub name: String,
    pub count: usize,
    pub gap_to_three: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubmissionStatsReport {
    pub collection: String,
    pub selected_window_months: Vec<String>,
    pub all_time_summary: Option<SubmissionSummaryRow>,
    pub window_summary: Option<SubmissionSummaryRow>,
    pub monthly: Vec<SubmissionSummaryRow>,
    pub own_super_by_region: Vec<OwnSuperRegionRow>,
    pub recent_daily: Vec<DailySubmissionRow>,
    pub pyramid_summary: JsonValue,
}

fn doc_f64(doc: &Document, key: &str) -> Option<f64> {
    doc.get_f64(key)
        .ok()
        .or_else(|| doc.get_i32(key).ok().map(|v| v as f64))
        .or_else(|| doc.get_i64(key).ok().map(|v| v as f64))
}

fn doc_i64(doc: &Document, key: &str) -> i64 {
    doc.get_i64(key)
        .ok()
        .or_else(|| doc.get_i32(key).ok().map(i64::from))
        .unwrap_or(0)
}

pub fn calculate_grouped_submission_summary(
    docs: &[Document],
    label: &str,
) -> Option<SubmissionSummaryRow> {
    if docs.is_empty() {
        return None;
    }
    #[derive(Default)]
    struct Acc {
        region: String,
        delay: i32,
        count: i64,
        super_count: i64,
        w_sharpe: f64,
        w_fitness: f64,
        w_drawdown: f64,
        w_turnover: f64,
        w_returns: f64,
        w_margin: f64,
    }
    let mut groups: BTreeMap<(String, i32), Acc> = BTreeMap::new();
    let mut total_count = 0i64;
    let mut total_super_count = 0i64;
    let mut total_w_sharpe = 0.0;
    let mut total_w_fitness = 0.0;
    let mut total_w_drawdown = 0.0;
    let mut total_w_turnover = 0.0;
    let mut total_w_returns = 0.0;
    let mut total_w_margin = 0.0;

    for doc in docs {
        let Ok(id_doc) = doc.get_document("_id") else {
            continue;
        };
        let region = id_doc.get_str("region").unwrap_or("Unknown").to_string();
        let delay = id_doc
            .get_i32("delay")
            .ok()
            .or_else(|| id_doc.get_i64("delay").ok().map(|v| v as i32))
            .unwrap_or(0);
        let c = doc_i64(doc, "count");
        let sc = doc_i64(doc, "super_count");
        let wf = c as f64;
        let sharpe = doc_f64(doc, "avg_sharpe").unwrap_or(0.0);
        let fitness = doc_f64(doc, "avg_fitness").unwrap_or(0.0);
        let drawdown = doc_f64(doc, "avg_drawdown").unwrap_or(0.0);
        let turnover = doc_f64(doc, "avg_turnover").unwrap_or(0.0);
        let returns = doc_f64(doc, "avg_returns").unwrap_or(0.0);
        let margin = doc_f64(doc, "avg_margin").unwrap_or(0.0);

        let entry = groups.entry((region.clone(), delay)).or_insert_with(|| Acc {
            region,
            delay,
            ..Default::default()
        });
        entry.count += c;
        entry.super_count += sc;
        entry.w_sharpe += sharpe * wf;
        entry.w_fitness += fitness * wf;
        entry.w_drawdown += drawdown * wf;
        entry.w_turnover += turnover * wf;
        entry.w_returns += returns * wf;
        entry.w_margin += margin * wf;

        total_count += c;
        total_super_count += sc;
        total_w_sharpe += sharpe * wf;
        total_w_fitness += fitness * wf;
        total_w_drawdown += drawdown * wf;
        total_w_turnover += turnover * wf;
        total_w_returns += returns * wf;
        total_w_margin += margin * wf;
    }

    if total_count == 0 && total_super_count == 0 {
        return None;
    }

    let children = groups
        .into_values()
        .map(|g| {
            let denom = if g.count > 0 { Some(g.count as f64) } else { None };
            SubmissionRegionDelayRow {
                month: label.to_string(),
                region: g.region,
                delay: g.delay,
                count: g.count,
                super_count: g.super_count,
                avg_sharpe: denom.map(|d| g.w_sharpe / d),
                avg_fitness: denom.map(|d| g.w_fitness / d),
                avg_drawdown: denom.map(|d| g.w_drawdown / d),
                avg_turnover: denom.map(|d| g.w_turnover / d),
                avg_returns: denom.map(|d| g.w_returns / d),
                avg_margin: denom.map(|d| g.w_margin / d),
            }
        })
        .collect();

    let denom = if total_count > 0 {
        Some(total_count as f64)
    } else {
        None
    };
    Some(SubmissionSummaryRow {
        month: label.to_string(),
        count: total_count,
        super_count: total_super_count,
        avg_sharpe: denom.map(|d| total_w_sharpe / d),
        avg_fitness: denom.map(|d| total_w_fitness / d),
        avg_drawdown: denom.map(|d| total_w_drawdown / d),
        avg_turnover: denom.map(|d| total_w_turnover / d),
        avg_returns: denom.map(|d| total_w_returns / d),
        avg_margin: denom.map(|d| total_w_margin / d),
        children,
    })
}

fn is_own_super_selection(selection_code: &str) -> bool {
    let lower = selection_code.to_ascii_lowercase();
    if lower.contains("!own") || lower.contains("not(own)") || lower.contains("not (own)") {
        return false;
    }
    lower.contains("own")
}

pub async fn query_submission_stats_report(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &SubmissionStatsQuery,
) -> Result<SubmissionStatsReport, String> {
    let coll_name = params
        .collection
        .as_deref()
        .and_then(sanitize_collection_name)
        .unwrap_or_else(|| "alpha_submitted".to_string());
    let db = client.database(&config.mongodb.databases.alpha);
    let coll = db.collection::<Document>(&coll_name);

    let mut filters = Vec::new();
    if let Some(q) = params.query.as_deref().filter(|s| !s.trim().is_empty()) {
        if let Some(parsed_doc) = parse_query_to_bson(q) {
            filters.push(parsed_doc);
        }
    }
    if let Some(region) = params.region.as_deref().filter(|s| !s.trim().is_empty()) {
        filters.push(doc! { "settings.region": region });
    }
    if let Some(delay) = params.delay {
        filters.push(doc! { "settings.delay": delay });
    }
    if let Some(since) = params.since.as_deref().filter(|s| !s.trim().is_empty()) {
        filters.push(doc! {
            "$or": [
                { "dateSubmitted": { "$gte": since } },
                { "dateCreated": { "$gte": since } },
            ]
        });
    }

    let filter = if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    };

    let pipeline = vec![
        doc! { "$match": filter.clone() },
        doc! {
            "$project": {
                "region": { "$ifNull": ["$settings.region", "Unknown"] },
                "delay": { "$ifNull": ["$settings.delay", 0] },
                "month": { "$substr": [{ "$ifNull": ["$dateSubmitted", "$dateCreated"] }, 0, 7] },
                "alpha_type": { "$ifNull": ["$type", "REGULAR"] },
                "sharpe": "$is.sharpe",
                "fitness": "$is.fitness",
                "drawdown": "$is.drawdown",
                "turnover": "$is.turnover",
                "returns": "$is.returns",
                "margin": "$is.margin",
            }
        },
        doc! {
            "$group": {
                "_id": { "month": "$month", "region": "$region", "delay": "$delay" },
                "count": {
                    "$sum": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, 1, 0] }
                },
                "super_count": {
                    "$sum": { "$cond": [{ "$eq": ["$alpha_type", "SUPER"] }, 1, 0] }
                },
                "avg_sharpe": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$sharpe", "$$REMOVE"] }
                },
                "avg_fitness": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$fitness", "$$REMOVE"] }
                },
                "avg_drawdown": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$drawdown", "$$REMOVE"] }
                },
                "avg_turnover": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$turnover", "$$REMOVE"] }
                },
                "avg_returns": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$returns", "$$REMOVE"] }
                },
                "avg_margin": {
                    "$avg": { "$cond": [{ "$eq": ["$alpha_type", "REGULAR"] }, "$margin", "$$REMOVE"] }
                },
            }
        },
        doc! {
            "$sort": { "_id.month": -1, "_id.region": 1, "_id.delay": 1 }
        },
    ];

    let detail_opts = FindOptions::builder()
        .projection(doc! {
            "_id": 0,
            "id": 1,
            "type": 1,
            "settings.region": 1,
            "settings.delay": 1,
            "dateSubmitted": 1,
            "dateCreated": 1,
            "selection.code": 1,
            "is.turnover": 1,
            "is.margin": 1,
            "classifications": 1,
            "is.checks": 1,
        })
        .sort(doc! { "dateSubmitted": -1, "dateCreated": -1 })
        .limit(5000)
        .build();

    let (agg_res, detail_res) = join!(
        coll.aggregate(pipeline, None),
        coll.find(filter, detail_opts)
    );

    let mut agg_cursor = agg_res.map_err(|e| format!("submission-stats aggregate error: {e}"))?;
    let mut raw_docs = Vec::new();
    while let Some(doc) = agg_cursor.try_next().await.map_err(|e| e.to_string())? {
        raw_docs.push(doc);
    }

    let mut by_month: BTreeMap<String, Vec<Document>> = BTreeMap::new();
    for doc in &raw_docs {
        if let Ok(id_doc) = doc.get_document("_id") {
            let month = id_doc.get_str("month").unwrap_or("Unknown").to_string();
            by_month.entry(month).or_default().push(doc.clone());
        }
    }
    let mut unique_months: Vec<String> = by_month.keys().cloned().collect();
    unique_months.sort_by(|a, b| b.cmp(a));

    let window_months = params.window_months.unwrap_or(3).max(1) as usize;
    let selected_window_months: Vec<String> =
        unique_months.iter().take(window_months).cloned().collect();
    let window_docs: Vec<Document> = raw_docs
        .iter()
        .filter(|d| {
            d.get_document("_id")
                .ok()
                .and_then(|id| id.get_str("month").ok())
                .is_some_and(|m| selected_window_months.iter().any(|wm| wm == m))
        })
        .cloned()
        .collect();

    let all_time_summary = calculate_grouped_submission_summary(&raw_docs, "全部时间");
    let window_summary = calculate_grouped_submission_summary(
        &window_docs,
        &format!("最近{}个月", window_months),
    );

    let monthly: Vec<SubmissionSummaryRow> = unique_months
        .iter()
        .filter_map(|m| {
            by_month
                .get(m)
                .and_then(|docs| calculate_grouped_submission_summary(docs, m))
        })
        .collect();

    // Coordinator inventory & daily/pyramid aggregation
    let mut detail_cursor = detail_res.map_err(|e| format!("submission-stats detail error: {e}"))?;
    let mut detail_docs = Vec::new();
    while let Some(doc) = detail_cursor.try_next().await.map_err(|e| e.to_string())? {
        detail_docs.push(doc);
    }

    let priority_regions = [
        "EUR", "ASI", "JPN", "GLB", "USA", "CHN", "KOR", "TWN", "HKG", "AMR",
    ];
    #[derive(Default)]
    struct RegionSuperAcc {
        own_super_count: usize,
        other_super_count: usize,
        latest_submitted_date: Option<String>,
        own_super_ids: Vec<String>,
    }
    let mut super_by_region: HashMap<String, RegionSuperAcc> = HashMap::new();
    for &r in &priority_regions {
        super_by_region.insert(r.to_string(), RegionSuperAcc::default());
    }

    let mut daily_map: BTreeMap<String, DailySubmissionRow> = BTreeMap::new();
    let mut pyramid_counts: BTreeMap<String, usize> = BTreeMap::new();

    for doc in &detail_docs {
        let id = doc.get_str("id").unwrap_or_default().to_string();
        let alpha_type = doc.get_str("type").unwrap_or("REGULAR");
        let region = doc
            .get_document("settings")
            .ok()
            .and_then(|s| s.get_str("region").ok())
            .unwrap_or("Unknown")
            .to_string();
        let date_raw = doc
            .get_str("dateSubmitted")
            .ok()
            .or_else(|| doc.get_str("dateCreated").ok())
            .unwrap_or("");
        let day = if date_raw.len() >= 10 {
            date_raw[..10].to_string()
        } else {
            "Unknown".to_string()
        };

        let sel_code = doc
            .get_document("selection")
            .ok()
            .and_then(|s| s.get_str("code").ok())
            .unwrap_or("");
        let is_own_super = alpha_type == "SUPER" && is_own_super_selection(sel_code);

        if alpha_type == "SUPER" {
            let entry = super_by_region.entry(region.clone()).or_default();
            if is_own_super {
                entry.own_super_count += 1;
                entry.own_super_ids.push(id.clone());
                if !date_raw.is_empty()
                    && entry
                        .latest_submitted_date
                        .as_deref()
                        .is_none_or(|prev| date_raw > prev)
                {
                    entry.latest_submitted_date = Some(date_raw.to_string());
                }
            } else {
                entry.other_super_count += 1;
            }
        }

        let is_atom = doc
            .get_array("classifications")
            .ok()
            .is_some_and(|arr| {
                arr.iter().any(|item| {
                    item.as_document()
                        .and_then(|d| d.get_str("id").ok())
                        .is_some_and(|cid| cid == "DATA_USAGE:SINGLE_DATA_SET")
                })
            });

        let is_doc = doc.get_document("is").ok();
        let turnover = is_doc.and_then(|d| doc_f64(d, "turnover"));
        let margin = is_doc.and_then(|d| doc_f64(d, "margin"));
        let glb_hard_gate_pass = region == "GLB"
            && turnover.is_some_and(|t| t <= 0.20)
            && margin.is_some_and(|m| m >= 0.001);

        if day != "Unknown" {
            let d_entry = daily_map.entry(day.clone()).or_insert_with(|| DailySubmissionRow {
                date: day,
                total_count: 0,
                regular_count: 0,
                super_count: 0,
                own_super_count: 0,
                glb_count: 0,
                glb_atom_count: 0,
                glb_hard_gate_pass_count: 0,
                alpha_ids: Vec::new(),
            });
            d_entry.total_count += 1;
            if alpha_type == "SUPER" {
                d_entry.super_count += 1;
                if is_own_super {
                    d_entry.own_super_count += 1;
                }
            } else {
                d_entry.regular_count += 1;
            }
            if region == "GLB" {
                d_entry.glb_count += 1;
                if is_atom {
                    d_entry.glb_atom_count += 1;
                }
                if glb_hard_gate_pass {
                    d_entry.glb_hard_gate_pass_count += 1;
                }
            }
            d_entry.alpha_ids.push(id);
        }

        if let Some(checks) = is_doc.and_then(|d| d.get_array("checks").ok()) {
            for check_bson in checks {
                let Some(cdoc) = check_bson.as_document() else {
                    continue;
                };
                if cdoc.get_str("name").ok() == Some("MATCHES_PYRAMID") {
                    if let Ok(pyramids) = cdoc.get_array("pyramids") {
                        for p in pyramids {
                            if let Some(pname) = p
                                .as_document()
                                .and_then(|pd| pd.get_str("name").ok())
                                .or_else(|| p.as_str())
                            {
                                *pyramid_counts.entry(pname.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut ordered_regions: Vec<String> = priority_regions.iter().map(|s| (*s).to_string()).collect();
    for r in super_by_region.keys() {
        if !ordered_regions.contains(r) {
            ordered_regions.push(r.clone());
        }
    }
    let own_super_by_region: Vec<OwnSuperRegionRow> = ordered_regions
        .into_iter()
        .filter_map(|region| {
            let acc = super_by_region.remove(&region)?;
            let total = acc.own_super_count + acc.other_super_count;
            Some(OwnSuperRegionRow {
                region,
                own_super_count: acc.own_super_count,
                other_super_count: acc.other_super_count,
                total_super_count: total,
                target: 6,
                gap_to_target: 6usize.saturating_sub(acc.own_super_count),
                latest_submitted_date: acc.latest_submitted_date,
                own_super_ids: acc.own_super_ids,
            })
        })
        .collect();

    let recent_days = params.recent_days.unwrap_or(14).max(1) as usize;
    let recent_daily: Vec<DailySubmissionRow> = daily_map
        .into_values()
        .rev()
        .take(recent_days)
        .collect();

    let mut by_pyramid: Vec<PyramidSummaryRow> = pyramid_counts
        .into_iter()
        .map(|(name, count)| PyramidSummaryRow {
            name,
            count,
            gap_to_three: 3usize.saturating_sub(count),
        })
        .collect();
    by_pyramid.sort_by(|a, b| a.gap_to_three.cmp(&b.gap_to_three).then_with(|| a.name.cmp(&b.name)));
    let qualified_ge_3 = by_pyramid.iter().filter(|p| p.count >= 3).count();
    let gap_1_count = by_pyramid.iter().filter(|p| p.count == 2).count();
    let gap_2_count = by_pyramid.iter().filter(|p| p.count == 1).count();

    let pyramid_summary = json!({
        "uniquePyramidsCount": by_pyramid.len(),
        "qualifiedGe3Count": qualified_ge_3,
        "gap1CountEq2": gap_1_count,
        "gap2CountEq1": gap_2_count,
        "byPyramid": by_pyramid,
    });

    Ok(SubmissionStatsReport {
        collection: coll_name,
        selected_window_months,
        all_time_summary,
        window_summary,
        monthly,
        own_super_by_region,
        recent_daily,
        pyramid_summary,
    })
}

pub async fn cmd_submission_stats(
    client: &mongodb::Client,
    config: &AppConfig,
    params: &SubmissionStatsQuery,
) -> TaskOutcome {
    match query_submission_stats_report(client, config, params).await {
        Ok(report) => {
            let mut value = json!({
                "schemaVersion": 1,
                "command": "submission-stats",
                "ok": true,
                "collection": report.collection,
                "selectedWindowMonths": report.selected_window_months,
                "allTimeSummary": report.all_time_summary,
                "windowSummary": report.window_summary,
                "monthly": report.monthly,
                "ownSuperByRegion": report.own_super_by_region,
                "recentDaily": report.recent_daily,
                "pyramidSummary": report.pyramid_summary,
            });
            if let Some(out) = &params.output {
                value["output"] = json!(out.display().to_string());
                if let Err(e) = write_optional_output(Some(out), &value) {
                    return command_error("submission-stats", e);
                }
            }
            TaskOutcome {
                value,
                exit_code: 0,
            }
        }
        Err(e) => command_error("submission-stats", e),
    }
}
