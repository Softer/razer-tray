use razer_tray::{apply_debounce, apply_sleep_detection, format_device_label};

#[test]
fn sleep_detection_keeps_prev_when_zero_drop_below_threshold() {
    assert_eq!(
        apply_sleep_detection(Some(0), Some(50), false),
        Some(50),
        "raw=0% from prev=50% while not charging means sleep, not real drop"
    );
}

#[test]
fn sleep_detection_passes_real_zero_when_prev_was_low() {
    assert_eq!(
        apply_sleep_detection(Some(0), Some(3), false),
        Some(0),
        "drop from below MIN_DROP=5 is treated as a real reading, not sleep"
    );
}

#[test]
fn sleep_detection_does_not_trigger_when_charging() {
    assert_eq!(
        apply_sleep_detection(Some(0), Some(50), true),
        Some(0),
        "charging cable in -> 0% should pass through, never assume sleep"
    );
}

#[test]
fn sleep_detection_passes_normal_levels() {
    assert_eq!(apply_sleep_detection(Some(72), Some(73), false), Some(72));
    assert_eq!(apply_sleep_detection(Some(50), Some(50), false), Some(50));
}

#[test]
fn sleep_detection_first_appearance_returns_raw() {
    assert_eq!(apply_sleep_detection(Some(80), None, false), Some(80));
    assert_eq!(apply_sleep_detection(None, None, false), None);
}

#[test]
fn debounce_commits_when_unchanged() {
    assert_eq!(apply_debounce(Some(50), Some(50), Some(50)), Some(50));
}

#[test]
fn debounce_holds_first_change() {
    assert_eq!(
        apply_debounce(Some(49), Some(50), Some(50)),
        Some(50),
        "single transient drop is held back, not committed"
    );
}

#[test]
fn debounce_commits_after_second_matching_read() {
    assert_eq!(
        apply_debounce(Some(49), Some(50), Some(49)),
        Some(49),
        "two consecutive same readings flip the displayed level"
    );
}

#[test]
fn debounce_first_appearance_commits_immediately() {
    assert_eq!(
        apply_debounce(Some(80), None, None),
        Some(80),
        "no previous level (first read or post-reconnect) commits without confirmation"
    );
}

#[test]
fn debounce_oscillation_never_commits() {
    let mut last_raw = Some(50);
    let mut displayed = Some(50);
    for &raw in &[Some(49), Some(50), Some(49), Some(50)] {
        displayed = apply_debounce(raw, displayed, last_raw);
        last_raw = raw;
    }
    assert_eq!(
        displayed,
        Some(50),
        "alternating noise must never reach a stable commit"
    );
}

#[test]
fn debounce_disconnect_takes_two_ticks() {
    let displayed_after_first = apply_debounce(None, Some(50), Some(50));
    assert_eq!(displayed_after_first, Some(50));
    let displayed_after_second = apply_debounce(None, Some(50), None);
    assert_eq!(displayed_after_second, None);
}

#[test]
fn format_label_handles_all_cases() {
    assert_eq!(
        format_device_label(Some("Razer DeathAdder V3 Pro"), Some(67), false),
        "Razer DeathAdder V3 Pro: 67%"
    );
    assert_eq!(
        format_device_label(Some("Razer BlackWidow V3 Pro"), Some(23), true),
        "Razer BlackWidow V3 Pro: 23% (charging)"
    );
    assert_eq!(
        format_device_label(Some("Razer Viper Ultimate"), None, false),
        "Razer Viper Ultimate: not found"
    );
    assert_eq!(
        format_device_label(None, Some(50), false),
        "Razer Device: 50%",
        "missing name falls back to DEFAULT_DEVICE_NAME"
    );
}
