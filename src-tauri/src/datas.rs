use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use futures_util::stream::TryStreamExt;
use futures::join;

use mongodb::{
    bson::doc,
    options::FindOptions,
};
use serde::{Deserialize, Serialize};
use tauri::{command, State};

use crate::mongo_manager::MongoClients;
use crate::config::{AppConfig, run_python_command};



#[derive(Serialize, Deserialize, Debug)]
pub struct AlphaResult {
    pub id: String,  // 原来是 ObjectId，改为 String
    pub region: String,
    pub code: Option<String>,
    pub sharpe: Option<f64>,
    pub fitness: Option<f64>,
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
    pub cluster_x: Option<f64>,
    pub cluster_y: Option<f64>,
    pub cluster_id: Option<serde_json::Value>,
    pub classifications: Option<Vec<serde_json::Value>>,
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

fn parse_alpha_document(doc: mongodb::bson::Document, embedding_key: Option<&str>) -> Option<AlphaResult> {
    let id = doc.get_str("id").ok()?.to_string();

    let region = doc.get_document("settings").ok()
        .and_then(|d| d.get_str("region").ok())
        .unwrap_or("Unknown")
        .to_string();

    let alpha_type = doc.get_str("type").unwrap_or("UNKNOWN");
    let code = match alpha_type {
        "REGULAR" => doc.get_document("regular").ok()
            .and_then(|d| d.get_str("code").ok())
            .map(|s| s.to_string()),
        "SUPER" => {
            let selection_code = doc.get_document("selection").ok()
                .and_then(|d| d.get_str("code").ok())
                .unwrap_or("")
                .to_string();
            let combo_code = doc.get_document("combo").ok()
                .and_then(|d| d.get_str("code").ok())
                .unwrap_or("")
                .to_string();
            Some(format!("{}\n{}", selection_code, combo_code))
        }
        _ => None,
    };

    let is = doc.get_document("is").ok();
    let sharpe = is.and_then(|d| d.get_f64("sharpe").ok());
    let fitness = is.and_then(|d| d.get_f64("fitness").ok());
    let turnover = is.and_then(|d| d.get_f64("turnover").ok());
    let margin = is.and_then(|d| d.get_f64("margin").ok());
    let long_count = is.and_then(|d| d.get_i32("longCount").ok());
    let short_count = is.and_then(|d| d.get_i32("shortCount").ok());
    let returns = is.and_then(|d| d.get_f64("returns").ok());
    let pnl_score = doc.get_f64("pnl_score").ok();

    let date_created = doc.get_str("dateCreated").ok().map(|s| s.to_string());
    let date_submitted = doc.get_str("dateSubmitted").ok().map(|s| s.to_string());

    let classifications = doc.get_array("classifications").ok().map(|arr| {
        arr.iter().filter_map(|bson| {
            if let Some(doc) = bson.as_document() {
                let id = doc.get_str("id").unwrap_or("").to_string();
                let name = doc.get_str("name").unwrap_or("").to_string();
                Some(serde_json::json!({ "id": id, "name": name }))
            } else {
                None
            }
        }).collect()
    });

    let (cluster_x, cluster_y, cluster_id) = if let Some(analysis_doc) = doc.get_document("analysis").ok() {
        if let Some(key) = embedding_key {
            analysis_doc.get_document("embeddings").ok()
                .and_then(|em| em.get_document(key).ok())
                .map_or((None, None, None), |umap| {
                    let x = umap.get_f64("x").ok();
                    let y = umap.get_f64("y").ok();
                    let cid = umap.get_document("cluster").ok()
                                .and_then(|c| c.get("id"))
                                .and_then(|id_val| match id_val {
                                    mongodb::bson::Bson::Int32(i) => Some(serde_json::json!(i)),
                                    mongodb::bson::Bson::Int64(i) => Some(serde_json::json!(i)),
                                    mongodb::bson::Bson::Double(f) => Some(serde_json::json!(f)),
                                    mongodb::bson::Bson::String(s) => Some(serde_json::json!(s)),
                                    _ => None,
                                });
                    (x, y, cid)
                })
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };
    
    let mut sub_universe_sharpe = None;
    let mut message = None;

    if let Some(checks) = is.and_then(|d| d.get_array("checks").ok()) {
        for item in checks {
            if let Some(check_doc) = item.as_document() {
                if let Ok(result) = check_doc.get_str("result") {
                    if result == "FAIL" || result == "WARNING" {
                        if let Ok(name) = check_doc.get_str("name") {
                            message = Some(match message {
                                Some(m) => format!("{},{}", m, name),
                                None => name.to_string(),
                            });
                        }
                    }
                }

                if check_doc.get_str("name") == Ok("LOW_SUB_UNIVERSE_SHARPE") {
                    sub_universe_sharpe = check_doc.get_f64("value").ok();
                }
            }
        }
    }

    Some(AlphaResult {
        id,
        region,
        code,
        sharpe,
        fitness,
        returns,
        turnover,
        margin,
        long_count,
        short_count,
        sub_universe_sharpe,
        message,
        date_created,
        date_submitted,
        pnl_score,
        cluster_x,
        cluster_y,
        cluster_id,
        classifications,
    })
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
        let q_trimmed = q.trim();
        let mut parsed_ok = false;
        
        // Try parsing as-is first
        if let Ok(parsed_doc) = serde_json::from_str::<mongodb::bson::Document>(q_trimmed) {
            filters.push(parsed_doc);
            parsed_ok = true;
        } else if q_trimmed.starts_with('{') && q_trimmed.ends_with('}') {
            // If it looks like JSON but failed, try cleaning trailing commas
            // This is a simple regex-based cleanup for trailing commas in objects and arrays
            let re = regex::Regex::new(r",\s*([\]}])").unwrap();
            let cleaned_q = re.replace_all(q_trimmed, "$1").to_string();
            
            if let Ok(parsed_doc) = serde_json::from_str::<mongodb::bson::Document>(&cleaned_q) {
                filters.push(parsed_doc);
                parsed_ok = true;
            } else {
                return Err("Invalid JSON query format. Please check for syntax errors.".to_string());
            }
        }

        if !parsed_ok {
            let escaped = regex::escape(q);
            filters.push(doc! {
                "$or": [
                    { "regular.code": { "$regex": &escaped, "$options": "i" } },
                    { "selection.code": { "$regex": &escaped, "$options": "i" } },
                    { "combo.code": { "$regex": &escaped, "$options": "i" } }
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

    let filter = if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    };
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
        .projection(doc! { "pnl": 0 })
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

    let coll_name = query
        .collection
        .as_deref()
        .and_then(|s| sanitize_collection_name(s))
        .unwrap_or_else(|| "alpha_results".to_string());

    let collection = db.collection::<mongodb::bson::Document>(&coll_name);
    let filter = doc! { "id": &query.id };

    let doc = collection
        .find_one(filter, None)
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "Document not found".to_string())?;

    let pnl_obj = doc.get_document("pnl").map_err(|_| "No pnl field")?;
    let records = pnl_obj.get_array("records").map_err(|_| "No records array")?;

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
    pub returns: Option<f64>,
    pub turnover: Option<f64>,
    pub margin: Option<f64>,
    pub date_created: Option<String>,
    pub date_submitted: Option<String>, // ✅ 新增提交日期
    pub sub_universe_sharpe: Option<f64>,
    pub message: Option<String>,
    pub cluster_x: Option<f64>,
    pub cluster_y: Option<f64>,
    pub cluster_id: Option<serde_json::Value>,
}

#[command]
pub async fn get_submission_stats(
    collection: Option<String>,
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

    let pipeline = vec![
        doc! {
            "$project": {
                "region": { "$ifNull": ["$settings.region", "Unknown"] },
                "month": { "$substr": [{ "$ifNull": ["$dateSubmitted", "$dateCreated"] }, 0, 7] },
                "sharpe": "$is.sharpe",
                "fitness": "$is.fitness",
                "turnover": "$is.turnover",
                "returns": "$is.returns",
                "margin": "$is.margin",
            }
        },
        doc! {
            "$group": {
                "_id": { "month": "$month", "region": "$region" },
                "count": { "$sum": 1 },
                "avg_sharpe": { "$avg": "$sharpe" },
                "avg_fitness": { "$avg": "$fitness" },
                "avg_turnover": { "$avg": "$turnover" },
                "avg_returns": { "$avg": "$returns" },
                "avg_margin": { "$avg": "$margin" },
            }
        },
        doc! {
            "$sort": { "_id.month": -1, "_id.region": 1 }
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
        
        if let Ok(Some(doc)) = collection.find_one(filter, None).await {
            if let Some(result) = parse_alpha_document(doc, None) {
                return Ok(Some(AlphaInCollectionResult {
                    id: result.id,
                    collection: coll_name,
                    region: result.region,
                    code: result.code,
                    sharpe: result.sharpe,
                    fitness: result.fitness,
                    returns: result.returns,
                    turnover: result.turnover,
                    margin: result.margin,
                    date_created: result.date_created,
                    date_submitted: result.date_submitted,
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