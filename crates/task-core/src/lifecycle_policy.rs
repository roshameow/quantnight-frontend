//! Pure lifecycle policy ported from taskctl.py and util.dataset_mission.
//!
//! No store, process, configuration, or network access. Callers own locking,
//! snapshots, CAS execution, and error receipts. In particular a local terminal
//! item status is NOT proof of platform termination. Keep BSON types intact:
//! ISO strings are not persisted stop-proof datetimes and missing CAS fields
//! are not interchangeable with explicit null. Checked receipt APIs preserve
//! Python's short-circuit order and TypeError messages for unhashable values.
//! State-changing callers must propagate these errors; the legacy bool APIs
//! only provide conservative compatibility fallbacks.

use mongodb::bson::{doc, Bson, Document};

const TERMINAL_ROW_STATUSES: &[&str] = &[
    "success",
    "permanent_error",
    "timeout_error",
    "cancelled",
    "error",
];

/// Python bool(value) for BSON-decoded values, not JSON's notion of truth.
/// Decimal128, ObjectId, regex and timestamp are objects (always true). Code
/// and Binary inherit Python string/bytes truthiness. Undefined decodes to
/// None, and legacy Symbol decodes to str in PyMongo.
fn truthy(value: Option<&Bson>) -> bool {
    match value {
        None | Some(Bson::Null | Bson::Undefined) => false,
        Some(Bson::Boolean(value)) => *value,
        Some(Bson::Int32(value)) => *value != 0,
        Some(Bson::Int64(value)) => *value != 0,
        Some(Bson::Double(value)) => *value != 0.0, // NaN is true in Python.
        Some(Bson::String(value) | Bson::Symbol(value) | Bson::JavaScriptCode(value)) => {
            !value.is_empty()
        }
        Some(Bson::JavaScriptCodeWithScope(value)) => !value.code.is_empty(),
        Some(Bson::Array(value)) => !value.is_empty(),
        Some(Bson::Document(value)) => !value.is_empty(),
        Some(Bson::Binary(value)) => !value.bytes.is_empty(),
        Some(_) => true,
    }
}

fn is_null(value: Option<&Bson>) -> bool {
    matches!(value, None | Some(Bson::Null | Bson::Undefined))
}

/// Equality to a Python str, as used by the original schema's status fields.
/// BSON Code is deliberately excluded: Code('success') != 'success'.
fn text(value: Option<&Bson>) -> Option<&str> {
    match value {
        Some(Bson::String(value) | Bson::Symbol(value)) => Some(value),
        _ => None,
    }
}

/// Python set membership hashes the queried value before comparing it. Keep
/// this check at the original expression site: an earlier short-circuit may
/// legitimately avoid hashing a malformed field. PyMongo decodes BSON regex,
/// decimal and code to unhashable Regex/Decimal128/Code objects as well.
fn checked_one_of(value: Option<&Bson>, choices: &[&str]) -> Result<bool, String> {
    let unhashable = match value {
        Some(Bson::Array(_)) => Some("list"),
        Some(Bson::Document(_)) => Some("dict"),
        Some(Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_)) => Some("Code"),
        Some(Bson::RegularExpression(_)) => Some("Regex"),
        Some(Bson::Decimal128(_)) => Some("Decimal128"),
        _ => None,
    };
    if let Some(kind) = unhashable {
        return Err(format!("unhashable type: '{kind}'"));
    }
    Ok(text(value).is_some_and(|value| choices.contains(&value)))
}

/// Only these BSON types can stringify to one of the original word tokens.
/// Unlike equality, Python str(Code('UNKNOWN')) is 'UNKNOWN'.
fn upper_token(value: Option<&Bson>, choices: &[&str]) -> bool {
    let value = match value {
        Some(Bson::String(value) | Bson::Symbol(value) | Bson::JavaScriptCode(value)) => {
            value.as_str()
        }
        Some(Bson::JavaScriptCodeWithScope(value)) => value.code.as_str(),
        _ => return false,
    };
    choices.contains(&value.to_uppercase().as_str())
}

pub fn validate_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    if (1..=100).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
    {
        Ok(())
    } else {
        Err("task name must match ^[A-Za-z0-9][A-Za-z0-9_-]{0,99}$".into())
    }
}

fn fence(query: &mut Document, expected: &Document, keys: &[&str]) {
    for key in keys {
        query.insert(
            *key,
            expected
                .get(*key)
                .cloned()
                .unwrap_or_else(|| Bson::Document(doc! {"$exists": false})),
        );
    }
}

/// Minimal identity and version CAS query: `(_id, name, status, isRemote, updatedAt)`.
/// Historical proof/snapshot fields must never gate or deadlock control transitions.
pub fn task_binding_query(
    name: &str,
    expected: &Document,
    _command: &str,
) -> Result<Document, String> {
    let id = expected.get("_id").ok_or_else(|| "'_id'".to_string())?;
    let mut query = doc! {
        "name": name,
        "_id": id.clone(),
        "status": expected.get("status").cloned().unwrap_or(Bson::Null),
        "isRemote": expected.get("isRemote").cloned().unwrap_or(Bson::Null),
    };
    fence(&mut query, expected, &["updatedAt"]);
    Ok(query)
}

/// Conservative durable dispatch evidence, including historical attempts.
/// `httpStatus` being present and non-null counts even when it is 0/false/"".
/// This typed API accepts a document (Python's non-dict branch is not exposed).
pub fn checked_receipt_evidence(row: &Document) -> Result<bool, String> {
    if upper_token(row.get("acceptance"), &["ACCEPTED", "UNKNOWN"])
        || upper_token(row.get("terminalStatus"), &["UNKNOWN"])
        || row.get("campaign_post_attempted") == Some(&Bson::Boolean(true))
        || !is_null(row.get("httpStatus"))
        || [
            "alphaId",
            "progressUrl",
            "progress_url",
            "responseHeaders",
            "superSlotId",
            "childSimulationIds",
            "childSimulationId",
        ]
        .iter()
        .any(|key| truthy(row.get(*key)))
        || checked_one_of(
            row.get("errorClass"),
            &["campaign_outcome_unknown", "simulation_outcome_unknown"],
        )?
    {
        return Ok(true);
    }
    // This is AFTER the errorClass set lookup in the Python function. Moving
    // attempts above it would swallow an original TypeError.
    Ok(truthy(row.get("submissionAttempts")))
}

/// Legacy compatibility only. Invalid evidence must never mean "no receipt".
/// Use checked_receipt_evidence for state-changing eligibility decisions.
pub fn receipt_evidence(row: &Document) -> bool {
    checked_receipt_evidence(row).unwrap_or(true)
}

pub fn checked_positively_unposted(row: &Document) -> Result<bool, String> {
    // The acceptance test is Python tuple membership, NOT a set/hash check.
    // Non-scalar acceptance simply fails equality and skips receipt evaluation.
    if row.get("_not_posted_") != Some(&Bson::Boolean(true))
        || !(is_null(row.get("acceptance")) || text(row.get("acceptance")) == Some("NOT_POSTED"))
    {
        return Ok(false);
    }
    Ok(!checked_receipt_evidence(row)?)
}

/// Legacy compatibility only; an error is never positive no-POST proof.
pub fn positively_unposted(row: &Document) -> bool {
    checked_positively_unposted(row).unwrap_or(false)
}

/// Per-row equivalent of the original unresolved_execution collection loop.
/// Aggregate without swallowing Err; do not replace this with status counting.
pub fn checked_unresolved_execution_row(row: &Document) -> Result<bool, String> {
    if checked_positively_unposted(row)? {
        return Ok(false);
    }
    if upper_token(row.get("terminalStatus"), &["UNKNOWN"])
        || upper_token(row.get("acceptance"), &["UNKNOWN"])
    {
        return Ok(true);
    }
    let resolved = (text(row.get("status")) == Some("success") && truthy(row.get("alphaId")))
        || (checked_one_of(row.get("status"), TERMINAL_ROW_STATUSES)?
            && checked_one_of(
                row.get("terminalStatus"),
                &[
                    "COMPLETE",
                    "COMPLETED",
                    "WARNING",
                    "ERROR",
                    "FAIL",
                    "FAILED",
                    "CANCELLED",
                ],
            )?);
    if resolved {
        return Ok(false);
    }
    if checked_one_of(row.get("status"), &["processing", "outcome_unknown"])? {
        return Ok(true);
    }
    checked_receipt_evidence(row)
}

/// Legacy compatibility only; malformed evidence stays unresolved on error.
pub fn unresolved_execution_row(row: &Document) -> bool {
    checked_unresolved_execution_row(row).unwrap_or(true)
}

/// Paused control state or matching task stop identity on the expected side
/// suffices; historical timestamp ordering (`stopConfirmedAt > launchAttemptAt`)
/// is never required as an admission gate.
pub fn stop_confirmation_valid(task: &Document, remote: bool) -> bool {
    let id = match task.get("_id") {
        Some(Bson::ObjectId(id)) => id,
        _ => return false,
    };
    if truthy(task.get("isRemote")) != remote {
        return false;
    }
    if text(task.get("status")) == Some("paused") {
        return true;
    }
    text(task.get("stopConfirmedTaskId")) == Some(id.to_hex().as_str())
        && (is_null(task.get("stopConfirmedRemote"))
            || task.get("stopConfirmedRemote") == Some(&Bson::Boolean(remote)))
}

/// Persisted `acceptedObservation` records never lock out control operations;
/// live process inspection is the sole liveness authority.
pub fn assert_observation_closed(_task: &Document) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::oid::ObjectId;

    fn stop_task() -> Document {
        let id = ObjectId::parse_str("0123456789abcdef01234567").unwrap();
        doc! {"_id": id, "status": "paused", "stopConfirmedTaskId": id.to_hex(), "isRemote": false,
        "stopConfirmedRemote": false}
    }

    #[test]
    fn strict_name_boundaries() {
        for name in ["a", "0_X-y", &"a".repeat(100)] {
            assert!(validate_name(name).is_ok());
        }
        for name in ["", "_a", "-a", "a\n", " a", "a/b", "é", &"a".repeat(101)] {
            assert!(validate_name(name).is_err());
        }
    }

    #[test]
    fn cas_preserves_minimal_identity_and_version_fields() {
        let id = ObjectId::new();
        let old = doc! {"_id": id, "status": "ready", "isRemote": false, "updatedAt": Bson::Null};
        let query = task_binding_query("same", &old, "pause").unwrap();
        assert_eq!(query.get_object_id("_id").unwrap(), id);
        assert_ne!(query.get_object_id("_id").unwrap(), ObjectId::new());
        assert_eq!(query.get("updatedAt"), Some(&Bson::Null));
        assert!(!query.contains_key("generation"));
        assert!(!query.contains_key("tmuxContext"));
        assert!(task_binding_query("same", &doc! {}, "start").is_err());
        assert!(task_binding_query("same", &doc! {"_id": Bson::Null}, "start").is_ok());
        let missing_updated = doc! {"_id": id, "status": "ready", "isRemote": false};
        let missing_query = task_binding_query("same", &missing_updated, "pause").unwrap();
        assert_eq!(missing_query.get_document("updatedAt").unwrap(), &doc! {"$exists": false});
    }

    #[test]
    fn minimal_cas_ignores_historical_metadata_and_fences_identity_and_revision() {
        fn matches(query: &Document, row: &Document) -> bool {
            query.iter().all(|(key, value)| {
                if value == &Bson::Document(doc! {"$exists": false}) {
                    !row.contains_key(key)
                } else if value == &Bson::Null {
                    is_null(row.get(key))
                } else {
                    row.get(key) == Some(value)
                }
            })
        }
        let mut task = stop_task();
        task.extend(doc! {
            "name": "same", "updatedAt": "unchanged",
            "acceptedObservation": {"id": "observer-1", "finishedAt": Bson::Null},
            "launchAttemptId": "launch-1", "tmuxContext": {"sessionId": "$1"},
            "generation": {"id": "generation-1"}, "startupBinding": {"consumer_run_id": "consumer-1"},
        });
        let query = task_binding_query("same", &task, "pause").unwrap();
        assert!(matches(&query, &task));
        for key in ["_id", "name", "status", "isRemote", "updatedAt"] {
            let mut changed = task.clone();
            changed.insert(key, "replacement");
            assert!(!matches(&query, &changed), "core CAS field must fence: {key}");
        }
        for key in [
            "acceptedObservation", "launchAttemptId", "tmuxContext", "generation",
            "startupBinding", "stopConfirmedAt", "launchHistory", "events",
        ] {
            let mut changed = task.clone();
            changed.insert(key, "unrelated-change");
            assert!(matches(&query, &changed), "historical field should not fence CAS: {key}");
        }
    }

    #[test]
    fn unknown_beats_success_and_timeout_is_not_platform_terminal() {
        assert!(unresolved_execution_row(
            &doc! {"status": "success", "alphaId": "a", "acceptance": "unknown"}
        ));
        assert!(unresolved_execution_row(
            &doc! {"status": "timeout_error", "acceptance": "ACCEPTED"}
        ));
        assert!(unresolved_execution_row(
            &doc! {"status": "error", "superSlotId": "slot"}
        ));
        assert!(!unresolved_execution_row(
            &doc! {"status": "error", "terminalStatus": "ERROR", "acceptance": "ACCEPTED"}
        ));
        assert!(!unresolved_execution_row(&doc! {"status": "pending"}));
        assert!(!unresolved_execution_row(&doc! {"status": "timeout_error"})); // Original policy, no receipt.
    }

    #[test]
    fn strict_boolean_vs_python_truthiness() {
        assert!(!positively_unposted(&doc! {"_not_posted_": 1}));
        assert!(!receipt_evidence(&doc! {"campaign_post_attempted": 1}));
        assert!(receipt_evidence(&doc! {"httpStatus": false}));
        assert!(positively_unposted(
            &doc! {"_not_posted_": true, "submissionAttempts": []}
        ));
        assert!(!positively_unposted(
            &doc! {"_not_posted_": true, "submissionAttempts": [Bson::Null]}
        ));
        assert!(assert_observation_closed(&doc! {"acceptedObservation": {"finishedAt": Bson::Null}}).is_ok());
    }

    #[test]
    fn checked_receipt_errors_follow_python_short_circuit_order() {
        let bad = doc! {"errorClass": [], "submissionAttempts": [1]};
        assert_eq!(
            checked_receipt_evidence(&bad),
            Err("unhashable type: 'list'".into())
        );
        assert!(receipt_evidence(&bad));
        // submissionAttempts is later than the hash lookup; it cannot mask the error.
        for (key, value) in [
            ("acceptance", Bson::String("ACCEPTED".into())),
            ("terminalStatus", Bson::String("UNKNOWN".into())),
            ("campaign_post_attempted", Bson::Boolean(true)),
            ("httpStatus", Bson::Int32(0)),
            ("progressUrl", Bson::String("receipt".into())),
        ] {
            let mut short = bad.clone();
            short.insert(key, value);
            assert_eq!(checked_receipt_evidence(&short), Ok(true), "{key}");
        }
        let mut unposted = bad.clone();
        // No _not_posted_ proof: receipt evaluation is skipped, not an error.
        assert_eq!(checked_positively_unposted(&unposted), Ok(false));
        unposted.insert("_not_posted_", true);
        assert_eq!(
            checked_positively_unposted(&unposted),
            Err("unhashable type: 'list'".into())
        );
        assert!(!positively_unposted(&unposted));
        unposted.insert("acceptance", Bson::Array(vec![]));
        assert_eq!(checked_positively_unposted(&unposted), Ok(false)); // tuple equality, no hash
    }

    #[test]
    fn checked_unresolved_never_reorders_corrupt_receipts() {
        let bad = doc! {"status": [], "terminalStatus": {}, "errorClass": {"bad": true}};
        assert_eq!(
            checked_unresolved_execution_row(&bad),
            Err("unhashable type: 'list'".into())
        );
        assert!(unresolved_execution_row(&bad));
        let mut early = bad.clone();
        early.insert("_not_posted_", true);
        assert_eq!(
            checked_unresolved_execution_row(&early),
            Err("unhashable type: 'dict'".into())
        );
        early.remove("errorClass");
        assert_eq!(checked_unresolved_execution_row(&early), Ok(false)); // positive noPOST skips bad status
        let mut unknown = bad.clone();
        unknown.insert("acceptance", "UNKNOWN");
        assert_eq!(checked_unresolved_execution_row(&unknown), Ok(true));
        let mut success = bad.clone();
        success.insert("status", "success");
        success.insert("alphaId", "alpha");
        assert_eq!(checked_unresolved_execution_row(&success), Ok(false)); // skip terminal/errorClass hash
        success.remove("alphaId");
        assert_eq!(
            checked_unresolved_execution_row(&success),
            Err("unhashable type: 'dict'".into())
        );
        let mut processing = bad.clone();
        processing.insert("status", "processing");
        assert_eq!(checked_unresolved_execution_row(&processing), Ok(true)); // no terminal/errorClass hash
        let mut pending = bad.clone();
        pending.insert("status", "pending");
        assert_eq!(
            checked_unresolved_execution_row(&pending),
            Err("unhashable type: 'dict'".into())
        );
    }

    #[test]
    fn checked_hashability_matches_pymongo_objects_and_cas_order() {
        use mongodb::bson::{Decimal128, JavaScriptCodeWithScope, Regex};
        use std::str::FromStr;
        for (value, kind) in [
            (Bson::Array(vec![]), "list"),
            (Bson::Document(doc! {}), "dict"),
            (Bson::JavaScriptCode("UNKNOWN".into()), "Code"),
            (
                Bson::JavaScriptCodeWithScope(JavaScriptCodeWithScope {
                    code: "x".into(),
                    scope: doc! {},
                }),
                "Code",
            ),
            (
                Bson::RegularExpression(Regex {
                    pattern: "".into(),
                    options: "".into(),
                }),
                "Regex",
            ),
            (
                Bson::Decimal128(Decimal128::from_str("0").unwrap()),
                "Decimal128",
            ),
        ] {
            let error = format!("unhashable type: '{kind}'");
            assert_eq!(
                checked_receipt_evidence(&doc! {"errorClass": value.clone()}),
                Err(error)
            );
            assert_eq!(
                task_binding_query("task", &doc! {"status": value}, "pause"),
                Err("'_id'".into())
            );
        }
        // Code stringification works for UNKNOWN detection, but Code objects
        // are unhashable when a later set-membership site is actually reached.
        let code_unknown = Bson::JavaScriptCode("UNKNOWN".into());
        assert_eq!(
            checked_unresolved_execution_row(&doc! {"status": [], "terminalStatus": code_unknown}),
            Ok(true)
        );
    }

    #[test]
    fn stop_confirmation_uses_paused_status_or_matching_identity_without_timestamp_gate() {
        let mut task = stop_task();
        assert!(stop_confirmation_valid(&task, false));
        assert!(!stop_confirmation_valid(&task, true));
        // Paused task stays valid even without stopConfirmedAt or with later launchAttemptAt.
        task.insert("launchAttemptAt", bson::DateTime::from_millis(5000));
        assert!(stop_confirmation_valid(&task, false));
        task.remove("stopConfirmedTaskId");
        task.remove("stopConfirmedRemote");
        assert!(stop_confirmation_valid(&task, false));

        let mut completed = doc! {
            "_id": task.get("_id").unwrap().clone(),
            "status": "completed_with_errors",
            "isRemote": false,
            "stopConfirmedTaskId": task.get_object_id("_id").unwrap().to_hex(),
            "stopConfirmedRemote": false,
        };
        assert!(stop_confirmation_valid(&completed, false));
        completed.insert("stopConfirmedTaskId", "foreign");
        assert!(!stop_confirmation_valid(&completed, false));
    }
}
