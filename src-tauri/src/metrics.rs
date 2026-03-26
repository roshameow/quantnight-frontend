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
    pub drawdown: f64,
    pub predicted_score: f64,
}

fn predict_wqb_score(sharpe: f64, turnover: f64, returns: f64, drawdown: f64) -> f64 {
    // Expected inputs (all decimals, rounded to UI precision):
    // sharpe: 1.21, turnover: 0.0636, returns: 0.0517, drawdown: 0.0608
    
    let pi = std::f64::consts::PI;
    let af_physical = 250.0f64.sqrt();
    let c_curve = (40.0 / pi) * af_physical;
    let c_neg = 20.0 * af_physical;
    let p_floor = 0.5 * pi;
    let p_ceil = 1.5 * pi;
    
    let a = 200.0;
    let b = 2.0;
    let c = 0.34108;
    
    let to_off = 0.10; 
    let eps_sharpe = 1e-4;
    
    let bu = (sharpe.abs() + eps_sharpe) / (turnover.abs() + to_off);
    
    if sharpe <= 0.0 {
        return -c_neg * bu;
    }
    
    let calmar = returns.abs() / (drawdown.abs() + 1e-9);
    let p_calc = c * (a * calmar.powf(b)).asinh();
    let p_quality = p_calc.clamp(p_floor, p_ceil);
        
    c_curve * p_quality * bu
}

#[command]
pub fn calculate_pnl_metrics(
    pnl_series: Vec<PnlPoint>, 
    avg_turnover: f64, 
    avg_returns: f64
) -> Result<MetricsResponse, String> {
    if pnl_series.len() < 2 {
        return Ok(MetricsResponse { sharpe: 0.0, returns: 0.0, drawdown: 0.0, predicted_score: 0.0 });
    }

    let mut sorted_pnl = pnl_series;
    sorted_pnl.sort_by(|a, b| a.date.cmp(&b.date));

    let capital = 10_000_000.0;
    
    // 1. Max Drawdown (Absolute -> Pct)
    let mut max_drawdown_abs = 0.0;
    let mut peak = sorted_pnl[0].pnl;
    for point in &sorted_pnl {
        if point.pnl > peak {
            peak = point.pnl;
        }
        let dd = peak - point.pnl;
        if dd > max_drawdown_abs {
            max_drawdown_abs = dd;
        }
    }
    let max_drawdown_pct = max_drawdown_abs / capital;

    // 2. Daily Returns for Sharpe
    let mut daily_pnl = Vec::new();
    for i in 1..sorted_pnl.len() {
        daily_pnl.push(sorted_pnl[i].pnl - sorted_pnl[i-1].pnl);
    }

    let n = daily_pnl.len() as f64;
    let sum_pnl: f64 = daily_pnl.iter().sum();
    let mean_pnl = sum_pnl / n;

    // Use Population Standard Deviation (like np.std with ddof=0)
    let variance = daily_pnl.iter()
        .map(|&x| (x - mean_pnl).powi(2))
        .sum::<f64>() / n;
    let std_dev = variance.sqrt();

    // 3. Sharpe for Display (252 days)
    let af_metrics = 252.0f64;
    let raw_sharpe = if std_dev > 0.0 {
        (mean_pnl / std_dev) * af_metrics.sqrt()
    } else {
        0.0
    };
    
    // 4. Calculate native returns of the curve (for internal use, not display)
    let raw_returns_series = (mean_pnl * af_metrics) / capital;

    // 5. PRECISE UI ROUNDING FOR SCORE PREDICTION
    // We match what the user sees in the UI. 
    // sharpe and drawdown come from this curve calculation.
    // turnover and returns come from the metadata average passed from frontend.
    let sharpe_input = (raw_sharpe * 100.0).round() / 100.0;
    let drawdown_input = (max_drawdown_pct * 10000.0).round() / 10000.0;
    let turnover_input = (avg_turnover * 10000.0).round() / 10000.0;
    let returns_input = (avg_returns * 10000.0).round() / 10000.0;

    // 6. Predict Score
    let predicted_score = predict_wqb_score(sharpe_input, turnover_input, returns_input, drawdown_input);

    Ok(MetricsResponse {
        sharpe: raw_sharpe,
        returns: raw_returns_series,
        drawdown: max_drawdown_pct,
        predicted_score,
    })
}
