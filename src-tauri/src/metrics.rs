use serde::{Deserialize, Serialize};
use tauri::command;

#[derive(Debug, Deserialize)]
pub struct PnlPoint {
    pub date: String,
    pub pnl: f64,
}

#[derive(Debug, Serialize)]
pub struct MetricsResponse {
    pub sharpe: f64,
    pub returns: f64,
}

#[command]
pub fn calculate_pnl_metrics(pnl_series: Vec<PnlPoint>) -> Result<MetricsResponse, String> {
    if pnl_series.len() < 2 {
        return Ok(MetricsResponse { sharpe: 0.0, returns: 0.0 });
    }

    // Ensure it's sorted by date
    let mut sorted_pnl = pnl_series;
    sorted_pnl.sort_by(|a, b| a.date.cmp(&b.date));

    // Calculate daily PnL (daily returns)
    let mut daily_pnl = Vec::new();
    for i in 1..sorted_pnl.len() {
        daily_pnl.push(sorted_pnl[i].pnl - sorted_pnl[i-1].pnl);
    }

    if daily_pnl.is_empty() {
        return Ok(MetricsResponse { sharpe: 0.0, returns: 0.0 });
    }

    let n = daily_pnl.len() as f64;
    let sum_pnl: f64 = daily_pnl.iter().sum();
    let mean_pnl = sum_pnl / n;

    // Calculate standard deviation of daily PnL
    let variance = daily_pnl.iter()
        .map(|&x| (x - mean_pnl).powi(2))
        .sum::<f64>() / (n - 1.0);
    
    let std_dev = variance.sqrt();

    // Annualized Sharpe Ratio = (mean / std) * sqrt(252)
    // We assume 252 trading days per year
    let sharpe = if std_dev > 0.0 {
        (mean_pnl / std_dev) * (252.0f64).sqrt()
    } else {
        0.0
    };

    // Annualized Return = mean_daily_pnl * 252
    let annualized_return = mean_pnl * 252.0;

    Ok(MetricsResponse {
        sharpe,
        returns: annualized_return,
    })
}
