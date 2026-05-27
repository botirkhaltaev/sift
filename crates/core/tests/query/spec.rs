use sift_core::{QueryFlags, QuerySpec};

#[test]
fn query_spec_accepts_pattern_and_flags() {
    let pattern = "needle".to_string();
    let flags = QueryFlags::CASE_INSENSITIVE;
    let spec = QuerySpec {
        patterns: &[pattern],
        flags,
    };
    assert_eq!(spec.patterns, &["needle"]);
    assert!(spec.flags.contains(QueryFlags::CASE_INSENSITIVE));
}
