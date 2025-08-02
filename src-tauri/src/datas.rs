use std::collections::HashMap;

use chrono::{Duration, Utc};
use futures_util::stream::TryStreamExt;
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
pub struct AlphaQuery {
    pub query: Option<String>,
    pub status: Option<String>,
    pub region: Option<String>,
    pub days_within: Option<u32>,
    pub min_turnover: Option<f64>,
    pub max_turnover: Option<f64>,
    pub min_margin: Option<f64>, // ✅ 新增字段
    pub delay: Option<u32>,      // ✅ 新增：delay 筛选
    pub min_returns: Option<f64>,    // ✅ 新增：returns 筛选
}

#[command]
pub async fn get_alpha_results(params: AlphaQuery,clients: State<'_, MongoClients>) -> Result<Vec<AlphaResult>, String> {

    let client = &clients.local;

    let db = client.database("alpha_db");
    let collection = db.collection::<mongodb::bson::Document>("alpha_results");

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

    if let Some(region) = &params.region {
        filters.push(doc! { "settings.region": region });
    }

    if let Some(delay) = params.delay {
        filters.push(doc! { "settings.delay": delay as i32 });
    }

    if let Some(days) = params.days_within {
        let since = Utc::now() - Duration::days(days as i64);
        println!("Filtering by date >= {}", since.to_rfc3339());
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
            "$expr": {
                "$gte": [
                    { "$abs": "$is.margin" },
                    min_margin
                ]
            }
        });
    }

    if let Some(min_returns) = params.min_returns {
        filters.push(doc! {
            "$expr": {
                "$gte": [
                    { "$abs": "$is.returns" },
                    min_returns
                ]
            }
        });
    }

    let filter = if filters.is_empty() {
        doc! {}
    } else {
        doc! { "$and": filters }
    };

    println!("Mongo filter: {:?}", filter);

    let find_options = FindOptions::builder()
        .projection(doc! { "pnl": 0 })  // 排除 pnl 字段
        .sort(doc! { "dateCreated": -1 }) // 按照日期倒序排序
        .build();

    let mut cursor = match collection.find(filter, find_options).await {
        Ok(cursor) => cursor,
        Err(e) => {
            println!("DB query error: {}", e);
            return Err(format!("DB query error: {}", e));
        }
    };

    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        let id = match doc.get_str("id") {
            Ok(s) => s.to_string(),
            Err(_) => {
                println!("❗ Document missing 'id': {:?}", doc);
                continue;
            }
        };
        
        let region = match doc.get_document("settings").and_then(|d| d.get_str("region")) {
            Ok(s) => s.to_string(),
            Err(_) => {
                println!("❗ Document missing 'settings.region': {:?}", doc);
                continue;
            }
        };


        let alpha_type = doc.get_str("type").unwrap_or("UNKNOWN");
        if alpha_type == "UNKNOWN" {
            println!("⚠️ Unknown alpha type in doc {}: {:?}", id, doc.get("type"));
        }

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
            _ => {
                println!("❓ Unrecognized alpha_type '{}', no code extracted", alpha_type);
                None
            }
        };

        let is = match doc.get_document("is") {
            Ok(doc) => Some(doc),
            Err(_) => {
                println!("⚠️ Missing or invalid 'is' in doc {}: {:?}", id, doc.get("is"));
                None
            }
        };

        let sharpe = is.and_then(|d| d.get_f64("sharpe").ok());
        let fitness = is.and_then(|d| d.get_f64("fitness").ok());
        let turnover = is.and_then(|d| d.get_f64("turnover").ok());
        let margin = is.and_then(|d| d.get_f64("margin").ok());
        let long_count = is.and_then(|d| d.get_i32("longCount").ok());
        let short_count = is.and_then(|d| d.get_i32("shortCount").ok());
        let returns = is.and_then(|d| d.get_f64("returns").ok());
        let pnl_score = doc.get_f64("pnl_score").ok();


        let date_created = doc.get_str("dateCreated").ok().map(|s| s.to_string());
        if date_created.is_none() {
            println!("📅 Missing dateCreated in doc {}", id);
        }

        let mut sub_universe_sharpe = None;
        let mut message = None;
        let mut status = "UNKNOWN".to_string();

        if let Some(checks) = is.and_then(|d| d.get_array("checks").ok()) {
            for item in checks {
                if let Some(check_doc) = item.as_document() {
                    if let Ok(result) = check_doc.get_str("result") {
                        if result == "FAIL" || result == "WARNING" {
                            status = "FAIL".to_string();  // 将状态设置为 "FAIL"
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



        // 根据计算结果筛选status
        if let Some(status_filter) = &params.status {
            if status != *status_filter {
                continue;  // 如果状态不匹配，则跳过
            }
        }
        // println!("✅ Parsed result for {}: code = {:?}", id, code);

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
    }

    println!("Total results returned: {}", results.len());
    Ok(results)
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

#[command]
pub async fn get_pnl_by_id(
    id: String,
    state: State<'_, MongoClients>, // ✅ 正确引入 State
) -> Result<PnlResponse, String> {
    use mongodb::bson::{doc, Document};
    let client = &state.local; // 使用本地 Mongo


    // 获取数据库和集合
    let db = client.database("alpha_db");
    let collection = db.collection::<Document>("alpha_results");

    // 查询条件，匹配指定的 id
    let filter = doc! { "id": &id };
    // println!("🔎 Querying PnL for id = {}", id);

    // 查询并返回文档
    let doc = collection
        .find_one(filter, None)
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "Document not found".to_string())?;

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
pub fn compute_correlation(alpha_ids: Vec<String>, config: State<'_, AppConfig>) -> Result<HashMap<String, CorrResult>, String> {
    let alpha_str = alpha_ids.join(",");
    let mut replacements = HashMap::new();
    replacements.insert("{alpha_ids}", alpha_str.as_str());

    let stdout = run_python_module(&config, "alpha_correlation", &replacements)?;

    serde_json::from_str(&stdout).map_err(|e| format!("解析 JSON 失败: {}", e))
}








