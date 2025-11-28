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
use crate::config::{AppConfig, run_python_module};



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
    pub status: String,
    pub message: Option<String>,
    pub date_created: Option<String>,    // ✅ 新增字段，使用字符串存时间戳
    pub pnl_score: Option<f64>,   // ✅ 新增

}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]  // 前端传字符串时自动匹配
pub enum SortField {
    DateCreated,
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
    pub status: Option<String>,
    pub region: Option<String>,
    pub days_within: Option<u32>,
    pub min_turnover: Option<f64>,
    pub max_turnover: Option<f64>,
    pub min_margin: Option<f64>,
    pub delay: Option<u32>,
    pub min_returns: Option<f64>,
    pub collection: Option<String>, // 新增：可选的 collection 名称
    pub page: Option<u32>,       // ✅ 新增
    pub page_size: Option<u32>,  // ✅ 新增

    pub sort_field: Option<SortField>, // ✅ 使用枚举
    pub sort_order: Option<i32>,       // 1 = 升序, -1 = 降序
}

fn sanitize_collection_name(name: &str) -> Option<String> {
    // 只允许字母数字和下划线，防止注入奇怪字符
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(name.to_string())
    } else {
        None
    }
}

#[derive(Serialize)]
pub struct PagedResult<T> {
    pub data: Vec<T>,   // 当前页数据
    pub total: u64,     // 总条数
    pub page: u32,      // 当前页
    pub page_size: u32, // 每页大小
}


#[command]
pub async fn get_alpha_results(
    params: AlphaQuery,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<PagedResult<AlphaResult>, String> {

    let t0 = std::time::Instant::now();

    let client = &clients.local;
    let db = client.database(&config.mongodb.databases.alpha);

    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(50);
    let skip = ((page - 1) * page_size) as u64;

    // --- STEP 1: 获取 collection ---
    let coll_name = params
        .collection
        .as_deref()
        .and_then(|s| sanitize_collection_name(s))
        .unwrap_or_else(|| "alpha_results".to_string());
    let collection = db.collection::<mongodb::bson::Document>(&coll_name);

    // --- STEP 2: 构建过滤条件 ---
    let t_filter = std::time::Instant::now();
    let mut filters = vec![];

    if let Some(q) = &params.query {
        let escaped = regex::escape(q);
        filters.push(doc! {
            "$or": [
                { "regular.code": { "$regex": &escaped, "$options": "i" } },
                { "selection.code": { "$regex": &escaped, "$options": "i" } },
                { "combo.code": { "$regex": &escaped, "$options": "i" } }
            ]
        });
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

    // --- STEP 3: 并发执行 count + find ---
    let t_count = std::time::Instant::now();
    let t_find = std::time::Instant::now();

    let sort_doc = if let Some(field) = &params.sort_field {
        let order = params.sort_order.unwrap_or(-1);
        let mongo_field = match field {
            SortField::DateCreated => "dateCreated",
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

    // 并发执行 count_documents 和 find
    let (count_res, cursor_res) = join!(
        collection.count_documents(filter.clone(), None),
        collection.find(filter.clone(), find_options)
    );

    let total = match count_res {
        Ok(count) => count,
        Err(e) => {
            println!("⚠️ count_documents error: {}", e);
            0
        }
    };
    println!("⏱ count_documents took {:?}", t_count.elapsed());

    let mut cursor = match cursor_res {
        Ok(cursor) => cursor,
        Err(e) => {
            println!("❌ collection.find() error: {}", e);
            return Err(format!("DB query error: {}", e));
        }
    };
    println!("⏱ collection.find() took {:?}", t_find.elapsed());

    // --- STEP 4: 遍历 cursor 并解析结果 ---
    let t_fetch = std::time::Instant::now();
    let mut results = Vec::new();
    let mut doc_count = 0;

    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        doc_count += 1;

        let id = match doc.get_str("id") {
            Ok(s) => s.to_string(),
            Err(_) => continue,
        };

        let region = match doc.get_document("settings").and_then(|d| d.get_str("region")) {
            Ok(s) => s.to_string(),
            Err(_) => continue,
        };

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

        // 检查 status
        let mut sub_universe_sharpe = None;
        let mut message = None;
        let mut status = "UNKNOWN".to_string();

        if let Some(checks) = is.and_then(|d| d.get_array("checks").ok()) {
            for item in checks {
                if let Some(check_doc) = item.as_document() {
                    if let Ok(result) = check_doc.get_str("result") {
                        if result == "FAIL" || result == "WARNING" {
                            status = "FAIL".to_string();
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

        if let Some(status_filter) = &params.status {
            if status != *status_filter {
                continue;
            }
        }

        results.push(AlphaResult {
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
            status,
            message,
            date_created,
            pnl_score,
        });

        if doc_count % 50 == 0 {
            println!(
                "⏱ parsed {} docs, elapsed {:?}",
                doc_count,
                t_fetch.elapsed()
            );
        }
    }

    drop(cursor);


    println!("⏱ cursor iteration & parse took {:?}", t_fetch.elapsed());
    println!("Total results returned: {}", results.len());
    println!("✅ Total get_alpha_results() took {:?}", t0.elapsed());

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
    risk_neutralized_pnl: Option<f64>,  // 如果有的话
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
    use mongodb::bson::doc;

    let client = &clients.local;
    let db = client.database(&config.mongodb.databases.alpha);

    let coll_name = query
        .collection
        .as_deref()
        .and_then(|s| sanitize_collection_name(s))
        .unwrap_or_else(|| "alpha_results".to_string());

    let collection = db.collection::<mongodb::bson::Document>(&coll_name);
    let filter = doc! { "id": &query.id };

    // 查询并返回文档
    let doc = collection
        .find_one(filter, None)
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "Document not found".to_string())?;

    // println!("📦 Document keys: {:?}", doc.keys().collect::<Vec<_>>());

    // 提取 pnl 字段中的 records 数组
    let pnl_obj = doc.get_document("pnl").map_err(|_| "No pnl field")?;
    let records = pnl_obj.get_array("records").map_err(|_| "No records array")?;

    // 将 records 数组转换为 PnlPoint 数组
    let pnl_series: Vec<PnlPoint> = records
        .iter()
        .filter_map(|entry| {
            entry.as_array().and_then(|arr| {
                if arr.len() >= 2 {
                    // 提取第一个元素作为日期
                    let date = arr.get(0)?.as_str()?.to_string();
                    
                    // 提取第二个元素作为 PnL
                    let pnl = arr.get(1)?.as_f64().or(arr.get(1)?.as_i32().map(|v| v as f64))?;
                    
                    // 提取第三个元素作为风险中性PnL（如果存在）
                    let risk_neutralized_pnl = if arr.len() > 2 {
                        arr.get(2)?.as_f64().or(arr.get(2)?.as_i32().map(|v| v as f64))
                    } else {
                        None
                    };

                    // 返回 PnlPoint 实例
                    Some(PnlPoint { date, pnl, risk_neutralized_pnl })
                } else {
                    None
                }
            })
        })
        .collect();

    // 返回结果
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

    // let stdout = run_python_module(&config, "alpha_correlation", &replacements)?;
    let stdout = run_python_module(&config, "alpha_correlation", &replacements).await?; // 注意 .await?


    serde_json::from_str(&stdout).map_err(|e| format!("解析 JSON 失败: {}", e))
}