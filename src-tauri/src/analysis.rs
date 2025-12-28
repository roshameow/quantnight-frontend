use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::future::join_all;
use mongodb::bson::doc;
use ndarray::Array2;
use serde::{Deserialize, Serialize};
use tauri::{command, State};

use linfa::traits::{Fit, Predict};
use linfa::Dataset;
use linfa_clustering::KMeans;
use linfa_reduction::Pca;

use crate::config::AppConfig;
use crate::mongo_manager::MongoClients;

#[derive(Deserialize, Debug)]
pub struct AlphaIdentity {
    id: String,
    collection: String,
}

#[derive(Deserialize, Debug)]
pub struct PcaRequest {
    alphas: Vec<AlphaIdentity>,
}

#[derive(Serialize, Debug)]
pub struct PcaPoint {
    id: String,
    x: f64,
    y: f64,
    cluster: usize,
}

#[command]
pub async fn compute_pnl_pca(
    request: PcaRequest,
    clients: State<'_, Arc<MongoClients>>,
    config: State<'_, AppConfig>,
) -> Result<Vec<PcaPoint>, String> {
    if request.alphas.len() < 3 {
        return Err("需要至少3个Alpha才能进行PCA分析".to_string());
    }

    let client = &clients.local;
    let db = client.database(&config.mongodb.databases.alpha);

    // 1. Fetch PnL data concurrently
    let mut futures = Vec::new();
    for alpha in &request.alphas {
        let collection = db.collection::<mongodb::bson::Document>(&alpha.collection);
        let filter = doc! { "id": &alpha.id };
        futures.push(async move {
            let doc_opt = collection.find_one(filter, None).await.ok().flatten();
            (alpha.id.clone(), doc_opt)
        });
    }

    let results = join_all(futures).await;

    // 2. Process and Align PnL Data
    let mut all_dates_set: HashSet<String> = HashSet::new();
    let mut alpha_pnl_map: HashMap<String, HashMap<String, f64>> = HashMap::new();

    for (id, doc_opt) in results {
        if let Some(doc) = doc_opt {
            if let Ok(pnl_doc) = doc.get_document("pnl") {
                if let Ok(records) = pnl_doc.get_array("records") {
                    let mut pnl_series: HashMap<String, f64> = HashMap::new();
                    for r in records {
                        if let Some(arr) = r.as_array() {
                            if arr.len() >= 2 {
                                if let (Some(d), Some(v)) = (arr[0].as_str(), arr[1].as_f64().or(arr[1].as_i32().map(|i| i as f64))) {
                                    if v.is_finite() {
                                        all_dates_set.insert(d.to_string());
                                        pnl_series.insert(d.to_string(), v);
                                    }
                                }
                            }
                        }
                    }
                    if !pnl_series.is_empty() {
                        alpha_pnl_map.insert(id, pnl_series);
                    }
                }
            }
        }
    }

    if alpha_pnl_map.is_empty() {
        return Err("没有找到有效的PnL数据".to_string());
    }

    // 3. Filter dates to last 4 years
    let mut all_dates: Vec<String> = all_dates_set.into_iter().collect();
    if all_dates.is_empty() {
        return Err("未在返回的 PnL 数据中找到任何有效日期".to_string());
    }
    all_dates.sort();

    let max_date_str = all_dates.last().unwrap();
    let max_date = chrono::NaiveDate::parse_from_str(max_date_str, "%Y-%m-%d")
        .map_err(|e| format!("无法解析最大日期: '{}', 错误: {}", max_date_str, e))?;

    // Use a more standard duration for 4 years
    let four_years_ago = max_date - chrono::Duration::days(4 * 365 + 1);

    let relevant_dates: Vec<String> = all_dates.into_iter().filter(|d| {
        if let Ok(current_date) = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            current_date >= four_years_ago
        } else {
            false
        }
    }).collect();

    if relevant_dates.len() < 2 {
        return Err("在最近4年内没有足够的交易日期 (少于2天)".to_string());
    }
    
    // 4. Build feature matrix with forward-fill and daily returns
    let n_features = relevant_dates.len() - 1;
    let valid_ids: Vec<String> = request.alphas.iter()
        .map(|a| a.id.clone())
        .filter(|id| alpha_pnl_map.contains_key(id))
        .collect();
    
    let n_samples_valid = valid_ids.len();

    if n_samples_valid < 3 {
        return Err("有效数据的Alpha数量不足".to_string());
    }
    
    let mut data_matrix = Array2::<f64>::zeros((n_samples_valid, n_features));

    for (i, id) in valid_ids.iter().enumerate() {
        let pnl_series = alpha_pnl_map.get(id).unwrap();
        let mut last_pnl: Option<f64> = None;
        let mut filled_pnl = Vec::new();

        // Forward-fill PnL series against the union of relevant dates
        for date in &relevant_dates {
            if let Some(pnl) = pnl_series.get(date) {
                last_pnl = Some(*pnl);
            }
            filled_pnl.push(last_pnl);
        }

        // Calculate daily returns and fill matrix row
        for j in 1..filled_pnl.len() {
            let pnl_today = filled_pnl[j];
            let pnl_yesterday = filled_pnl[j-1];
            
            let ret = match (pnl_today, pnl_yesterday) {
                (Some(today), Some(yesterday)) => today - yesterday,
                _ => 0.0, // If there's a None (should only be at the start), return is 0
            };
            
            data_matrix[[i, j-1]] = if ret.is_finite() { ret } else { 0.0 };
        }
    }

    // 5. PCA
    // Create a dummy target array for the dataset, required by linfa v0.7
    let targets: ndarray::Array1<usize> = ndarray::Array1::zeros(n_samples_valid);
    let dataset = Dataset::new(data_matrix, targets);
    
    // We want 2 components
    let pca_model = Pca::params(2)
        .fit(&dataset)
        .map_err(|e| format!("PCA fit error: {}", e))?;
    let projected_records = pca_model.predict(&dataset); // This returns ArrayBase, not Dataset

    // 6. Clustering (K-Means)
    // We must create a new dataset for K-Means from the projected data
    let projected_targets: ndarray::Array1<usize> = ndarray::Array1::zeros(n_samples_valid);
    let projected_dataset = Dataset::new(projected_records.clone(), projected_targets);

    // Determine K. Let's use min(4, n_samples)
    let k = std::cmp::min(4, n_samples_valid);
    let kmeans = KMeans::params(k)
        .max_n_iterations(200)
        .tolerance(1e-5)
        .fit(&projected_dataset)
        .map_err(|e| format!("KMeans fit error: {}", e))?;
    
    let clusters = kmeans.predict(projected_dataset); // Predict returns a Dataset

    // 7. Result
    let mut pca_points = Vec::new();
    for i in 0..n_samples_valid {
        let point = projected_records.row(i);
        let cluster_id = clusters.targets()[i]; // Use .targets() to get the array of predictions
        
        pca_points.push(PcaPoint {
            id: valid_ids[i].clone(),
            x: point[0],
            y: point[1],
            cluster: cluster_id,
        });
    }

    Ok(pca_points)
}
