//! Structural routing oracle, not a fake service/business-safety test.
//! Parse the actual compiled source with syn; every match arm must consist
//! solely of the expected common-facade call (pause adapts its outcome).
use quote::ToTokens;
use std::collections::BTreeMap;

fn tokens(value: &impl ToTokens) -> String {
    value.to_token_stream().to_string()
}
fn routes(source: &str) -> BTreeMap<String, String> {
    let file = syn::parse_file(source).unwrap();
    let function = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == "dispatch" => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        function.block.stmts.len(),
        1,
        "dispatch must only match the typed request"
    );
    let syn::Stmt::Expr(syn::Expr::Match(m), _) = &function.block.stmts[0] else {
        panic!("expected direct match")
    };
    assert_eq!(tokens(&m.expr), "request");
    let mut result = BTreeMap::new();
    for arm in &m.arms {
        assert!(arm.guard.is_none());
        assert!(result.insert(tokens(&arm.pat), tokens(&arm.body)).is_none());
    }
    result
}
fn expected() -> BTreeMap<String, String> {
    let specs = [
        ("List", "control.list(r).await"),
        ("VerifyRegistration", "control.dataset_evidence(r).await"),
        ("ArchiveResearch", "control.dataset_evidence(r).await"),
        ("Create", "control.create(r, None).await"),
        ("Generate", "control.generate(r, None).await"),
        ("Sync", "control.sync(&r.name, r.expected_task_id.as_deref(), None).await"),
        ("Start", "control.start(r, None).await"),
        ("ObserveAccepted", "control.observe_accepted(r, None).await"),
        ("Watch", "control.watch(r).await"),
        ("Status", "control.status(r).await"),
        ("Pause", "{ let result = control.pause(&r.name, PauseOptions {
            expected_task_id: r.expected_task_id.as_deref(),
            expected_launch_attempt_id: r.expected_launch_attempt_id.as_deref(),
            expected_remote: None, observer: None,
        }, StopBudget::Cli, None).await;
        TaskOutcome { value: result.value, exit_code: result.exit_code } }"),
        ("Resume", "control.resume(r, None).await"),
        ("PriorityPrune", "control.priority_prune(r, None).await"),
        ("Delete", "control.delete(&r.name, r.expected_task_id.as_deref(), r.confirm.as_deref(), None).await"),
        ("Results", "control.results(r).await"),
        ("CollectResults", "control.collect_results(r).await"),
    ];
    // Rustfmt may add trailing commas or braces to multiline calls. Compare
    // AST after removing punctuation-only formatting, not substring presence.
    specs
        .into_iter()
        .map(|(variant, body)| {
            (
                format!("Request :: {variant} (r)"),
                tokens(&syn::parse_str::<syn::Expr>(body).unwrap()),
            )
        })
        .collect()
}
fn normalized(body: &str) -> String {
    // Remove only trailing commas before closing argument delimiters.
    // rustfmt can also wrap expression arms in blocks, handled via syn below.
    let mut expr = syn::parse_str::<syn::Expr>(body).unwrap();
    if let syn::Expr::Block(b) = &expr {
        if let [syn::Stmt::Expr(inner, None)] = b.block.stmts.as_slice() {
            expr = inner.clone();
        }
    }
    tokens(&expr)
        .split_whitespace()
        .collect::<String>()
        .replace(",)", ")")
        .replace(",}", "}")
}
fn assert_routes(source: &str) {
    let actual: BTreeMap<_, _> = routes(source)
        .into_iter()
        .map(|(k, v)| (k, normalized(&v)))
        .collect();
    let expected: BTreeMap<_, _> = expected()
        .into_iter()
        .map(|(k, v)| (k, normalized(&v)))
        .collect();
    assert_eq!(actual.len(), 16);
    assert_eq!(actual, expected);
}

#[test]
fn every_route_delegates_to_the_actual_common_facade() {
    assert_routes(include_str!("../src/dispatch.rs"));
}
#[test]
fn oracle_rejects_misrouting_or_changed_safety_arguments() {
    let source = include_str!("../src/dispatch.rs");
    for (old, new) in [
        ("control.status(r)", "control.results(r)"),
        ("StopBudget::Cli", "StopBudget::Gui"),
        ("expected_remote: None", "expected_remote: Some(true)"),
        ("exit_code: result.exit_code", "exit_code: 0"),
        ("r.confirm.as_deref()", "None"),
    ] {
        assert!(source.contains(old));
        let bad = source.replace(old, new);
        assert!(
            std::panic::catch_unwind(|| assert_routes(&bad)).is_err(),
            "oracle must reject {new}"
        );
    }
}
