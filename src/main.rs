use glob::glob;
use ksni::{self, MenuItem, Tray, TrayService};
use notify_rust::{Notification, Urgency};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const SYSFS_DRIVERS: &[&str] = &["razermouse", "razerkbd"];
const POLL_INTERVAL_SECS: u64 = 1;
const LOW_BATTERY_THRESHOLD: u8 = 20;
const SLEEP_DETECTION_MIN_DROP: u8 = 5;

const STARTUP_GRACE_RETRIES: u8 = 5;
const STARTUP_GRACE_INTERVAL_SECS: u64 = 1;

const STATE_FILENAME: &str = "selected_device";
const DEFAULT_DEVICE_NAME: &str = "Razer Device";

static VERBOSE: AtomicBool = AtomicBool::new(false);
static QUIT_ON_DISCONNECT: AtomicBool = AtomicBool::new(false);

macro_rules! log_info {
    ($($arg:tt)*) => {
        if VERBOSE.load(Ordering::Relaxed) {
            println!("[razer-tray] {}", format_args!($($arg)*));
        }
    };
}

const HELP_TEXT: &str = "\
razer-tray - tray indicator for Razer wireless device battery

USAGE:
    razer-tray [OPTIONS]

OPTIONS:
    -h, --help                 Print this help message and exit
    -V, --version              Print version and exit
    -v, --verbose              Print info logs to stdout
    -q, --quit-on-disconnect   Exit cleanly when no Razer devices remain
                               in sysfs. Combine with the udev rule
                               shipped in the package to relaunch on
                               hotplug instead of polling forever.
";

#[derive(Clone)]
struct DeviceState {
    sysfs_id: String,
    persistent_id: String,
    driver: &'static str,
    level: Option<u8>,
    charging: bool,
    name: Option<String>,
    low_notified: bool,
    last_raw_level: Option<u8>,
}

struct MultiState {
    devices: Vec<DeviceState>,
    selected_id: Option<String>,
}

struct DiscoveredDevice {
    sysfs_id: String,
    persistent_id: String,
    driver: &'static str,
}

struct RazerTray {
    state: Arc<Mutex<MultiState>>,
}

impl RazerTray {
    fn selected(&self) -> Option<DeviceState> {
        let s = self.state.lock().unwrap();
        let id = s.selected_id.as_ref()?;
        s.devices.iter().find(|d| &d.sysfs_id == id).cloned()
    }
}

fn format_device_label(d: &DeviceState) -> String {
    let name = d.name.as_deref().unwrap_or(DEFAULT_DEVICE_NAME);
    match (d.level, d.charging) {
        (None, _) => format!("{}: not found", name),
        (Some(l), true) => format!("{}: {}% (charging)", name, l),
        (Some(l), false) => format!("{}: {}%", name, l),
    }
}

impl Tray for RazerTray {
    fn icon_name(&self) -> String {
        let sel = self.selected();
        if let Some(icons) = get_icons() {
            let lvl = sel.as_ref().and_then(|d| d.level);
            if lvl.is_some() || icons.missing.is_some() {
                return String::new();
            }
        }
        match sel {
            None => "battery-missing".into(),
            Some(d) => match (d.level, d.charging) {
                (None, _) => "battery-missing".into(),
                (Some(_), true) => "battery-full-charging".into(),
                (Some(l), false) if l > 80 => "battery-full".into(),
                (Some(l), false) if l > 60 => "battery-good".into(),
                (Some(l), false) if l > 40 => "battery-medium".into(),
                (Some(l), false) if l > 20 => "battery-low".into(),
                (Some(_), false) => "battery-caution".into(),
            },
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let Some(icons) = get_icons() else {
            return vec![];
        };
        let sel = self.selected();
        let icon = match sel.as_ref().and_then(|d| d.level.map(|l| (l, d.charging))) {
            Some((level, charging)) => {
                let idx = (level as usize).min(100);
                if charging {
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
        let sel = self.selected();
        let title = match sel {
            None => "No Razer device".into(),
            Some(d) => format_device_label(&d),
        };
        ksni::ToolTip {
            title,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = self.state.lock().unwrap();
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        if s.devices.is_empty() {
            items.push(MenuItem::Standard(ksni::menu::StandardItem {
                label: "No Razer devices".into(),
                enabled: false,
                ..Default::default()
            }));
        } else {
            let device_ids: Vec<String> = s.devices.iter().map(|d| d.sysfs_id.clone()).collect();
            let selected_idx = s
                .selected_id
                .as_ref()
                .and_then(|sel| s.devices.iter().position(|d| &d.sysfs_id == sel))
                .unwrap_or(0);
            let options: Vec<ksni::menu::RadioItem> = s
                .devices
                .iter()
                .map(|d| ksni::menu::RadioItem {
                    label: format_device_label(d),
                    ..Default::default()
                })
                .collect();
            items.push(MenuItem::RadioGroup(ksni::menu::RadioGroup {
                selected: selected_idx,
                select: Box::new(move |this: &mut RazerTray, idx: usize| {
                    if let Some(id) = device_ids.get(idx) {
                        on_menu_click(&this.state, id.clone());
                    }
                }),
                options,
            }));
        }
        items.push(MenuItem::Separator);
        items.push(MenuItem::Standard(ksni::menu::StandardItem {
            label: "Quit".into(),
            activate: Box::new(|_| std::process::exit(0)),
            ..Default::default()
        }));
        items
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

fn read_device_sysfs(driver: &str, sysfs_id: &str, filename: &str) -> Option<String> {
    let path = format!("/sys/bus/hid/drivers/{}/{}/{}", driver, sysfs_id, filename);
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

fn read_battery_for(driver: &str, sysfs_id: &str) -> (Option<u8>, bool) {
    let level =
        read_device_sysfs(driver, sysfs_id, "charge_level").and_then(|s| s.parse::<u8>().ok());
    let charging = read_device_sysfs(driver, sysfs_id, "charge_status")
        .map(|s| !s.is_empty() && s != "0")
        .unwrap_or(false);
    (level, charging)
}

fn read_device_name_for(driver: &str, sysfs_id: &str) -> Option<String> {
    read_device_sysfs(driver, sysfs_id, "device_type").filter(|s| !s.is_empty())
}

fn extract_persistent_id(sysfs_id: &str) -> Option<String> {
    let main = sysfs_id.split('.').next()?;
    let parts: Vec<&str> = main.split(':').collect();
    if parts.len() == 3 && !parts[1].is_empty() && !parts[2].is_empty() {
        Some(format!("{}:{}", parts[1], parts[2]))
    } else {
        None
    }
}

fn discover() -> Vec<DiscoveredDevice> {
    let mut out = Vec::new();
    for &drv in SYSFS_DRIVERS {
        let pattern = format!("/sys/bus/hid/drivers/{}/*", drv);
        let Ok(paths) = glob(&pattern) else { continue };
        for path in paths.flatten() {
            if !path.join("charge_level").exists() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(pid) = extract_persistent_id(name) else {
                continue;
            };
            out.push(DiscoveredDevice {
                sysfs_id: name.to_string(),
                persistent_id: pid,
                driver: drv,
            });
        }
    }
    out
}

fn state_file_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("razer-tray").join(STATE_FILENAME))
}

fn load_persisted_selection() -> Option<String> {
    let path = state_file_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let trimmed = content.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some(trimmed.to_string())
    } else {
        log_info!("persisted selection malformed: {:?}", trimmed);
        None
    }
}

fn save_persisted_selection(persistent_id: &str) {
    let Some(path) = state_file_path() else {
        log_info!("no XDG_CONFIG_HOME or HOME, persistence disabled");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            log_info!("failed to create state dir {}: {}", parent.display(), e);
            return;
        }
    }
    if let Err(e) = fs::write(&path, persistent_id) {
        log_info!(
            "failed to write persisted selection {}: {}",
            path.display(),
            e
        );
    }
}

fn on_menu_click(state: &Arc<Mutex<MultiState>>, sysfs_id: String) {
    let new_pid = {
        let mut s = state.lock().unwrap();
        if s.selected_id.as_ref() == Some(&sysfs_id) {
            return;
        }
        let pid = match s.devices.iter().find(|d| d.sysfs_id == sysfs_id) {
            Some(d) => d.persistent_id.clone(),
            None => {
                log_info!("ignoring stale menu click for {}", sysfs_id);
                return;
            }
        };
        s.selected_id = Some(sysfs_id);
        pid
    };
    log_info!("user selected device, persisting {}", new_pid);
    save_persisted_selection(&new_pid);
}

fn notify_low_battery(level: u8, device_name: &str) {
    log_info!("notification: low battery {}% on {}", level, device_name);
    if let Err(e) = Notification::new()
        .summary(&format!("{}: Low Battery", device_name))
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

    let quit_on_disconnect = QUIT_ON_DISCONNECT.load(Ordering::Relaxed);
    let mut discovered = discover();
    if quit_on_disconnect && discovered.is_empty() {
        for _ in 0..STARTUP_GRACE_RETRIES {
            thread::sleep(Duration::from_secs(STARTUP_GRACE_INTERVAL_SECS));
            discovered = discover();
            if !discovered.is_empty() {
                break;
            }
        }
        if discovered.is_empty() {
            eprintln!(
                "[razer-tray] no Razer devices present at startup, exiting (--quit-on-disconnect)"
            );
            std::process::exit(0);
        }
    }
    log_info!("discovered {} device(s) at startup", discovered.len());

    let devices: Vec<DeviceState> = discovered
        .into_iter()
        .map(|d| {
            let driver = d.driver;
            let (level, charging) = read_battery_for(driver, &d.sysfs_id);
            let name = read_device_name_for(driver, &d.sysfs_id);
            log_info!(
                "  {} ({}): level={:?} charging={} name={:?}",
                d.sysfs_id,
                driver,
                level,
                charging,
                name
            );
            DeviceState {
                sysfs_id: d.sysfs_id,
                persistent_id: d.persistent_id,
                driver,
                level,
                charging,
                name,
                low_notified: false,
                last_raw_level: level,
            }
        })
        .collect();

    let persisted = load_persisted_selection();
    if let Some(p) = persisted.as_ref() {
        log_info!("loaded persisted selection: {}", p);
    }
    let selected_id = persisted
        .as_ref()
        .and_then(|pid| {
            devices
                .iter()
                .find(|d| &d.persistent_id == pid)
                .map(|d| d.sysfs_id.clone())
        })
        .or_else(|| devices.first().map(|d| d.sysfs_id.clone()));
    if persisted.is_some() && selected_id.as_ref().is_some() {
        let matched_pid = selected_id
            .as_ref()
            .and_then(|sid| devices.iter().find(|d| &d.sysfs_id == sid))
            .map(|d| &d.persistent_id);
        if matched_pid != persisted.as_ref() {
            log_info!(
                "persisted selection {:?} not present, using first discovered",
                persisted
            );
        }
    }
    if let Some(id) = &selected_id {
        log_info!("initial selection: {}", id);
    }

    if quit_on_disconnect && devices.is_empty() {
        eprintln!(
            "[razer-tray] no Razer devices with battery present, exiting (--quit-on-disconnect)"
        );
        std::process::exit(0);
    }

    let mut last_selected = selected_id.clone();
    let state = Arc::new(Mutex::new(MultiState {
        devices,
        selected_id,
    }));

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

        let discovered = discover();

        if quit_on_disconnect && discovered.is_empty() {
            log_info!("all devices disconnected, exiting (--quit-on-disconnect)");
            std::process::exit(0);
        }

        let mut s = state.lock().unwrap();
        let mut changed = false;

        let before = s.devices.len();
        s.devices
            .retain(|d| discovered.iter().any(|disc| disc.sysfs_id == d.sysfs_id));
        if s.devices.len() != before {
            log_info!("removed {} gone device(s)", before - s.devices.len());
            changed = true;
        }

        for disc in &discovered {
            if !s.devices.iter().any(|d| d.sysfs_id == disc.sysfs_id) {
                let driver = disc.driver;
                let sysfs_id = disc.sysfs_id.clone();
                let (level, charging) = read_battery_for(driver, &sysfs_id);
                let name = read_device_name_for(driver, &sysfs_id);
                log_info!(
                    "new device: {} ({}) level={:?} name={:?}",
                    sysfs_id,
                    driver,
                    level,
                    name
                );
                s.devices.push(DeviceState {
                    sysfs_id,
                    persistent_id: disc.persistent_id.clone(),
                    driver,
                    level,
                    charging,
                    name,
                    low_notified: false,
                    last_raw_level: level,
                });
                changed = true;
            }
        }

        let selected_present = s
            .selected_id
            .as_ref()
            .map(|id| s.devices.iter().any(|d| &d.sysfs_id == id))
            .unwrap_or(false);
        if !selected_present {
            let new_sel = s.devices.first().map(|d| d.sysfs_id.clone());
            if s.selected_id != new_sel {
                match &new_sel {
                    Some(id) => log_info!("selected device gone, falling back to {}", id),
                    None => log_info!("selected device gone, no devices remaining"),
                }
                s.selected_id = new_sel;
                changed = true;
            }
        }

        let mut pending_notifies: Vec<(u8, String)> = Vec::new();
        for d in s.devices.iter_mut() {
            let prev_level = d.level;
            let prev_charging = d.charging;
            let prev_name = d.name.clone();

            let (raw_level, charging) = read_battery_for(d.driver, &d.sysfs_id);
            if d.name.is_none() {
                if let Some(n) = read_device_name_for(d.driver, &d.sysfs_id) {
                    d.name = Some(n);
                }
            }

            let post_sleep = match (raw_level, prev_level) {
                (Some(0), Some(p)) if !charging && p >= SLEEP_DETECTION_MIN_DROP => {
                    log_info!("{}: sleep detected, keeping {}%", d.sysfs_id, p);
                    Some(p)
                }
                _ => raw_level,
            };

            let new_level = if post_sleep == prev_level
                || prev_level.is_none()
                || d.last_raw_level == post_sleep
            {
                post_sleep
            } else {
                log_info!(
                    "{}: level change pending raw={:?}, awaiting confirmation",
                    d.sysfs_id,
                    post_sleep
                );
                prev_level
            };
            d.last_raw_level = post_sleep;
            d.level = new_level;
            d.charging = charging;

            if let Some(l) = new_level {
                if l <= LOW_BATTERY_THRESHOLD && !charging && !d.low_notified {
                    d.low_notified = true;
                    let name = d
                        .name
                        .clone()
                        .unwrap_or_else(|| DEFAULT_DEVICE_NAME.to_string());
                    pending_notifies.push((l, name));
                }
                if l > LOW_BATTERY_THRESHOLD {
                    d.low_notified = false;
                }
            }

            if d.level != prev_level || d.charging != prev_charging || d.name != prev_name {
                changed = true;
            }
        }

        if s.selected_id != last_selected {
            log_info!(
                "selection changed: {:?} -> {:?}",
                last_selected,
                s.selected_id
            );
            last_selected = s.selected_id.clone();
            changed = true;
        }

        drop(s);

        for (l, name) in pending_notifies {
            notify_low_battery(l, &name);
        }

        if changed {
            handle.update(|_| {});
        }
    }
}
