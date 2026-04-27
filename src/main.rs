use glob::glob;
use ksni::{self, MenuItem, Tray, TrayService};
use notify_rust::{Notification, Urgency};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const SYSFS_PATTERN: &str = "/sys/bus/hid/drivers/razermouse/*/{}";
const POLL_INTERVAL_SECS: u64 = 60;
const LOW_BATTERY_THRESHOLD: u8 = 20;

static VERBOSE: AtomicBool = AtomicBool::new(false);

macro_rules! log_info {
    ($($arg:tt)*) => {
        if VERBOSE.load(Ordering::Relaxed) {
            println!("[razer-tray] {}", format_args!($($arg)*));
        }
    };
}

const HELP_TEXT: &str = "\
razer-tray - tray indicator for Razer wireless mouse battery

USAGE:
    razer-tray [OPTIONS]

OPTIONS:
    -h, --help       Print this help message and exit
    -V, --version    Print version and exit
    -v, --verbose    Print info logs to stdout
";

#[derive(Clone)]
struct BatteryState {
    level: Option<u8>,
    charging: bool,
    low_notified: bool,
}

struct RazerTray {
    state: Arc<Mutex<BatteryState>>,
}

impl Tray for RazerTray {
    fn icon_name(&self) -> String {
        let state = self.state.lock().unwrap();
        match (state.level, state.charging) {
            (None, _) => "battery-missing".into(),
            (Some(_), true) => "battery-full-charging".into(),
            (Some(l), false) if l > 80 => "battery-full".into(),
            (Some(l), false) if l > 60 => "battery-good".into(),
            (Some(l), false) if l > 40 => "battery-medium".into(),
            (Some(l), false) if l > 20 => "battery-low".into(),
            (Some(_), false) => "battery-caution".into(),
        }
    }

    fn title(&self) -> String {
        "Razer Battery".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let state = self.state.lock().unwrap();
        let description = match (state.level, state.charging) {
            (None, _) => "Mouse not found".into(),
            (Some(l), true) => format!("Razer Mouse: {}% (charging)", l),
            (Some(l), false) => format!("Razer Mouse: {}%", l),
        };
        ksni::ToolTip {
            title: description,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let state = self.state.lock().unwrap();
        let label = match (state.level, state.charging) {
            (None, _) => "Mouse not found".into(),
            (Some(l), true) => format!("Battery: {}% (charging)", l),
            (Some(l), false) => format!("Battery: {}%", l),
        };
        vec![
            MenuItem::Standard(ksni::menu::StandardItem {
                label,
                enabled: false,
                ..Default::default()
            }),
            MenuItem::Separator,
            MenuItem::Standard(ksni::menu::StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }),
        ]
    }

    fn id(&self) -> String {
        "razer-tray".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }
}

fn read_sysfs(filename: &str) -> Option<String> {
    for path in glob(&SYSFS_PATTERN.replace("{}", filename)).ok()?.flatten() {
        if let Ok(content) = fs::read_to_string(&path) {
            return Some(content.trim().to_string());
        }
    }
    None
}

fn read_battery() -> (Option<u8>, bool) {
    let level = read_sysfs("charge_level").and_then(|s| s.parse::<u8>().ok());
    let charging = read_sysfs("charge_status")
        .map(|s| s == "1")
        .unwrap_or(false);
    (level, charging)
}

fn notify_low_battery(level: u8) {
    log_info!("notification: low battery {}%", level);
    if let Err(e) = Notification::new()
        .summary("Razer Mouse — Low Battery")
        .body(&format!("Battery level: {}%", level))
        .icon("battery-caution")
        .urgency(Urgency::Critical)
        .timeout(10000)
        .show()
    {
        eprintln!("[razer-tray] failed to show notification: {}", e);
    }
}

fn parse_args() -> Result<(), i32> {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{}", HELP_TEXT);
                return Err(0);
            }
            "-V" | "--version" => {
                println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                return Err(0);
            }
            "-v" | "--verbose" => {
                VERBOSE.store(true, Ordering::Relaxed);
            }
            other => {
                eprintln!("razer-tray: unknown argument '{}'", other);
                eprintln!();
                eprint!("{}", HELP_TEXT);
                return Err(2);
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(code) = parse_args() {
        std::process::exit(code);
    }

    log_info!(
        "starting {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let state = Arc::new(Mutex::new(BatteryState {
        level: None,
        charging: false,
        low_notified: false,
    }));

    let (level, charging) = read_battery();
    log_info!("initial battery: level={:?} charging={}", level, charging);
    {
        let mut s = state.lock().unwrap();
        s.level = level;
        s.charging = charging;
    }

    let tray = RazerTray {
        state: state.clone(),
    };

    let service = TrayService::new(tray);
    let handle = service.handle();
    service.spawn();

    loop {
        thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));

        let (level, charging) = read_battery();
        log_info!("poll: level={:?} charging={}", level, charging);
        let mut s = state.lock().unwrap();
        s.level = level;
        s.charging = charging;

        if let Some(l) = level {
            if l <= LOW_BATTERY_THRESHOLD && !charging && !s.low_notified {
                notify_low_battery(l);
                s.low_notified = true;
            }
            if l > LOW_BATTERY_THRESHOLD {
                s.low_notified = false;
            }
        }

        drop(s);
        handle.update(|_| {});
    }
}
