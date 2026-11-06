use std::sync::OnceLock;
use mongodb::bson::{doc, Document};
use regex::Regex;
use serde::{Deserialize, Serialize};

fn re_true() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bTrue\b").unwrap())
}

fn re_false() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bFalse\b").unwrap())
}

fn re_none() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bNone\b").unwrap())
}

fn re_int() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(?:int|NumberInt)\((\d+)\)").unwrap())
}

fn re_float() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(?:float|NumberDecimal)\(([\d\.]+)\)").unwrap())
}

fn re_keys() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([{,\[]\s*)([\$a-zA-Z_][\$a-zA-Z0-9_\.]*)\s*:").unwrap())
}

fn re_comma() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r",\s*([\]}])").unwrap())
}

/// Parse user-supplied JSON or relaxed Mongo query syntax into a BSON Document.
/// Supports macOS smart quotes, Python booleans (True/False/None), unquoted keys,
/// and trailing commas.
pub fn parse_query_to_bson(q: &str) -> Option<Document> {
    let q_trimmed = q.trim();
    if q_trimmed.is_empty() {
        return None;
    }

    // 1. Normalize smart quotes (common on macOS) and Python-style values
    let mut normalized_q = q_trimmed
        .replace('“', "\"")
        .replace('”', "\"")
        .replace('‘', "'")
        .replace('’', "'");

    normalized_q = re_true().replace_all(&normalized_q, "true").to_string();
    normalized_q = re_false().replace_all(&normalized_q, "false").to_string();
    normalized_q = re_none().replace_all(&normalized_q, "null").to_string();
    normalized_q = re_int().replace_all(&normalized_q, "$1").to_string();
    normalized_q = re_float().replace_all(&normalized_q, "$1").to_string();

    // 2. Smart wrap: if it looks like an object (contains :) but lacks braces, wrap it
    if !normalized_q.starts_with('{') && normalized_q.contains(':') {
        normalized_q = format!("{{{}}}", normalized_q);
    }

    // 3. Automatically quote unquoted keys (e.g. $or -> "$or", os.sharpe -> "os.sharpe")
    normalized_q = re_keys().replace_all(&normalized_q, "$1\"$2\":").to_string();
    normalized_q = normalized_q.replace("\"\"", "\"");

    // 4. Robust parsing
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&normalized_q) {
        if let Ok(bson_val) = mongodb::bson::to_bson(&json_val) {
            if let Some(parsed_doc) = bson_val.as_document() {
                return Some(parsed_doc.clone());
            }
        }
    }

    // 5. Fallback: Clean trailing commas
    let cleaned_q = re_comma().replace_all(&normalized_q, "$1").to_string();

    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&cleaned_q) {
        if let Ok(bson_val) = mongodb::bson::to_bson(&json_val) {
            if let Some(parsed_doc) = bson_val.as_document() {
                return Some(parsed_doc.clone());
            }
        }
    }

    None
}

/// Convert a query string (JSON expression or plain text/regex) into a MongoDB match Document
/// suitable for item collections (matches either `regular` text or `refine_result.regular.code`).
pub fn code_or_query_filter(query: &str) -> Document {
    if let Some(parsed) = parse_query_to_bson(query) {
        return parsed;
    }
    let escaped = regex::escape(query.trim());
    doc! {
        "$or": [
            { "regular": { "$regex": &escaped, "$options": "i" } },
            { "selection": { "$regex": &escaped, "$options": "i" } },
            { "combo": { "$regex": &escaped, "$options": "i" } },
            { "refine_result.regular.code": { "$regex": &escaped, "$options": "i" } },
            { "alphaId": { "$regex": &escaped, "$options": "i" } }
        ]
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    DateCreated,
    DateSubmitted,
    Sharpe,
    Returns,
    Turnover,
    Fitness,
    Margin,
    PnlScore,
    Drawdown,
    IsScore,
    OsSharpe,
    OsFitness,
    SubUniverseSharpe,
}

impl SortField {
    pub fn mongo_field(&self) -> &'static str {
        match self {
            Self::DateCreated => "dateCreated",
            Self::DateSubmitted => "dateSubmitted",
            Self::Sharpe => "is.sharpe",
            Self::Returns => "is.returns",
            Self::Turnover => "is.turnover",
            Self::Fitness => "is.fitness",
            Self::Margin => "is.margin",
            Self::PnlScore => "pnl_score",
            Self::Drawdown => "is.drawdown",
            Self::IsScore => "is_score",
            Self::OsSharpe => "os.sharpe",
            Self::OsFitness => "os.fitness",
            Self::SubUniverseSharpe => "is.sub_universe_sharpe",
        }
    }
}

/// Map category name to all variants/aliases used across Brain databases.
pub fn category_aliases(cat: &str) -> Vec<String> {
    let base: Vec<&str> = match cat.trim().to_uppercase().as_str() {
        "INSIDER" | "INSIDERS" => vec!["INSIDER", "INSIDERS"],
        "INSTITUTION" | "INSTITUTIONS" => vec!["INSTITUTION", "INSTITUTIONS"],
        "SHORTINTEREST" | "SHORT_INTEREST" => vec!["SHORT_INTEREST", "SHORTINTEREST", "Short Interest"],
        "SOCIALMEDIA" | "SOCIAL_MEDIA" => vec!["SOCIAL_MEDIA", "SOCIALMEDIA", "Social Media"],
        "PV" | "PRICEVOLUME" | "PRICE_VOLUME" => vec!["PV", "Price Volume"],
        _ => vec![cat.trim()],
    };
    base.into_iter().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::Bson;

    #[test]
    fn parses_valid_json() {
        let q = r#"{"is.sharpe": {"$gte": 1.2}}"#;
        let doc = parse_query_to_bson(q).expect("should parse");
        assert!(doc.contains_key("is.sharpe"));
    }

    #[test]
    fn parses_python_literals_and_smart_quotes() {
        let q = r#"{“status”: “success”, active: True, value: None}"#;
        let doc = parse_query_to_bson(q).expect("should parse");
        assert_eq!(doc.get_str("status").unwrap(), "success");
        assert_eq!(doc.get_bool("active").unwrap(), true);
        assert_eq!(doc.get("value").unwrap(), &Bson::Null);
    }

    #[test]
    fn parses_unquoted_wrapping() {
        let q = r#"is.sharpe: 1.5, is.fitness: 0.8"#;
        let doc = parse_query_to_bson(q).expect("should parse");
        assert_eq!(doc.get_f64("is.sharpe").unwrap(), 1.5);
        assert_eq!(doc.get_f64("is.fitness").unwrap(), 0.8);
    }

    #[test]
    fn falls_back_to_code_or_query_regex() {
        let q = "ts_decay_linear(close, 10)";
        let filter = code_or_query_filter(q);
        assert!(filter.contains_key("$or"));
    }
}
