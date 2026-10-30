use bson::{doc, Bson, Document};
use mongodb::{
    options::{Hint, IndexOptions},
    IndexModel,
};
use quantnight_task_core::task_store::*;
use std::time::Duration;

#[test]
fn list_is_projected_newest_first_without_item_payload_or_candidate_cap() {
    let o = list_tasks_options();
    assert_eq!(o.sort, Some(doc! {"_id":-1}));
    assert_eq!(o.limit, None);
    assert_eq!(
        o.projection,
        Some(
            doc! {"name":1,"status":1,"taskType":1,"auth_profile":1,"isRemote":1,"targetRemote":1,"templatefile":1,"lastError":1,"generation":1}
        )
    );
}
#[test]
fn results_boundary_page_is_small_id_sorted_projection_not_full_payload() {
    for (limit, offset) in [(1, 0), (20, 17), (200, 10000)] {
        let o = result_items_options(limit, offset).unwrap();
        assert_eq!(o.limit, Some(limit + 1));
        assert_eq!(o.skip, Some(offset as u64));
        assert_eq!(o.sort, Some(doc! {"_id":1}));
        assert_eq!(o.max_time, Some(Duration::from_secs(5)));
        assert_eq!(
            o.projection,
            Some(
                doc! {"_id":1,"alphaId":1,"status":1,"variantId":1,"variantHash":1,"candidateHash":1,"ideaId":1,"payloadHash":1,"acceptance":1,"terminalStatus":1}
            )
        );
    }
    for (limit, offset) in [(0, 0), (-1, 0), (201, 0), (1, -1), (1, 10001)] {
        assert!(result_items_options(limit, offset).is_err());
    }
}
#[test]
fn canonical_index_guard_first_key_ascending_visible_nonpartial_and_lexical_name() {
    let invalid = vec![
        doc! {"name":"_id_","key":{"_id":1}},
        doc! {"name":"id_desc","key":{"id":-1}},
        doc! {"name":"not_leading","key":{"tag":1,"id":1}},
        doc! {"name":"hidden","key":{"id":1},"hidden":true},
        doc! {"name":"partial","key":{"id":1},"partialFilterExpression":{"is.fitness":{"$gt":0}}},
    ];
    assert!(canonical_id_hint(&invalid)
        .unwrap_err()
        .contains("refusing a full collection scan"));
    let mut indexes = invalid;
    indexes.extend([
        doc! {"name":"z_compound","key":{"id":1,"type":1}},
        doc! {"name":"a_id","key":{"id":1},"hidden":false},
    ]);
    assert_eq!(canonical_id_hint(&indexes).unwrap(), "a_id");
    let o = canonical_result_options(canonical_id_hint(&indexes).unwrap(), 200).unwrap();
    assert!(matches!(o.hint,Some(Hint::Name(ref name)) if name=="a_id"));
    assert_eq!(o.limit, Some(200));
    assert_eq!(o.max_time, Some(Duration::from_secs(5)));
    assert_eq!(o.sort, None);
    assert_eq!(o.skip, None);
    assert_eq!(
        o.projection,
        Some(
            doc! {"_id":0,"id":1,"type":1,"status":1,"dateCreated":1,"dateModified":1,"updated_at":1,"settings.region":1,"settings.universe":1,"settings.delay":1,"is.sharpe":1,"is.fitness":1,"is.margin":1,"is.turnover":1,"is.returns":1,"is.drawdown":1}
        )
    );
    assert!(canonical_result_options("id_1".into(), 0).is_err());
    assert!(canonical_result_options("id_1".into(), 201).is_err());
}
#[test]
fn real_mongodb_index_model_serialization_keeps_guard_metadata_flat() {
    let model = IndexModel::builder()
        .keys(doc! {"id":1,"type":1})
        .options(
            IndexOptions::builder()
                .name("fixture".to_string())
                .hidden(false)
                .build(),
        )
        .build();
    let serialized = bson::to_document(&model).unwrap();
    assert_eq!(
        serialized.get_document("key").unwrap(),
        &doc! {"id":1,"type":1}
    );
    assert_eq!(canonical_id_hint(&[serialized]).unwrap(), "fixture");
    let model = IndexModel::builder()
        .keys(doc! {"id":1})
        .options(
            IndexOptions::builder()
                .name("partial".to_string())
                .partial_filter_expression(doc! {"tag":{"$exists":true}})
                .build(),
        )
        .build();
    assert!(canonical_id_hint(&[bson::to_document(&model).unwrap()]).is_err());
}
#[test]
fn namespace_requires_both_original_mapping_values_without_fallback() {
    assert!(CanonicalNamespace::new("".into(), "alpha_results".into()).is_err());
    assert!(CanonicalNamespace::new("alpha_db".into(), " ".into()).is_err());
    let n =
        CanonicalNamespace::new("profile_specific".into(), "original_collection".into()).unwrap();
    assert_eq!(n.database, "profile_specific");
    assert_eq!(n.collection, "original_collection");
}
#[test]
fn additive_read_api_does_not_change_existing_summary_receipt_or_probe_shapes() {
    assert_eq!(
        summary_pipeline(),
        vec![doc! {"$group":{"_id":{"$ifNull":["$status","missing"]},"count":{"$sum":1}}}]
    );
    assert_eq!(nonempty_options().limit, Some(1));
    assert_eq!(nonempty_options().max_time, Some(Duration::from_secs(10)));
    assert_eq!(never_started_controls_options().limit, Some(2));
    assert_eq!(
        never_started_controls_options().max_time,
        Some(Duration::from_secs(8))
    );
    let p: Document = receipt_projection();
    assert_eq!(p.get("submissionAttempts"), Some(&Bson::Int32(1)));
    assert!(!p.contains_key("payload"));
    assert!(!p.contains_key("result"));
}

#[test]
fn only_explicit_export_is_full_payload_and_preserves_no_total_limit_or_new_deadline() {
    let o = collect_result_options();
    assert_eq!(o.projection, Some(doc! {"_id":0}));
    assert_eq!(o.limit, None);
    assert_eq!(o.skip, None);
    assert_eq!(o.max_time, None);
}
