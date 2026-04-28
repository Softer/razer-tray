use glob::glob;
use ksni::{self, MenuItem, Tray, TrayService};
use notify_rust::{Notification, Urgency};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const SYSFS_PATTERN: &str = "/sys/bus/hid/drivers/razermouse/*/{}";
const POLL_INTERVAL_SECS: u64 = 1;
const LOW_BATTERY_THRESHOLD: u8 = 20;
const SLEEP_DETECTION_MIN_DROP: u8 = 5;

static VERBOSE: AtomicBool = AtomicBool::new(false);
static QUIT_ON_DISCONNECT: AtomicBool = AtomicBool::new(false);

const STARTUP_GRACE_RETRIES: u8 = 5;
const STARTUP_GRACE_INTERVAL_SECS: u64 = 1;

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
    -h, --help                 Print this help message and exit
    -V, --version              Print version and exit
    -v, --verbose              Print info logs to stdout
    -q, --quit-on-disconnect   Exit cleanly when the mouse disappears
                               from sysfs. Combine with the udev rule
                               shipped in the package to relaunch on
                               hotplug instead of polling forever.
";

#[derive(Clone)]
struct BatteryState {
    level: Option<u8>,
    charging: bool,
    low_notified: bool,
    device_name: Option<String>,
}

const DEFAULT_DEVICE_NAME: &str = "Razer Mouse";

struct RazerTray {
    state: Arc<Mutex<BatteryState>>,
}

impl Tray for RazerTray {
    fn icon_name(&self) -> String {
        let state = self.state.lock().unwrap();
        if let Some(icons) = get_icons() {
            if state.level.is_some() || icons.missing.is_some() {
                return String::new();
            }
        }
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

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let state = self.state.lock().unwrap();
        let Some(icons) = get_icons() else {
            return vec![];
        };
        let icon = match state.level {
            Some(level) => {
                let idx = (level as usize).min(100);
                if state.charging {
                    &icons.charging[idx]
                } else {
                    &icons.discharging[idx]
                }
            }
            None => match icons.missing.as_ref() {
                Some(m) => m,
                None => return vec![],
            },
        };
        vec![icon.clone()]
    }

    fn title(&self) -> String {
        "Razer Battery".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let state = self.state.lock().unwrap();
        let name = state.device_name.as_deref().unwrap_or(DEFAULT_DEVICE_NAME);
        let description = match (state.level, state.charging) {
            (None, _) => format!("{}: not found", name),
            (Some(l), true) => format!("{}: {}% (charging)", name, l),
            (Some(l), false) => format!("{}: {}%", name, l),
        };
        ksni::ToolTip {
            title: description,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let state = self.state.lock().unwrap();
        let name = state.device_name.as_deref().unwrap_or(DEFAULT_DEVICE_NAME);
        let label = match (state.level, state.charging) {
            (None, _) => format!("{}: not found", name),
            (Some(l), true) => format!("{}: {}% (charging)", name, l),
            (Some(l), false) => format!("{}: {}%", name, l),
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

struct IconSet {
    discharging: Vec<ksni::Icon>,
    charging: Vec<ksni::Icon>,
    missing: Option<ksni::Icon>,
}

static ICON_SET: OnceLock<Option<IconSet>> = OnceLock::new();

fn get_icons() -> Option<&'static IconSet> {
    ICON_SET
        .get_or_init(|| {
            let dir = find_icons_dir()?;
            log_info!("loading icons from {}", dir.display());
            match load_icon_set(&dir) {
                Ok(set) => Some(set),
                Err(e) => {
                    eprintln!(
                        "[razer-tray] failed to load icons from {}: {}",
                        dir.display(),
                        e
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn find_icons_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RAZER_TRAY_ICONS_DIR") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(prefix) = exe.parent().and_then(|p| p.parent()) {
            let p = prefix.join("share/razer-tray/icons");
            if p.is_dir() {
                return Some(p);
            }
        }
        if let Some(project) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            let p = project.join("icons");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let p = PathBuf::from("/usr/share/razer-tray/icons");
    if p.is_dir() {
        return Some(p);
    }
    let p = PathBuf::from("./icons");
    if p.is_dir() {
        return Some(p);
    }
    None
}

fn load_icon_set(dir: &Path) -> Result<IconSet, String> {
    let mut discharging = Vec::with_capacity(101);
    let mut charging = Vec::with_capacity(101);
    for level in 0..=100u8 {
        discharging.push(load_png(&dir.join(format!("bat_{}.png", level)))?);
        charging.push(load_png(&dir.join(format!("bat_{}_c.png", level)))?);
    }
    let missing = load_png(&dir.join("bat_missing.png")).ok();
    Ok(IconSet {
        discharging,
        charging,
        missing,
    })
}

fn load_png(path: &Path) -> Result<ksni::Icon, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{}: {}", path.display(), e))?;

    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "{}: expected 8-bit RGBA, got {:?} {:?}",
            path.display(),
            info.color_type,
            info.bit_depth
        ));
    }

    let mut argb = Vec::with_capacity(buf.len());
    for px in buf.chunks_exact(4) {
        argb.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
    }

    Ok(ksni::Icon {
        width: info.width as i32,
        height: info.height as i32,
        data: argb,
    })
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
        .map(|s| !s.is_empty() && s != "0")
        .unwrap_or(false);
    (level, charging)
}

fn read_device_name() -> Option<String> {
    read_sysfs("device_type").filter(|s| !s.is_empty())
}

fn notify_low_battery(level: u8, device_name: &str) {
    log_info!("notification: low battery {}%", level);
    if let Err(e) = Notification::new()
        .summary(&format!("{} — Low Battery", device_name))
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
            "-q" | "--quit-on-disconnect" => {
                QUIT_ON_DISCONNECT.store(true, Ordering::Relaxed);
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
        device_name: None,
    }));

    let quit_on_disconnect = QUIT_ON_DISCONNECT.load(Ordering::Relaxed);
    let mut initial = read_battery();
    let mut device_name = read_device_name();
    if quit_on_disconnect && initial.0.is_none() {
        for _ in 0..STARTUP_GRACE_RETRIES {
            thread::sleep(Duration::from_secs(STARTUP_GRACE_INTERVAL_SECS));
            initial = read_battery();
            if initial.0.is_some() {
                device_name = read_device_name();
                break;
            }
        }
        if initial.0.is_none() {
            eprintln!("[razer-tray] device not present at startup, exiting (--quit-on-disconnect)");
            std::process::exit(0);
        }
    }
    let (level, charging) = initial;
    log_info!(
        "initial: level={:?} charging={} device={:?}",
        level,
        charging,
        device_name
    );
    let mut device_name_resolved = device_name.is_some();
    {
        let mut s = state.lock().unwrap();
        s.level = level;
        s.charging = charging;
        s.device_name = device_name;
    }

    let tray = RazerTray {
        state: state.clone(),
    };

    let service = TrayService::new(tray);
    let handle = service.handle();
    thread::spawn(move || {
        if let Err(e) = service.run() {
            eprintln!("[razer-tray] tray registration failed: {}", e);
            eprintln!("[razer-tray] continuing without tray icon (notifications still work)");
        }
    });

    loop {
        thread::sleep(Duration::from_secs(POLL_INTERVAL_SECS));

        let (raw_level, charging) = read_battery();
        let new_name = if device_name_resolved {
            None
        } else {
            let n = read_device_name();
            if n.is_some() {
                device_name_resolved = true;
            }
            n
        };

        if quit_on_disconnect && raw_level.is_none() {
            log_info!("device disconnected, exiting (--quit-on-disconnect)");
            std::process::exit(0);
        }

        let mut s = state.lock().unwrap();

        let prev_level = s.level;
        let prev_charging = s.charging;
        let prev_name = s.device_name.clone();

        let level = match (raw_level, prev_level) {
            (Some(0), Some(prev)) if !charging && prev >= SLEEP_DETECTION_MIN_DROP => {
                log_info!(
                    "sleep detected (sysfs returned 0%, keeping previous {}%)",
                    prev
                );
                Some(prev)
            }
            _ => raw_level,
        };

        s.level = level;
        s.charging = charging;
        if new_name.is_some() {
            s.device_name = new_name;
        }

        let pending_notify = match level {
            Some(l) if l <= LOW_BATTERY_THRESHOLD && !charging && !s.low_notified => {
                s.low_notified = true;
                let name = s
                    .device_name
                    .clone()
                    .unwrap_or_else(|| DEFAULT_DEVICE_NAME.to_string());
                Some((l, name))
            }
            Some(l) => {
                if l > LOW_BATTERY_THRESHOLD {
                    s.low_notified = false;
                }
                None
            }
            None => None,
        };

        let changed =
            prev_level != s.level || prev_charging != s.charging || prev_name != s.device_name;
        let log_level = s.level;
        let log_charging = s.charging;
        let log_name = s.device_name.clone();

        drop(s);

        if let Some((l, name)) = pending_notify {
            notify_low_battery(l, &name);
        }

        if changed {
            log_info!(
                "state change: level={:?} charging={} device={:?}",
                log_level,
                log_charging,
                log_name
            );
            handle.update(|_| {});
        }
    }
}
