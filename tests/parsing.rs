use razer_tray::{extract_persistent_id, parse_persisted_selection};

#[test]
fn extract_persistent_id_happy_path() {
    assert_eq!(
        extract_persistent_id("0003:1532:00B8.000A").as_deref(),
        Some("1532:00B8")
    );
}

#[test]
fn extract_persistent_id_no_instance_suffix() {
    assert_eq!(
        extract_persistent_id("0003:1532:00B8").as_deref(),
        Some("1532:00B8")
    );
}

#[test]
fn extract_persistent_id_long_instance() {
    assert_eq!(
        extract_persistent_id("0003:1532:022D.0000FFFF").as_deref(),
        Some("1532:022D")
    );
}

#[test]
fn extract_persistent_id_too_few_segments() {
    assert_eq!(extract_persistent_id("1532:00B8"), None);
    assert_eq!(extract_persistent_id("1532:00B8.000A"), None);
}

#[test]
fn extract_persistent_id_empty_segment() {
    assert_eq!(extract_persistent_id("0003::00B8.000A"), None);
    assert_eq!(extract_persistent_id("0003:1532:.000A"), None);
}

#[test]
fn extract_persistent_id_total_garbage() {
    assert_eq!(extract_persistent_id(""), None);
    assert_eq!(extract_persistent_id("nonsense"), None);
    assert_eq!(extract_persistent_id("a:b:c:d:e"), None);
}

#[test]
fn parse_persisted_selection_happy_path() {
    assert_eq!(
        parse_persisted_selection("1532:00B8").as_deref(),
        Some("1532:00B8")
    );
}

#[test]
fn parse_persisted_selection_trims_whitespace() {
    assert_eq!(
        parse_persisted_selection("  1532:00B8  \n").as_deref(),
        Some("1532:00B8")
    );
}

#[test]
fn parse_persisted_selection_rejects_empty() {
    assert_eq!(parse_persisted_selection(""), None);
    assert_eq!(parse_persisted_selection("   "), None);
    assert_eq!(parse_persisted_selection("\n\n"), None);
}

#[test]
fn parse_persisted_selection_rejects_malformed() {
    assert_eq!(parse_persisted_selection("1532"), None);
    assert_eq!(parse_persisted_selection("1532:"), None);
    assert_eq!(parse_persisted_selection(":00B8"), None);
    assert_eq!(parse_persisted_selection("1532:00B8:extra"), None);
}
