use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use futures_util::stream::TryStreamExt;
use futures::join;

use mongodb::{
    bson::{doc, Document},
    options::{FindOptions, FindOneOptions},
};
use serde::{Deserialize, Serialize, Deserializer};
use tauri::{command, State};

use crate::mongo_manager::MongoClients;
use crate::config::{AppConfig, run_python_command};

use serde_json::Value as JsonValue;

/// 自定义反序列化器，用于处理可能是字符串（如 "RAM"）或数字的 Option<f64> 字段
fn deserialize_option_f64_flexible<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    // 尝试反序列化为 serde_json::Value 以处理多种可能的类型
    let v: serde_json::Value = serde::Deserialize::deserialize(deserializer).unwrap_or(serde_json::Value::Null);
    match v {
        serde_json::Value::Number(n) => Ok(n.as_f64()),
        serde_json::Value::String(s) => {
            // 尝试将字符串解析为 f64，如果失败（如 "RAM"）则返回 None
            Ok(s.parse::<f64>().ok())
        }
        _ => Ok(None),
    }
}



#[derive(Serialize, Deserialize, Debug)]
pub struct AlphaResult {
    pub id: String,  // 原来是 ObjectId，改为 String
    pub region: String,
    pub settings: Option<serde_json::Value>,
    pub code: Option<String>,
    pub sharpe: Option<f64>,
    pub fitness: Option<f64>,
    pub drawdown: Option<f64>,
    pub returns: Option<f64>,            // ✅ 新增字段
    pub turnover: Option<f64>,
    pub margin: Option<f64>,
    pub long_count: Option<i32>,
    pub short_count: Option<i32>,
    pub sub_universe_sharpe: Option<f64>,
    pub message: Option<String>,
    pub date_created: Option<String>,    // ✅ 新增字段，使用字符串存时间戳
    pub date_submitted: Option<String>,  // ✅ 新增提交日期
    pub pnl_score: Option<f64>,   // ✅ 新增
    pub os_sharpe: Option<f64>,
    pub os_fitness: Option<f64>,
    pub cluster_x: Option<f64>,
    pub cluster_y: Option<f64>,
    pub cluster_id: Option<serde_json::Value>,
    pub classifications: Option<Vec<serde_json::Value>>,
    #[serde(rename = "currentProdCorrelation")]
    pub current_prod_correlation: Option<serde_json::Value>,
    pub self_category: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]  // 前端传字符串时自动匹配
pub enum SortField {
    DateCreated,
    DateSubmitted,
    Sharpe,
    Returns,
    Turnover,
    Fitness,
    Margin,
    PnlScore,
}

#[derive(Deserialize)]
pub struct AlphaQuery {
    pub query: Option<String>,
    pub exclude_query: Option<String>, // New field for excluding certain alpha code patterns
    pub id: Option<String>,
    pub messages_in: Option<Vec<String>>, // Renamed from messages
    pub messages_nin: Option<Vec<String>>, // Added for exclusion
    pub classifications_in: Option<Vec<String>>, // Added for classifications
    pub region: Option<String>,
    pub days_within: Option<u32>,
    pub min_turnover: Option<f64>,
    pub max_turnover: Option<f64>,
    pub min_margin: Option<f64>,
    pub delay: Option<u32>,
    pub min_returns: Option<f64>,
    pub alpha_type: Option<String>,
    pub self_categories: Option<Vec<String>>,
    pub collection: Option<String>,
    pub embedding: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub sort_field: Option<SortField>,
    pub sort_order: Option<i32>,
}

fn sanitize_collection_name(name: &str) -> Option<String> {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(name.to_string())
    } else {
        None
    }
}

#[derive(Serialize)]
pub struct PagedResult<T> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbPyramid {
    pub name: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DbCheck {
    pub name: Option<String>,
    pub result: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub limit: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_f64_flexible")]
    pub value: Option<f64>,
    pub message: Option<String>,
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
    pub cluster: Option<serde_json::Value>, // Has an 'id' inside
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
}

impl DbAlphaDocument {
    pub fn into_alpha_result(self, embedding_key: Option<&str>) -> Option<AlphaResult> {
        let id = self.id?;
        let r#type = self.r#type.unwrap_or_else(|| "UNKNOWN".to_string());
        
        let settings_val = self.settings.unwrap_or_else(|| serde_json::json!({}));
        let region = settings_val.get("region").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
        
        let code = match r#type.as_str() {
            "REGULAR" => self.regular.and_then(|r| r.code),
            "SUPER" => {
                let selection_code = self.selection.and_then(|r| r.code).unwrap_or_default();
                let combo_code = self.combo.and_then(|r| r.code).unwrap_or_default();
                Some(format!("{}\n{}", selection_code, combo_code))
            }
            _ => None,
        };

        let mut sub_universe_sharpe = None;
        let mut message = None;
        let mut pyramid_names = Vec::new();

        if let Some(is_metrics) = &self.is {
            if let Some(checks) = &is_metrics.checks {
                let mut msg_parts = Vec::new();
                for check in checks {
                    let name = check.name.as_deref().unwrap_or("");
                    let result = check.result.as_deref().unwrap_or("");
                    
                    if name == "MATCHES_PYRAMID" {
                        if let Some(pyramids) = &check.pyramids {
                            for p in pyramids {
                                if let Some(pname) = &p.name {
                                    // Extract the last part after the last '/'
                                    let short_name = pname.split('/').last().unwrap_or(pname).to_string();
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
            arr.into_iter().filter_map(|c| {
                if let (Some(cid), Some(cname)) = (c.id, c.name) {
                    Some(serde_json::json!({ "id": cid, "name": cname }))
                } else {
                    None
                }
            }).collect()
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
            self_category: if !pyramid_names.is_empty() { Some(pyramid_names) } else { None },
        })
    }
}

fn parse_alpha_document(doc: mongodb::bson::Document, embedding_key: Option<&str>) -> Option<AlphaResult> {
    match mongodb::bson::from_document::<DbAlphaDocument>(doc) {
        Ok(db_alpha) => db_alpha.into_alpha_result(embedding_key),
        Err(e) => {
            println!("Error deserializing alpha document: {}", e);
            None
        }
    }
}


use once_cell::sync::Lazy;
use regex::Regex;

static RE_TRUE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bTrue\b").unwrap());
static RE_FALSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bFalse\b").unwrap());
static RE_NONE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bNone\b").unwrap());
static RE_INT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(?:int|NumberInt)\((\d+)\)").unwrap());
static RE_FLOAT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(?:float|NumberDecimal)\(([\d\.]+)\)").unwrap());
static RE_KEYS: Lazy<Regex> = Lazy::new(|| Regex::new(r"([{,\[]\s*)([\$a-zA-Z_][\$a-zA-Z0-9_\.]*)\s*:").unwrap());
static RE_COMMA: Lazy<Regex> = Lazy::new(|| Regex::new(r",\s*([\]}])").unwrap());

fn parse_query_to_bson(q: &str) -> Option<mongodb::bson::Document> {
    let q_trimmed = q.trim();
    if q_trimmed.is_empty() {
        return None;
    }

    println!("Incoming query: {}", q_trimmed);
    
    // 1. Normalize smart quotes (common on macOS) and Python-style values
    let mut normalized_q = q_trimmed
        .replace('“', "\"")
        .replace('”', "\"")
        .replace('‘', "'")
        .replace('’', "'");
    
    normalized_q = RE_TRUE.replace_all(&normalized_q, "true").to_string();
    normalized_q = RE_FALSE.replace_all(&normalized_q, "false").to_string();
    normalized_q = RE_NONE.replace_all(&normalized_q, "null").to_string();
    normalized_q = RE_INT.replace_all(&normalized_q, "$1").to_string();
    normalized_q = RE_FLOAT.replace_all(&normalized_q, "$1").to_string();

    // 2. Smart wrap: if it looks like an object (contains :) but lacks braces, wrap it
    if !normalized_q.starts_with('{') && normalized_q.contains(':') {
        normalized_q = format!("{{{}}}", normalized_q);
    }

    // 3. Automatically quote unquoted keys (e.g. $or -> "$or", os.sharpe -> "os.sharpe")
    normalized_q = RE_KEYS.replace_all(&normalized_q, "$1\"$2\":").to_string();
    normalized_q = normalized_q.replace("\"\"", "\"");

    println!("Normalized query: {}", normalized_q);

    // 4. Robust parsing
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&normalized_q) {
        if let Ok(bson_val) = mongodb::bson::to_bson(&json_val) {
            if let Some(parsed_doc) = bson_val.as_document() {
                return Some(parsed_doc.clone());
            }
        }
    }
    
    // 5. Fallback: Clean trailing commas
    let cleaned_q = RE_COMMA.replace_all(&normalized_q, "$1").to_string();
    
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&cleaned_q) {
        if let Ok(bson_val) = mongodb::bson::to_bson(&json_val) {
            if let Some(parsed_doc) = bson_val.as_document() {
                return Some(parsed_doc.clone());
            }
        }
    }

    None
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Deserialize)]
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
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Deserialize)]
pub struct DatafieldQuery {
    pub dataset_id: String,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub search: Option<String>,
    pub region: Option<String>,
    pub universe: Option<String>,
    pub delay: Option<i32>,
    pub sort_field: Option<String>,
    pub sort_order: Option<i32>,
}

#[command]
pub async fn get_datafields(
    params: DatafieldQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PagedResult<Datafield>, String> {
    let client = &clients.local;
    let db_name = config.mongodb.databases.datafield.as_deref().unwrap_or("data_db");
    let coll_name = config.mongodb.databases.datafield_collection.as_deref().unwrap_or("datafields_all");
    
    let db = client.database(db_name);
    let collection = db.collection::<mongodb::bson::Document>(coll_name);

    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(50);
    let skip = ((page - 1) * page_size) as u64;

    let mut filters = vec![
        doc! { "dataset.id": &params.dataset_id }
    ];
    
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

    // JOINT data-level filters
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

    let filter = doc! { "$and": filters };

    let sort_field = params.sort_field.as_deref().unwrap_or("id");
    let sort_order = params.sort_order.unwrap_or(1);

    let mut add_fields = doc! {};
    let mongo_sort_field;

    match sort_field {
        "alphaCount" | "coverage" | "dateCoverage" => {
            let mut filter_conds = vec![];
            if let Some(reg) = &params.region {
                filter_conds.push(doc! { "$eq": ["$$this.region", reg] });
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
                // No filters: sort by total sum for alphas or max for coverage/dateCoverage
                if sort_field == "alphaCount" {
                    add_fields.insert("sortValue", doc! { "$sum": "$data.alphaCount" });
                } else {
                    let field_name = if sort_field == "coverage" { "coverage" } else { "dateCoverage" };
                    add_fields.insert("sortValue", doc! { "$max": format!("$data.{}", field_name) });
                }
            } else {
                // With filters: sort by the sum of matching entries (usually one entry)
                let field_name = if sort_field == "alphaCount" { "alphaCount" } else if sort_field == "coverage" { "coverage" } else { "dateCoverage" };
                add_fields.insert("sortValue", doc! {
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
                });
            }
            mongo_sort_field = "sortValue";
        },
        "id" => {
            mongo_sort_field = "id";
        },
        "dateCreated" => {
            mongo_sort_field = "dateCreated";
        },
        _ => {
            mongo_sort_field = "id";
        }
    }

    let mut pipeline = vec![
        doc! { "$match": filter.clone() },
    ];

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

    let total = collection.count_documents(filter, None).await.map_err(|e| e.to_string())?;
    let mut cursor = collection.aggregate(pipeline, None).await.map_err(|e| e.to_string())?;
    
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

#[command]
pub async fn get_datasets(
    params: DatasetQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PagedResult<Dataset>, String> {
    let client = &clients.local;
    let db_name = config.mongodb.databases.dataset.as_deref().unwrap_or("dataset_db");
    let coll_name = config.mongodb.databases.dataset_collection.as_deref().unwrap_or("datasets_all");
    
    let db = client.database(db_name);
    let collection = db.collection::<mongodb::bson::Document>(coll_name);

    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(50);
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
            let df_coll_name = config.mongodb.databases.datafield_collection.as_deref().unwrap_or("datafields_all");
            let df_coll = client.database(df_db_name).collection::<mongodb::bson::Document>(df_coll_name);
            
            let df_filter = doc! { "id": { "$regex": regex::escape(field_id.trim()), "$options": "i" } };
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
            filters.push(doc! { "name": { "$regex": regex::escape(name.trim()), "$options": "i" } });
        }
    }

    if let Some(cat) = &params.category {
        if !cat.is_empty() {
            filters.push(doc! { "category.name": { "$regex": regex::escape(cat.trim()), "$options": "i" } });
        }
    }

    // Data-level filters: use $elemMatch to ensure conditions match within the same element
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

    let total = collection.count_documents(filter, None).await.map_err(|e| e.to_string())?;
    let mut cursor = collection.aggregate(pipeline, None).await.map_err(|e| e.to_string())?;
    
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

fn get_alpha_projection() -> mongodb::bson::Document {
    doc! {
        "id": 1,
        "type": 1,
        "settings": 1,
        "regular.code": 1,
        "selection.code": 1,
        "combo.code": 1,
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

    let coll_name = params
        .collection
        .as_deref()
        .and_then(|s| sanitize_collection_name(s))
        .unwrap_or_else(|| "alpha_results".to_string());
    let collection = db.collection::<mongodb::bson::Document>(&coll_name);

    let t_filter = std::time::Instant::now();
    let mut filters = vec![];

    if let Some(q) = &params.query {
        if let Some(parsed_doc) = parse_query_to_bson(q) {
            filters.push(parsed_doc);
        } else if !q.trim().is_empty() {
            println!("JSON parsing failed, falling back to regex search: {}", q.trim());
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
            println!("Excluding regex: {}", ex_q.trim());
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
        filters.push(doc! { "id": id });
    }
    if let Some(region) = &params.region {
        filters.push(doc! { "settings.region": region });
    }
    if let Some(delay) = params.delay {
        filters.push(doc! { "settings.delay": delay as i32 });
    }
    if let Some(days) = params.days_within {
        let since = Utc::now() - Duration::days(days as i64);
        filters.push(doc! { "dateCreated": { "$gte": since.to_rfc3339() } });
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
        filters.push(doc! { "type": alpha_type });
    }
    if let Some(categories) = &params.self_categories {
        if !categories.is_empty() {
            let mut and_conds = Vec::new();
            for cat in categories {
                // Use regex to match the suffix (e.g. "OPTION" matches "USA/D1/OPTION")
                let pattern = format!("{}$", regex::escape(cat));
                and_conds.push(doc! { 
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
            filters.push(doc! { "$and": and_conds });
        }
    }

    let filter = if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    };
    println!("Final Mongo filter: {:?}", filter);
    println!("⏱ filter build took {:?}", t_filter.elapsed());
    println!("Mongo filter: {:?}", filter);

    let t_count = std::time::Instant::now();
    let t_find = std::time::Instant::now();

    let sort_doc = if let Some(field) = &params.sort_field {
        let order = params.sort_order.unwrap_or(-1);
        let mongo_field = match field {
            SortField::DateCreated => "dateCreated",
            SortField::DateSubmitted => "dateSubmitted",
            SortField::Sharpe => "is.sharpe",
            SortField::Returns => "is.returns",
            SortField::Turnover => "is.turnover",
            SortField::Fitness => "is.fitness",
            SortField::Margin => "is.margin",
            SortField::PnlScore => "pnl_score",
        };
        doc! { mongo_field: order }
    } else {
        doc! { "dateCreated": -1 }
    };

    let find_options = FindOptions::builder()
        .projection(get_alpha_projection())
        .sort(sort_doc)
        .skip(skip)
        .limit(page_size as i64)
        .build();

    let (count_res, cursor_res) = join!(
        collection.count_documents(filter.clone(), None),
        collection.find(filter.clone(), find_options)
    );

    let total = count_res.unwrap_or(0);
    println!("⏱ count_documents took {:?}", t_count.elapsed());

    let mut cursor = match cursor_res {
        Ok(cursor) => cursor,
        Err(e) => {
            println!("❌ collection.find() error: {}", e);
            return Err(format!("DB query error: {}", e));
        }
    };
    println!("⏱ collection.find() took {:?}", t_find.elapsed());

    let mut results = Vec::new();

    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        if let Some(result) = parse_alpha_document(doc, params.embedding.as_deref()) {
            results.push(result);
        }
    }

    drop(cursor);

    Ok(PagedResult {
        data: results,
        total,
        page,
        page_size,
    })
}



#[derive(Debug, Serialize)]
pub struct PnlPoint {
    date: String,
    pnl: f64,
    risk_neutralized_pnl: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PnlResponse {
    pnl_series: Vec<PnlPoint>,
}

#[derive(Deserialize)]
pub struct PnlQuery {
    pub id: String,
    pub collection: Option<String>,
}

#[command]
pub async fn get_pnl_by_id(
    query: PnlQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PnlResponse, String> {

    let client = &clients.local;
    let db = client.database(&config.mongodb.databases.alpha);

    let filter = doc! { "id": &query.id };
    
    // 1. First attempt: Look in the centralized partitioned collection "alpha_pnls"
    let pnls_coll = db.collection::<mongodb::bson::Document>("alpha_pnls");
    let mut doc_opt = pnls_coll.find_one(filter.clone(), None).await.ok().flatten();

    // 2. Second attempt: Fallback to the original source collection if not found in alpha_pnls
    if doc_opt.is_none() {
        if let Some(source_coll_name) = query.collection.as_deref().and_then(sanitize_collection_name) {
            if source_coll_name != "alpha_pnls" {
                let source_coll = db.collection::<mongodb::bson::Document>(&source_coll_name);
                doc_opt = source_coll.find_one(filter, None).await.ok().flatten();
            }
        }
    }

    let doc = doc_opt.ok_or_else(|| "PNL document not found in alpha_pnls or source collection".to_string())?;

    let pnl_obj = doc.get_document("pnl").map_err(|_| "No pnl field found")?;
    let records = pnl_obj.get_array("records").map_err(|_| "No records array found")?;

    let pnl_series: Vec<PnlPoint> = records
        .iter()
        .filter_map(|entry| {
            entry.as_array().and_then(|arr| {
                if arr.len() >= 2 {
                    let date = arr.get(0)?.as_str()?.to_string();
                    let pnl = arr.get(1)?.as_f64().or(arr.get(1)?.as_i32().map(|v| v as f64))?;
                    let risk_neutralized_pnl = if arr.len() > 2 {
                        arr.get(2)?.as_f64().or(arr.get(2)?.as_i32().map(|v| v as f64))
                    } else {
                        None
                    };
                    Some(PnlPoint { date, pnl, risk_neutralized_pnl })
                } else {
                    None
                }
            })
        })
        .collect();

    Ok(PnlResponse { pnl_series })
}


#[derive(Debug, Serialize, Deserialize)]
pub struct CorrResult {
    pub ppac_correlation: Option<String>,
    pub os_correlation: Option<String>,
}

#[command]
pub async fn compute_correlation(alpha_ids: Vec<String>, config: State<'_, AppConfig>) -> Result<HashMap<String, CorrResult>, String> {
    let alpha_str = alpha_ids.join(",");
    let mut replacements = HashMap::new();
    replacements.insert("{alpha_ids}", alpha_str.as_str());

    let stdout = run_python_command(&config, "alpha_correlation", &replacements).await?;

    serde_json::from_str(&stdout).map_err(|e| format!("解析 JSON 失败: {}", e))
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
    pub date_submitted: Option<String>, // ✅ 新增提交日期
    pub os_sharpe: Option<f64>,
    pub os_fitness: Option<f64>,
    pub sub_universe_sharpe: Option<f64>,
    pub message: Option<String>,
    pub cluster_x: Option<f64>,
    pub cluster_y: Option<f64>,
    pub cluster_id: Option<serde_json::Value>,
}

#[command]
pub async fn get_submission_stats(
    collection: Option<String>,
    query: Option<String>,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<Vec<mongodb::bson::Document>, String> {
    let client = &clients.local;
    let db = client.database(&config.mongodb.databases.alpha);

    let coll_name = collection
        .as_deref()
        .and_then(|s| sanitize_collection_name(s))
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
        doc! {
            "$match": filter
        },
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
        }
    ];

    let mut cursor = coll.aggregate(pipeline, None).await.map_err(|e| e.to_string())?;
    let mut results = Vec::new();

    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        results.push(doc);
    }

    Ok(results)
}

#[command]
pub async fn search_alpha_in_all_collections(
    query: SearchAlphaInAllCollectionsQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<Option<AlphaInCollectionResult>, String> {
    let client = &clients.local;
    let db = client.database(&config.mongodb.databases.alpha);
    
    // 获取所有collection名称
    let collection_names = db.list_collection_names(None).await
        .map_err(|e| format!("Failed to list collections: {}", e))?;
    
    // 在每个collection中搜索指定的alpha ID
    for coll_name in collection_names {
        // 跳过系统collection
        if coll_name.starts_with("system.") {
            continue;
        }
        
        let collection = db.collection::<mongodb::bson::Document>(&coll_name);
        let filter = doc! { "id": &query.id };
        
        if let Ok(Some(doc)) = collection.find_one(filter, FindOneOptions::builder().projection(get_alpha_projection()).build()).await {
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
    
    // 在所有collection中都没有找到
    Ok(None)
}