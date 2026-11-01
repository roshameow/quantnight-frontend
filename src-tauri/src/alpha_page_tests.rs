//! Database-free tests for the shared page/count filter and total routing.
//! Run: cargo test --offline --locked --bin quantnight-frontend datas::
use super::*;
use serde_json::json;

fn query(value: serde_json::Value) -> AlphaQuery {
    serde_json::from_value(value).unwrap()
}

#[test]
fn omitted_and_null_include_total_preserve_legacy_clients() {
    for value in [json!({}), json!({"include_total": null})] {
        let params = query(value);
        assert_eq!(params.include_total, None);
        assert!(should_include_total(&params));
        assert!(!is_ra_query(&params));
        assert_eq!(build_alpha_filter(&params), doc! {});
    }
}

#[test]
fn only_explicit_false_omits_non_ra_totals() {
    for alpha_type in [None, Some("REGULAR"), Some("SUPER"), Some("RA_CHILD")] {
        for include_total in [None, Some(false), Some(true)] {
            let params = query(json!({
                "alpha_type": alpha_type, "include_total": include_total
            }));
            assert!(!is_ra_query(&params));
            assert_eq!(should_include_total(&params), include_total != Some(false));
        }
    }
}

#[test]
fn ra_and_ra_parent_always_keep_eligibility_totals() {
    for alpha_type in ["RA", "RA_PARENT"] {
        for include_total in [None, Some(false), Some(true)] {
            let params = query(json!({
                "alpha_type": alpha_type, "include_total": include_total,
                "id": "child-or-parent", "messages_nin": ["LOW_SHARPE"]
            }));
            assert!(is_ra_query(&params));
            assert!(should_include_total(&params));
            let filter = build_alpha_filter(&params);
            // ID routing and child-only exclusions must survive extraction.
            assert_eq!(
                filter,
                doc! { "$and": [
                    { "$or": [{ "id": "child-or-parent" }, { "children": "child-or-parent" }] },
                    { "type": "RA_PARENT" }
                ] }
            );
            let pipeline = ra_parent_page_pipeline(
                filter,
                "alpha_results",
                params.messages_nin.as_deref(),
                doc! { "dateCreated": -1 },
                0,
                50,
            );
            assert_eq!(
                pipeline[4]
                    .get_document("$facet")
                    .unwrap()
                    .get_array("total")
                    .unwrap(),
                &vec![Bson::Document(doc! { "$count": "count" })]
            );
        }
    }
}

#[test]
fn combined_non_ra_filter_preserves_all_static_predicates() {
    let params = query(json!({
        "query": " rank(foo) ", "exclude_query": " bad.* ", "id": "exact-id",
        "messages_in": ["LOW_SHARPE"], "messages_nin": ["LOW_FITNESS"],
        "classifications_in": ["POWER_POOL"], "region": "USA",
        "universe": " TOP(3000) ", "delay": 1,
        "min_turnover": 0.1, "max_turnover": 0.5,
        "min_margin": 0.001, "min_returns": 0.05, "alpha_type": "REGULAR"
    }));
    assert_eq!(
        build_alpha_filter(&params),
        doc! { "$and": [
            { "$or": [
                { "regular.code": { "$regex": "rank(foo)", "$options": "i" } },
                { "selection.code": { "$regex": "rank(foo)", "$options": "i" } },
                { "combo.code": { "$regex": "rank(foo)", "$options": "i" } }
            ] },
            { "$and": [
                { "regular.code": { "$not": { "$regex": "bad.*", "$options": "i" } } },
                { "selection.code": { "$not": { "$regex": "bad.*", "$options": "i" } } },
                { "combo.code": { "$not": { "$regex": "bad.*", "$options": "i" } } }
            ] },
            { "is.checks": { "$elemMatch": {
                "name": { "$in": ["LOW_SHARPE"] }, "result": { "$in": ["FAIL", "WARNING"] }
            } } },
            { "is.checks": { "$not": { "$elemMatch": {
                "name": { "$in": ["LOW_FITNESS"] }, "result": { "$in": ["FAIL", "WARNING"] }
            } } } },
            { "classifications": { "$elemMatch": { "id": { "$in": ["POWER_POOL"] } } } },
            { "id": "exact-id" }, { "settings.region": "USA" },
            { "settings.universe": { "$regex": "TOP\\(3000\\)", "$options": "i" } },
            { "settings.delay": 1_i32 },
            { "is.turnover": { "$gte": 0.1 } }, { "is.turnover": { "$lte": 0.5 } },
            { "$expr": { "$gte": [{ "$abs": "$is.margin" }, 0.001] } },
            { "$expr": { "$gte": [{ "$abs": "$is.returns" }, 0.05] } },
            { "type": "REGULAR" }
        ] }
    );
}

#[test]
fn structured_queries_are_not_replaced_by_regex() {
    let params = query(json!({"query": "{\"is.sharpe\": {\"$gte\": 2.5}}"}));
    assert_eq!(
        build_alpha_filter(&params),
        doc! { "$and": [{ "is.sharpe": { "$gte": 2.5 } }] }
    );
}

#[test]
fn empty_optional_filters_do_not_add_predicates() {
    let params = query(json!({
        "query": "  ", "exclude_query": "  ", "universe": "  ",
        "messages_in": [], "messages_nin": [], "classifications_in": [], "self_categories": []
    }));
    assert_eq!(build_alpha_filter(&params), doc! {});
}

#[test]
fn category_aliases_keep_and_between_categories_and_or_within_aliases() {
    let params =
        query(json!({"self_categories": [" INSTITUTION ", "SHORT_INTEREST", "PV", "A+B"]}));
    let expected_categories: Vec<Document> = [
        vec!["INSTITUTION$", "INSTITUTIONS$"],
        vec!["SHORT_INTEREST$", "SHORTINTEREST$", "Short Interest$"],
        vec!["PV$", "Price Volume$"],
        vec!["A\\+B$"],
    ]
    .iter()
    .map(|patterns| {
        let aliases: Vec<Document> = patterns
            .iter()
            .map(|pattern| {
                doc! {
                    "is.checks": { "$elemMatch": {
                        "name": "MATCHES_PYRAMID", "pyramids": { "$elemMatch": {
                            "name": { "$regex": *pattern, "$options": "i" }
                        } }
                    } }
                }
            })
            .collect();
        doc! { "$or": aliases }
    })
    .collect();
    assert_eq!(
        build_alpha_filter(&params),
        doc! { "$and": [{ "$and": expected_categories }] }
    );
}

#[test]
fn date_window_retains_rfc3339_string_cutoff() {
    let before = Utc::now() - Duration::days(7);
    let filter = build_alpha_filter(&query(json!({"days_within": 7})));
    let after = Utc::now() - Duration::days(7);
    let cutoff = filter.get_array("$and").unwrap()[0]
        .as_document()
        .unwrap()
        .get_document("dateCreated")
        .unwrap()
        .get_str("$gte")
        .unwrap();
    let cutoff = DateTime::parse_from_rfc3339(cutoff)
        .unwrap()
        .with_timezone(&Utc);
    assert!(before <= cutoff && cutoff <= after);
}

#[test]
fn count_and_page_controls_do_not_change_the_shared_filter() {
    let base = json!({"id": "a", "region": "USA", "alpha_type": "REGULAR", "self_categories": ["SOCIAL_MEDIA"]});
    let expected = build_alpha_filter(&query(base.clone()));
    for include_total in [None, Some(false), Some(true)] {
        let mut value = base.clone();
        value["include_total"] = json!(include_total);
        value["page"] = json!(4);
        value["page_size"] = json!(20);
        value["sort_field"] = json!("sharpe");
        value["sort_order"] = json!(1);
        value["embedding"] = json!("umap");
        value["collection"] = json!("archive_2025");
        assert_eq!(build_alpha_filter(&query(value)), expected);
    }
}

#[test]
fn both_commands_share_the_same_collection_sanitization() {
    for (collection, expected) in [
        (None, "alpha_results"),
        (Some("archive_2025"), "archive_2025"),
        (Some("alpha.results"), "alpha_results"),
        (Some("../other"), "alpha_results"),
        (Some("bad$name"), "alpha_results"),
    ] {
        assert_eq!(
            alpha_collection_name(&query(json!({"collection": collection}))),
            expected
        );
    }
}

#[test]
fn omitted_total_keeps_the_legacy_paged_result_wire_shape() {
    let result = PagedResult::<AlphaResult> {
        data: vec![],
        total: 0,
        page: 2,
        page_size: 50,
    };
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({"data": [], "total": 0, "page": 2, "page_size": 50})
    );
}


#[test]
fn empty_filter_count_uses_id_index_without_changing_other_options() {
    for seconds in [None, Some(30)] {
        let options = alpha_count_options(&doc! {}, CountOptions::builder()
            .max_time(seconds.map(std::time::Duration::from_secs))
            .build());
        assert_eq!(options.hint, Some(Hint::Name("_id_".to_string())));
        assert_eq!(options.max_time, seconds.map(std::time::Duration::from_secs));
        assert_eq!(options.skip, None);
        assert_eq!(options.limit, None);
        assert!(options.collation.is_none());
        assert!(options.read_concern.is_none());
        assert!(options.selection_criteria.is_none());
        assert!(options.comment.is_none());
    }
}

#[test]
fn only_literal_empty_filters_get_the_count_hint() {
    for value in [json!({}), json!({"query": "  ", "messages_in": []})] {
        let filter = build_alpha_filter(&query(value));
        assert!(filter.is_empty());
        assert_eq!(alpha_count_options(&filter, CountOptions::default()).hint,
            Some(Hint::Name("_id_".to_string())));
    }
    for value in [
        json!({"query": "close"}), json!({"exclude_query": "volume"}),
        json!({"query": "{}"}), json!({"query": "{\"$and\": []}"}),
        json!({"id": "a"}), json!({"region": "USA"}), json!({"delay": 0}),
        json!({"alpha_type": "REGULAR"}), json!({"min_margin": 0.001}),
        json!({"messages_nin": ["LOW_SHARPE"]}),
    ] {
        let filter = build_alpha_filter(&query(value));
        assert!(!filter.is_empty());
        let options = alpha_count_options(&filter, CountOptions::default());
        assert_eq!(options.hint, None, "nonempty filter: {filter:?}");
        assert_eq!(options.max_time, None);
    }
    // Do not simplify semantic tautologies into empty filters.
    for filter in [doc! {"$and": []}, doc! {"$expr": true}] {
        assert_eq!(alpha_count_options(&filter, CountOptions::default()).hint, None);
    }
}

#[test]
fn nonempty_count_preserves_existing_caller_options() {
    let options = CountOptions::builder()
        .hint(Hint::Name("caller_index".to_string()))
        .max_time(std::time::Duration::from_secs(30))
        .skip(2_u64).limit(7_u64).comment(Bson::String("count-test".into()))
        .build();
    let actual = alpha_count_options(&doc! {"settings.region": "USA"}, options);
    assert_eq!(actual.hint, Some(Hint::Name("caller_index".to_string())));
    assert_eq!(actual.max_time, Some(std::time::Duration::from_secs(30)));
    assert_eq!(actual.skip, Some(2));
    assert_eq!(actual.limit, Some(7));
    assert_eq!(actual.comment, Some(Bson::String("count-test".into())));
}

// Source-route guards supplement executable options/filter tests. They prove
// both real count call sites pass these options, not live Mongo/Tauri execution.
#[test]
fn actual_count_callers_pass_scoped_options_and_keep_legacy_routing() {
    let source = include_str!("datas.rs");
    let count = source.split("pub async fn get_alpha_count(").nth(1).unwrap()
        .split("pub async fn get_alpha_results(").next().unwrap();
    let page = source.split("pub async fn get_alpha_results(").nth(1).unwrap()
        .split("pub struct PnlPoint").next().unwrap();
    let compact = |s: &str| s.split_whitespace().collect::<String>();
    let count = compact(count);
    let page = compact(page);
    assert!(count.contains("ifis_ra_query(&params){returnErr("));
    assert!(count.contains("letfilter=build_alpha_filter(&params);"));
    assert!(count.contains("letoptions=alpha_count_options(&filter,CountOptions::builder().max_time(std::time::Duration::from_secs(30)).build());"));
    assert!(count.contains("collection.count_documents(filter,options).await.map_err(|e|format!(\"DBcounterror:{e}\"))"));
    assert_eq!(count.matches(".count_documents(").count(), 1);
    assert!(page.contains("ifshould_include_total(&params){let(count_res,cursor_res)=join!(collection.count_documents(filter.clone(),alpha_count_options(&filter,CountOptions::default())),collection.find(filter,find_options));"));
    assert!(page.contains("(count_res.unwrap_or(0),cursor_res)"));
    assert!(page.contains("(0,collection.find(filter,find_options).await)"));
    let ra = page.split("ifis_ra_query{").nth(1).unwrap().split("}else{").next().unwrap();
    assert!(ra.contains("collection.aggregate(pipeline,None).await"));
    assert!(!ra.contains("alpha_count_options"));
    assert_eq!(page.matches(".count_documents(").count(), 1);
    for caller in [&count, &page] {
        assert!(!caller.contains("estimated_document_count"));
    }
}
