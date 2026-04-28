pub const SLEEP_DETECTION_MIN_DROP: u8 = 5;
pub const DEFAULT_DEVICE_NAME: &str = "Razer Device";

pub fn extract_persistent_id(sysfs_id: &str) -> Option<String> {
    let main = sysfs_id.split('.').next()?;
    let parts: Vec<&str> = main.split(':').collect();
    if parts.len() == 3 && !parts[1].is_empty() && !parts[2].is_empty() {
        Some(format!("{}:{}", parts[1], parts[2]))
    } else {
        None
    }
}

pub fn apply_sleep_detection(
    raw_level: Option<u8>,
    prev_level: Option<u8>,
    charging: bool,
) -> Option<u8> {
    match (raw_level, prev_level) {
        (Some(0), Some(p)) if !charging && p >= SLEEP_DETECTION_MIN_DROP => Some(p),
        _ => raw_level,
    }
}

pub fn apply_debounce(
    post_sleep: Option<u8>,
    prev_level: Option<u8>,
    last_raw_level: Option<u8>,
) -> Option<u8> {
    if post_sleep == prev_level || prev_level.is_none() || last_raw_level == post_sleep {
        post_sleep
    } else {
        prev_level
    }
}

pub fn parse_persisted_selection(content: &str) -> Option<String> {
    let trimmed = content.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub fn format_device_label(name: Option<&str>, level: Option<u8>, charging: bool) -> String {
    let name = name.unwrap_or(DEFAULT_DEVICE_NAME);
    match (level, charging) {
        (None, _) => format!("{}: not found", name),
        (Some(l), true) => format!("{}: {}% (charging)", name, l),
        (Some(l), false) => format!("{}: {}%", name, l),
    }
}
