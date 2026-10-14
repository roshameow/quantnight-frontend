//! Read-only, database-free regression tests for RA child exclusion/aggregation.
//! Run the binary target: `cargo test --bin quantnight-frontend datas::ra_message_tests`.
//! The library target does not include datas.rs. These helper tests do not prove
//! parent Mongo filter construction or the independently fetched PnL chart path.
use super::*;

const EXCLUDED: &str = "LOW_ROBUST_UNIVERSE_RETURNS";

fn child(id: &str, metric: i32, failures: &[(&str, &str)]) -> Document {
    let mut checks: Vec<Document> = failures
        .iter()
        .map(|(name, result)| doc! { "name": *name, "result": *result })
        .collect();
    checks.push(doc! {
        "name": "LOW_SUB_UNIVERSE_SHARPE", "result": "PASS", "value": metric
    });
    doc! {
        "id": id,
        "type": "RA_CHILD",
        "parent": "parent",
        "settings": { "region": "USA" },
        "regular": { "code": id },
        "is": {
            "sharpe": metric, "fitness": metric, "returns": metric,
            "turnover": metric, "margin": metric, "drawdown": metric,
            "longCount": metric, "shortCount": metric * 2,
            "checks": checks
        },
        "os": { "sharpe": metric, "fitness": metric },
        "pnl_score": metric
    }
}

fn parent(ids: &[&str]) -> AlphaResult {
    let mut document = child("parent", 99, &[(EXCLUDED, "FAIL")]);
    document.insert("type", "RA_PARENT");
    document.remove("parent");
    document.insert(
        "children",
        ids.iter()
            .map(|id| Bson::String((*id).into()))
            .collect::<Vec<_>>(),
    );
    parse_alpha_document(document, None).unwrap()
}

fn attach(
    parent: AlphaResult,
    documents: Vec<Document>,
    excluded: Option<&[String]>,
) -> AlphaResult {
    let mut child_map = HashMap::new();
    let mut failure_map = HashMap::new();
    for document in documents {
        if let Some((child, failures)) = parse_ra_child(document, None, excluded) {
            failure_map.insert(child.id.clone(), failures);
            child_map.insert(child.id.clone(), child);
        }
    }
    let mut results = vec![parent];
    attach_ra_children(&mut results, &child_map, &failure_map);
    assert_eq!(results.len(), 1, "empty-child parents must not disappear");
    results.remove(0)
}

fn three_children() -> Vec<Document> {
    vec![
        child("first", 2, &[("LOW_SHARPE", "FAIL")]),
        child(
            "excluded",
            100,
            &[(EXCLUDED, "FAIL"), ("LOW_FITNESS", "FAIL")],
        ),
        child(
            "last",
            4,
            &[("LOW_SHARPE", "WARNING"), ("LOW_FITNESS", "FAIL")],
        ),
    ]
}

fn ids(result: &AlphaResult) -> Vec<&str> {
    result
        .children
        .as_ref()
        .unwrap()
        .iter()
        .map(|child| child.id.as_str())
        .collect()
}

fn assert_metrics(result: &AlphaResult, mean: f64, max_drawdown: f64, long_count: i32) {
    for metric in [
        result.sharpe,
        result.fitness,
        result.returns,
        result.turnover,
        result.margin,
        result.pnl_score,
        result.sub_universe_sharpe,
        result.os_sharpe,
        result.os_fitness,
    ] {
        assert_eq!(metric, Some(mean));
    }
    assert_eq!(result.drawdown, Some(max_drawdown));
    assert_eq!(result.long_count, Some(long_count));
    assert_eq!(result.short_count, Some(long_count * 2));
}

#[test]
fn three_children_become_two_with_retained_statistics_and_messages() {
    let exclusions = vec![EXCLUDED.to_string()];
    let result = attach(
        parent(&["first", "excluded", "last"]),
        three_children(),
        Some(&exclusions),
    );
    assert_eq!(ids(&result), vec!["first", "last"]);
    assert_metrics(&result, 3.0, 4.0, 6);
    assert_eq!(
        result.message.as_deref(),
        Some("LOW_SHARPE(2), LOW_FITNESS(1)")
    );
    // Identity metadata stays intact; the visible badge must count children instead.
    assert_eq!(result.children_ids.as_ref().unwrap().len(), 3);
    assert_eq!(result.children.as_ref().unwrap().len(), 2);
    assert_eq!(result.alpha_type.as_deref(), Some("RA_PARENT"));
}

#[test]
fn pass_keeps_child_but_warning_and_fail_exclude_it() {
    let exclusions = vec![EXCLUDED.to_string()];
    for status in ["FAIL", "WARNING"] {
        assert!(parse_ra_child(
            child("bad", 1, &[(EXCLUDED, status)]),
            None,
            Some(&exclusions)
        )
        .is_none());
    }
    let (retained, failures) = parse_ra_child(
        child("pass", 1, &[(EXCLUDED, "PASS"), ("LOW_SHARPE", "WARNING")]),
        None,
        Some(&exclusions),
    )
    .unwrap();
    assert_eq!(retained.id, "pass");
    assert_eq!(failures, vec!["LOW_SHARPE"]);
}

#[test]
fn multiple_exclusions_match_exact_names_and_the_same_check() {
    let exclusions = vec![EXCLUDED.to_string(), "LOW_FITNESS".to_string()];
    for name in [EXCLUDED, "LOW_FITNESS"] {
        assert!(
            parse_ra_child(child("bad", 1, &[(name, "FAIL")]), None, Some(&exclusions)).is_none()
        );
    }
    // An excluded PASS plus another check's FAIL must not match across elements.
    let (_, failures) = parse_ra_child(
        child(
            "keep",
            1,
            &[
                (EXCLUDED, "PASS"),
                ("LOW_ROBUST_UNIVERSE_RETURNS_EXTRA", "FAIL"),
                ("low_fitness", "WARNING"),
            ],
        ),
        None,
        Some(&exclusions),
    )
    .unwrap();
    assert_eq!(
        failures,
        vec!["LOW_ROBUST_UNIVERSE_RETURNS_EXTRA", "low_fitness"]
    );
}

#[test]
fn all_excluded_keeps_parent_with_explicit_empty_children_and_cleared_metrics() {
    let exclusions = vec![EXCLUDED.to_string()];
    let result = attach(
        parent(&["a", "b"]),
        vec![
            child("a", 2, &[(EXCLUDED, "FAIL")]),
            child("b", 4, &[(EXCLUDED, "WARNING")]),
        ],
        Some(&exclusions),
    );
    assert!(ids(&result).is_empty());
    for metric in [
        result.sharpe,
        result.fitness,
        result.returns,
        result.turnover,
        result.margin,
        result.drawdown,
        result.pnl_score,
        result.sub_universe_sharpe,
        result.os_sharpe,
        result.os_fitness,
    ] {
        assert_eq!(metric, None);
    }
    assert_eq!(result.long_count, None);
    assert_eq!(result.short_count, None);
    assert_eq!(result.message, None);
    let serialized = serde_json::to_value(&result).unwrap();
    assert_eq!(serialized["children"], serde_json::json!([]));
    assert_eq!(serialized["id"], "parent");
}

#[test]
fn absent_and_empty_exclusions_preserve_original_three_children() {
    let empty: Vec<String> = vec![];
    for exclusions in [None, Some(empty.as_slice())] {
        let result = attach(
            parent(&["first", "excluded", "last"]),
            three_children(),
            exclusions,
        );
        assert_eq!(ids(&result), vec!["first", "excluded", "last"]);
        assert_metrics(&result, 106.0 / 3.0, 100.0, 106);
        assert_eq!(
            result.message.as_deref(),
            Some("LOW_FITNESS(2), LOW_SHARPE(2), LOW_ROBUST_UNIVERSE_RETURNS(1)")
        );
    }
}

#[test]
fn absent_checks_and_absent_is_do_not_exclude_children() {
    let exclusions = vec![EXCLUDED.to_string()];
    let mut no_checks = child("no_checks", 1, &[]);
    no_checks.get_document_mut("is").unwrap().remove("checks");
    let mut no_is = child("no_is", 1, &[]);
    no_is.remove("is");
    for document in [no_checks, no_is] {
        let (_, failures) = parse_ra_child(document, None, Some(&exclusions)).unwrap();
        assert!(failures.is_empty());
    }
}

#[test]
fn passing_retained_children_clear_stale_parent_failure_message() {
    let exclusions = vec![EXCLUDED.to_string()];
    let result = attach(
        parent(&["pass"]),
        vec![child("pass", 2, &[(EXCLUDED, "PASS")])],
        Some(&exclusions),
    );
    assert_eq!(ids(&result), vec!["pass"]);
    assert_eq!(result.message, None);
    assert_metrics(&result, 2.0, 2.0, 2);
}

#[test]
fn parent_with_originally_empty_children_serializes_explicit_empty_array() {
    let result = attach(parent(&[]), vec![], None);
    assert!(ids(&result).is_empty());
    assert_eq!(result.message, None);
    assert_eq!(result.sharpe, None);
    assert_eq!(
        serde_json::to_value(result).unwrap()["children"],
        serde_json::json!([])
    );
}

#[test]
fn retained_children_without_metrics_do_not_keep_stale_parent_metrics() {
    let result = attach(
        parent(&["missing"]),
        vec![doc! { "id": "missing", "type": "RA_CHILD" }],
        Some(&[EXCLUDED.to_string()]),
    );
    assert_eq!(ids(&result), vec!["missing"]);
    assert_eq!(result.sharpe, None);
    assert_eq!(result.fitness, None);
    assert_eq!(result.returns, None);
    assert_eq!(result.margin, None);
    assert_eq!(result.long_count, None);
    assert_eq!(result.message, None);
}
