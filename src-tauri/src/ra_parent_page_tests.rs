//! Database-free regression tests for RA parent qualification before paging.
//! Wire this file as a test-only child module of `datas` with `#[path]`, then run
//! `cargo test --bin quantnight-frontend datas::ra_parent_page_tests`.
//! These tests inspect generated BSON; they do not execute a MongoDB aggregation.
use super::*;

fn lookup(pipeline: &[Document]) -> &Document {
    pipeline[1].get_document("$lookup").unwrap()
}

fn stages<'a>(document: &'a Document, key: &str) -> Vec<&'a Document> {
    document
        .get_array(key)
        .unwrap()
        .iter()
        .map(|stage| {
            stage
                .as_document()
                .expect("pipeline stages must be documents")
        })
        .collect()
}

fn integer(document: &Document, key: &str) -> i64 {
    match document.get(key).unwrap() {
        Bson::Int32(value) => i64::from(*value),
        Bson::Int64(value) => *value,
        value => panic!("{key} must be a BSON integer, got {value:?}"),
    }
}

#[test]
fn qualification_and_cleanup_precede_both_paging_and_total_count() {
    let filter = doc! {
        "$and": [
            { "type": "RA_PARENT" },
            { "settings.region": "USA" },
            { "$or": [{ "id": "parent" }, { "children": "child" }] }
        ]
    };
    let exclusions = vec!["LOW_SHARPE".to_string()];
    let sort = doc! { "dateCreated": -1 };
    let pipeline = ra_parent_page_pipeline(
        filter.clone(),
        "alpha_results",
        Some(&exclusions),
        sort.clone(),
        100,
        50,
    );

    assert_eq!(pipeline.len(), 5);
    let operators: Vec<&str> = pipeline
        .iter()
        .map(|stage| {
            assert_eq!(stage.len(), 1, "each stage must have one operator");
            stage.keys().next().unwrap().as_str()
        })
        .collect();
    assert_eq!(
        operators,
        ["$match", "$lookup", "$match", "$project", "$facet"]
    );
    assert_eq!(pipeline[0], doc! { "$match": filter });
    assert_eq!(
        lookup(&pipeline).get_str("as").unwrap(),
        "_matchingRaChildren"
    );
    assert_eq!(
        pipeline[2],
        doc! { "$match": { "_matchingRaChildren.1": { "$exists": true } } }
    );
    assert_eq!(
        pipeline[3],
        doc! { "$project": { "_matchingRaChildren": 0 } }
    );

    let facet = pipeline[4].get_document("$facet").unwrap();
    assert_eq!(facet.len(), 2);
    let data = stages(facet, "data");
    assert_eq!(data.len(), 4);
    assert_eq!(data[0], &doc! { "$sort": sort });
    assert_eq!(integer(data[1], "$skip"), 100);
    assert_eq!(integer(data[2], "$limit"), 50);
    assert_eq!(data[3], &doc! { "$project": get_alpha_projection() });
    // Total sees exactly the qualified parent stream, without sort/skip/limit.
    assert_eq!(stages(facet, "total"), vec![&doc! { "$count": "count" }]);
}

#[test]
fn lookup_counts_existing_child_documents_not_original_children_array_length() {
    let pipeline = ra_parent_page_pipeline(
        doc! { "type": "RA_PARENT" },
        "alpha_results",
        None,
        doc! { "id": 1 },
        0,
        10,
    );
    let join = lookup(&pipeline);
    assert_eq!(join.len(), 5);
    assert_eq!(join.get_str("localField").unwrap(), "children");
    assert_eq!(join.get_str("foreignField").unwrap(), "id");
    assert!(!join.contains_key("let"));
    let children = stages(join, "pipeline");
    assert_eq!(children.len(), 3);
    assert_eq!(
        children[0],
        &doc! { "$match": { "id": { "$type": "string" } } }
    );
    assert_eq!(children[1].len(), 1);
    assert_eq!(integer(children[1], "$limit"), 2);
    assert_eq!(children[2].len(), 1);
    let projection = children[2].get_document("$project").unwrap();
    assert_eq!(integer(projection, "id"), 1);
    // `_id` may be explicitly suppressed, but no other child data is needed.
    assert!(projection.keys().all(|key| key == "id" || key == "_id"));
    if projection.contains_key("_id") {
        assert_eq!(integer(projection, "_id"), 0);
    }

    // Missing IDs and repeated references cannot manufacture a second joined
    // document. The second lookup result, not `children.1` or `$size`, qualifies
    // the parent. The exact outer matches rule out a children-length-only gate.
    let matches: Vec<&Document> = pipeline
        .iter()
        .filter_map(|stage| stage.get_document("$match").ok())
        .collect();
    assert_eq!(
        matches,
        vec![
            &doc! { "type": "RA_PARENT" },
            &doc! { "_matchingRaChildren.1": { "$exists": true } }
        ]
    );
}

#[test]
fn absent_and_empty_exclusions_produce_identical_unfiltered_child_lookup() {
    let build = |excluded| {
        ra_parent_page_pipeline(
            doc! {},
            "alpha_results",
            excluded,
            doc! { "dateCreated": -1 },
            0,
            50,
        )
    };
    let absent = build(None);
    let empty: Vec<String> = vec![];
    assert_eq!(absent, build(Some(empty.as_slice())));
    let children = stages(lookup(&absent), "pipeline");
    assert_eq!(children.len(), 3);
    assert_eq!(
        children[0],
        &doc! { "$match": { "id": { "$type": "string" } } }
    );
    assert_eq!(integer(children[1], "$limit"), 2);
    assert!(children[2].contains_key("$project"));
}

#[test]
fn exclusions_match_exact_names_and_fail_or_warning_in_the_same_check_before_limit() {
    for exclusions in [
        vec!["LOW_ROBUST_UNIVERSE_RETURNS".to_string()],
        vec![
            "LOW_ROBUST_UNIVERSE_RETURNS".to_string(),
            "LOW_FITNESS".to_string(),
        ],
    ] {
        let pipeline = ra_parent_page_pipeline(
            doc! { "type": "RA_PARENT" },
            "alpha_results",
            Some(&exclusions),
            doc! { "id": 1 },
            0,
            50,
        );
        // Exclusions apply only to joined children, never to parent checks.
        assert_eq!(pipeline[0], doc! { "$match": { "type": "RA_PARENT" } });
        let children = stages(lookup(&pipeline), "pipeline");
        assert_eq!(children.len(), 4);
        assert_eq!(
            children[0],
            &doc! { "$match": { "id": { "$type": "string" } } }
        );
        // `$in` uses literal names (no regex/substrings/case folding). Keeping
        // name/result inside one `$elemMatch` prevents a PASS for an excluded
        // name and an unrelated FAIL from jointly excluding the child.
        assert_eq!(
            children[1],
            &doc! {
                "$match": {
                    "is.checks": {
                        "$not": {
                            "$elemMatch": {
                                "name": { "$in": &exclusions },
                                "result": { "$in": ["FAIL", "WARNING"] }
                            }
                        }
                    }
                }
            }
        );
        assert_eq!(children[2].len(), 1);
        assert_eq!(integer(children[2], "$limit"), 2);
        assert!(children[3].contains_key("$project"));
    }
}

#[test]
fn lookup_uses_the_requested_collection() {
    for collection in ["alpha_results", "alpha_results_archive_2025"] {
        let pipeline = ra_parent_page_pipeline(doc! {}, collection, None, doc! { "id": 1 }, 0, 1);
        assert_eq!(lookup(&pipeline).get_str("from").unwrap(), collection);
    }
}

#[test]
fn data_facet_preserves_sort_order_and_paging_numbers_without_narrowing() {
    for (sort, skip, page_size) in [
        (doc! { "dateCreated": -1 }, 0_u64, 1_u32),
        (doc! { "is.sharpe": 1, "id": -1 }, 150, 50),
        (
            doc! { "os.fitness": -1, "id": 1 },
            u64::from(u32::MAX) + 1,
            u32::MAX,
        ),
    ] {
        let pipeline = ra_parent_page_pipeline(
            doc! {},
            "alpha_results",
            None,
            sort.clone(),
            skip,
            page_size,
        );
        let facet = pipeline.last().unwrap().get_document("$facet").unwrap();
        let data = stages(facet, "data");
        assert_eq!(data.len(), 4);
        assert_eq!(data[0], &doc! { "$sort": &sort });
        // Compound sort key order is significant, unlike document equality.
        let actual_sort = data[0].get_document("$sort").unwrap();
        assert_eq!(
            actual_sort.iter().collect::<Vec<_>>(),
            sort.iter().collect::<Vec<_>>()
        );
        assert_eq!(data[1].len(), 1);
        assert_eq!(integer(data[1], "$skip"), i64::try_from(skip).unwrap());
        assert_eq!(data[2].len(), 1);
        assert_eq!(integer(data[2], "$limit"), i64::from(page_size));
        assert_eq!(data[3], &doc! { "$project": get_alpha_projection() });
        assert_eq!(stages(facet, "total"), vec![&doc! { "$count": "count" }]);
    }
}
