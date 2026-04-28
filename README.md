# razer-tray

[![CI](https://github.com/Softer/razer-tray/actions/workflows/ci.yml/badge.svg)](https://github.com/Softer/razer-tray/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Minimal tray indicator for Razer wireless device battery (mouse, keyboard) on Linux.

Reads directly from kernel sysfs (the `openrazer-driver-dkms` module) without depending on `openrazer-daemon`. A single Rust binary, no async runtime, no GUI frameworks.

## Features

- Battery icon in the system tray (StatusNotifierItem - KDE, MATE, XFCE, GNOME via extension, i3/Sway via waybar/i3status)
- Multi-device support (mouse + keyboard): one tray icon plus a radio menu to pick the active one
- Selection persisted across restarts (`$XDG_CONFIG_HOME/razer-tray/selected_device`)
- Standard freedesktop fallback icons: `battery-full`, `battery-good`, `battery-medium`, `battery-low`, `battery-caution`, `battery-full-charging`, `battery-missing`
- `Critical` notification at <= 20% **per device**, independent of the selected one (once per discharge cycle)
- 1 Hz polling (the tray only refreshes on a real state change)

## Requirements

- Linux with systemd (for the systemd user service; the .desktop autostart works without systemd too)
- The `razermouse` and/or `razerkbd` kernel modules from `openrazer-driver-dkms`
- A D-Bus session (for the tray and notifications)
- An icon theme with freedesktop battery-* icons (Adwaita, Breeze, Papirus, most modern themes) - only used as a fallback when the bundled PNG icons cannot be located

```bash
sudo modprobe razermouse razerkbd
ls /sys/bus/hid/drivers/razermouse/ /sys/bus/hid/drivers/razerkbd/
```

If both directories are empty the devices are either not connected or not supported by the openrazer driver.

## Installation

### From a published release (Arch Linux, x86_64)

```bash
sudo pacman -U https://github.com/Softer/razer-tray/releases/latest/download/razer-tray-0.4.0-1-x86_64.pkg.tar.zst
```

Use the version you actually want, see the [release list](https://github.com/Softer/razer-tray/releases). The release workflow builds `.pkg.tar.zst` from source on every `vX.Y.Z` tag and attaches it to the GitHub Release. `pacman -U` installs the service, autostart, and udev rule in one go.

### From source

```bash
git clone <repo-url> razer-tray
cd razer-tray
cargo build --release
sudo install -Dm755 target/release/razer-tray /usr/local/bin/razer-tray
```

### Building the Arch Linux package

```bash
cd arch
makepkg -si
```

`makepkg -si` builds the package, installs it, and on top of that:
- Enables the `razer-tray.service` systemd user unit globally (for all current and future users)
- Drops an XDG autostart entry under `/etc/xdg/autostart/` (fallback for non-systemd sessions)
- Tries to start the service for every logged-in user immediately after install

The icon shows up in the tray automatically on the next graphical login.

## Usage

```
razer-tray [OPTIONS]

OPTIONS:
    -h, --help                 Print this help message and exit
    -V, --version              Print version and exit
    -v, --verbose              Print info logs to stdout
    -q, --quit-on-disconnect   Exit cleanly when no Razer devices remain
                               in sysfs (use together with the udev rule)
```

### Multiple devices

When more than one Razer device with a battery is detected (for example, a wireless mouse plus a wireless keyboard) razer-tray keeps **a single** tray icon showing the level of the **selected** device. The context menu lists every detected device as a radio item with its current level; clicking another device flips the icon and tooltip to it.

The selection is persisted in `$XDG_CONFIG_HOME/razer-tray/selected_device` (or `~/.config/razer-tray/selected_device`). The format is one line `vendor:product`, e.g. `1532:00B8`. On restart razer-tray looks for a device with that `vendor:product` and makes it active; if it is not connected, it falls back to the first discovered device.

Low-battery notifications (<= 20%) fire **per device** regardless of which one drives the icon, so a keyboard cannot quietly drain while the tray shows a mouse.

**Edge case:** two devices of the exact same model (same `vendor:product`) - recall picks the first one in sysfs glob order. Within a session you can still switch between them via the menu.

### `--quit-on-disconnect` + udev

By default razer-tray polls sysfs once a second and shows a "not found" icon while no Razer device is connected. With `--quit-on-disconnect` the service **exits** when every Razer device disappears from sysfs, and the udev rule (shipped with the Arch package) **relaunches it** when any device returns. Benefits:

- No background process when no device is around
- No zombie icon in the tray
- Reconnection is instant via udev, not up to one second of polling

**Enable:**

1. Add the flag to the systemd unit:
   ```bash
   systemctl --user edit razer-tray.service
   ```
   Override content:
   ```
   [Service]
   ExecStart=
   ExecStart=/usr/bin/razer-tray --quit-on-disconnect
   ```
2. Reload and restart:
   ```bash
   systemctl --user daemon-reload
   systemctl --user restart razer-tray.service
   ```

The udev rule (`/usr/lib/udev/rules.d/99-razer-tray.rules`) ships with the package and activates system-wide. When any Razer device shows up (driver matching `razer*` - mouse, keyboard, etc.) it triggers `/usr/lib/razer-tray/udev-trigger`, which starts the service for every active user session. Devices without a `charge_level` (mousepads, for instance) are silently ignored by razer-tray itself.

### Running via systemd (recommended)

```bash
# Enable autostart and run now
systemctl --user enable --now razer-tray.service

# Tail logs
journalctl --user -u razer-tray.service -f

# Stop
systemctl --user stop razer-tray.service
```

### Manual run

```bash
razer-tray              # normal start
razer-tray --verbose    # with info logs to stdout
```

## Configuration

Parameters live as constants at the top of `src/main.rs`:

| Constant | Value | Purpose |
|---|---|---|
| `POLL_INTERVAL_SECS` | 1 | sysfs poll interval in seconds |
| `LOW_BATTERY_THRESHOLD` | 20 | low-battery notification threshold, % |
| `SLEEP_DETECTION_MIN_DROP` | 5 | minimum delta (%) used to recognise "device asleep, sysfs returned 0" |
| `SYSFS_DRIVERS` | `["razermouse", "razerkbd"]` | which openrazer drivers to scan |

The single runtime-state file is `$XDG_CONFIG_HOME/razer-tray/selected_device` (one line `vendor:product`). It is safe to delete: razer-tray will simply pick the first discovered device on the next start.

## Uninstall

```bash
# Pacman package
sudo pacman -R razer-tray

# Manual install
systemctl --user disable --now razer-tray.service
sudo rm /usr/local/bin/razer-tray
```

## License

MIT - see [LICENSE](LICENSE).
