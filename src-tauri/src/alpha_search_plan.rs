//! Narrow, measured search-plan policy. No query timeout or predicate changes.
//! No index creation, rewritten predicates, forced collation or partial results.
use mongodb::{bson::{Bson, Document}, options::Hint, IndexModel};

/// Limit the deterministic plan to the measured shape: a plain keyword OR on
/// the three code columns, with an optional exact type predicate and no other
/// filter. Advanced JSON, arbitrary regex, exclusions, IDs, region etc keep the
/// normal optimizer. The filter itself is NEVER rewritten.
pub fn simple_keyword_scope(filter: &Document) -> Option<bool> {
    if filter.len() != 1 { return None; }
    let terms = filter.get_array("$and").ok()?;
    if terms.is_empty() || terms.len() > 2 { return None; }
    let mut keyword = false;
    let mut has_type = false;
    for term in terms {
        let term = term.as_document()?;
        if term.len() != 1 { return None; }
        if let Ok(value) = term.get_str("type") {
            if has_type || !matches!(value, "REGULAR") { return None; }
            has_type = true;
        } else {
            if keyword { return None; }
            let branches = term.get_array("$or").ok()?;
            if branches.len() != 3 { return None; }
            let mut pattern: Option<&str> = None;
            for (branch, field) in branches.iter().zip(["regular.code", "selection.code", "combo.code"]) {
                let branch = branch.as_document()?;
                if branch.len() != 1 { return None; }
                let regex = branch.get_document(field).ok()?;
                if regex.len() != 2 || regex.get_str("$options").ok()? != "i" { return None; }
                let value = regex.get_str("$regex").ok()?;
                if value.is_empty() || !value.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                    return None;
                }
                if pattern.is_some_and(|p| p != value) { return None; }
                pattern = Some(value);
            }
            keyword = true;
        }
    }
    keyword.then_some(has_type)
}

pub fn index_direction(value: &Bson) -> bool {
    matches!(value, Bson::Int32(1 | -1) | Bson::Int64(1 | -1))
}

pub fn select_keyword_hint(filter: &Document, sort: &Document, indexes: &[IndexModel]) -> Option<Hint> {
    let has_type = simple_keyword_scope(filter)?;
    if sort.len() != 1 { return None; }
    let (field, direction) = sort.iter().next()?;
    // Only this exact simple-keyword + type + Sharpe shape was measured.
    // Other sorts/filters keep the existing optimizer until independently tested.
    if !has_type || !index_direction(direction) || field != "is.sharpe" { return None; }
    // Use only the complete single sort-field index measured in the control.
    // Neither sparse, partial, hidden, collated nor special indexes are safe here.
    let mut candidates = Vec::new();
    for index in indexes {
        let options = index.options.as_ref();
        if options.is_some_and(|o| o.hidden == Some(true) || o.sparse == Some(true)
            || o.partial_filter_expression.is_some() || o.collation.is_some()) { continue; }
        let keys = &index.keys;
        if !keys.values().all(index_direction) { continue; }
        let fields: Vec<_> = keys.keys().map(String::as_str).collect();
        if fields != vec![field.as_str()] { continue; }
        let priority = 0;
        if let Some(name) = options.and_then(|o| o.name.as_ref()) {
            candidates.push((priority, name.clone()));
        }
    }
    candidates.sort();
    candidates.into_iter().next().map(|(_, name)| Hint::Name(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::{bson::doc, options::{IndexOptions, Collation}};
    fn filter() -> Document { doc! { "$and": [
        {"$or": [
            {"regular.code": {"$regex": "oth455", "$options": "i"}},
            {"selection.code": {"$regex": "oth455", "$options": "i"}},
            {"combo.code": {"$regex": "oth455", "$options": "i"}}
        ]}, {"type": "REGULAR"}
    ]}}
    fn index(name: &str, keys: Document) -> IndexModel {
        IndexModel::builder().keys(keys).options(IndexOptions::builder().name(name.to_string()).build()).build()
    }
    #[test]
    fn measured_shape_hints_existing_sort_index_without_rewriting_filter() {
        let f = filter(); let before = f.clone();
        for direction in [1, -1] {
            assert_eq!(select_keyword_hint(&f, &doc!{"is.sharpe": direction},
                &[index("score", doc!{"is.sharpe": 1})]), Some(Hint::Name("score".into())));
        }
        assert_eq!(f, before);
    }
    #[test]
    fn absent_metadata_or_unmeasured_sort_does_not_hint() {
        assert_eq!(select_keyword_hint(&filter(), &doc!{"is.sharpe": -1}, &[]), None);
        for sort in [doc!{"dateCreated": -1}, doc!{"is.fitness": -1}, doc!{"is.sharpe": -1,"id":1}] {
            assert_eq!(select_keyword_hint(&filter(), &sort,
                &[index("score", doc!{"is.sharpe":1})]),None);
        }
        assert_eq!(select_keyword_hint(&filter(), &doc!{"is.sharpe": -1},
            &[index("compound",doc!{"type":1,"is.sharpe":-1})]),None);
    }
    #[test]
    fn complex_and_nonkeyword_filters_keep_optimizer() {
        for extra in [doc!{"settings.region":"USA"},doc!{"id":"some-id"},doc!{"$expr":{"$gt":["$is.sharpe",2]}}] {
            let mut f=filter();f.get_array_mut("$and").unwrap().push(Bson::Document(extra));
            assert_eq!(simple_keyword_scope(&f),None);
        }
        assert_eq!(simple_keyword_scope(&doc!{}),None);
        assert_eq!(simple_keyword_scope(&doc!{"type":"REGULAR"}),None);
        for pattern in ["a.*b","a b","^oth","","中文"] {
            let mut f=filter();let branches=f.get_array_mut("$and").unwrap()[0].as_document_mut().unwrap().get_array_mut("$or").unwrap();
            for branch in branches {let d=branch.as_document_mut().unwrap();let k=d.keys().next().unwrap().clone();d.get_document_mut(&k).unwrap().insert("$regex",pattern);}
            assert_eq!(simple_keyword_scope(&f),None);
        }
    }
    #[test]
    fn no_type_or_ra_does_not_hint() {
        let mut f=filter();f.get_array_mut("$and").unwrap().pop();
        assert_eq!(select_keyword_hint(&f,&doc!{"is.sharpe":-1},&[index("score",doc!{"is.sharpe":1})]),None);
        let mut f=filter();f.get_array_mut("$and").unwrap()[1]=Bson::Document(doc!{"type":"RA_PARENT"});
        assert_eq!(simple_keyword_scope(&f),None);
    }
    #[test]
    fn incomplete_hidden_or_collated_index_never_hint() {
        for flag in 0..5 {
            let mut idx=index("bad",doc!{"is.sharpe":1});let o=idx.options.as_mut().unwrap();
            match flag {0=>o.sparse=Some(true),1=>o.hidden=Some(true),2=>o.partial_filter_expression=Some(doc!{"type":"REGULAR"}),
                3=>o.collation=Some(Collation::builder().locale("en".to_string()).build()),_=>{idx.keys=doc!{"is.sharpe":"hashed"};}}
            assert_eq!(select_keyword_hint(&filter(),&doc!{"is.sharpe":-1},&[idx]),None);
        }
    }
}
