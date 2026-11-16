# MX002 — Linux user-space driver for a graphics tablet

[Русская версия](README.md)

A user-space driver for the graphics tablet sold in Brazil as the Multilaser MX002. It also works with other rebranded versions of the same device. The driver reads raw tablet reports over USB and creates virtual input devices (pen and buttons) that X11, Wayland and any application can use.

Two modes:

- **draw** — drawing: full active area, pressure, pen proximity like regular tablets;
- **osu** — osu! gameplay: higher sensitivity and adaptive smoothing that damps jitter without lagging fast movements.

## Supported devices

- Multilaser MX002
- VINSA 1060Plus
- Gotop Information Inc. [T501] Driver Inside Tablet
- 10moons G10 / 1060plus

Check whether your tablet is supported:

```bash
lsusb | grep 08f2:6811
```

## Features

- Uses the tablet's full active area.
- All physical tablet buttons and both stylus buttons work.
- Pressure sensitivity: the hardware reports 1024 levels (not the advertised 8192), roughly 700 of which are safely usable.
- All pen events go out in a single frame, so position, pressure and touch never disagree.
- Phantom-touch protection: a touch needs several consecutive over-threshold reports.
- Touch hysteresis (strokes do not break at the lightest pressure) and a configurable pressure curve.
- Pen proximity events (`BTN_TOOL_PEN`).
- Buttons sharing a modifier (e.g. Ctrl) do not release it while another button still holds it.
- One-command calibration for your pen (`calibrate`).

## Installation

Requires Rust (cargo), `libusb` and `pkg-config`.

```bash
# Arch / CachyOS
sudo pacman -S rust libusb pkgconf
# Debian / Ubuntu
sudo apt install cargo libusb-1.0-0-dev pkg-config

git clone https://github.com/dan4ikk3/mx002_linux_driver_cachyos.git
cd mx002_linux_driver_cachyos
cargo build --release
```

The driver is built to `target/release/mx002`.

## Usage

```bash
sudo ./target/release/mx002                 # drawing mode
sudo ./target/release/mx002 --mode osu      # osu! mode
```

To run without `sudo`, add a udev rule (may vary by distribution), then replug the tablet:

```bash
echo 'SUBSYSTEM=="usb", ATTRS{idVendor}=="08f2", ATTRS{idProduct}=="6811", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/99-mx002.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

If the driver complains about `/dev/uinput` without `sudo`, load the module (`sudo modprobe uinput`) and add:

```bash
echo 'KERNEL=="uinput", SUBSYSTEM=="misc", OPTIONS+="static_node=uinput", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/99-uinput.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### Helper script

```bash
./switch-mode.sh draw             # build and run in drawing mode
./switch-mode.sh osu              # build and run in osu! mode
./switch-mode.sh calibrate        # tune for your pen
./switch-mode.sh dump             # diagnostics: raw tablet reports
./switch-mode.sh build            # build only
./switch-mode.sh draw --max 650   # extra arguments go to the driver
```

The script runs the driver without `sudo`, so it needs the udev rule above (or build with `build` and run the binary with `sudo` yourself).

## Options

| Option | Default | Description |
|---|---|---|
| `--mode draw\|osu` | `draw` | Operating mode. |
| `--base N` | `1740` | Raw pressure value when the pen is not touching. |
| `--threshold N` | `150` | Touch threshold above `base`. |
| `--max N` | `700` | Press depth that gives 100% pressure. |
| `--curve F` | `1.0` | Pressure curve. 1.0 linear, below 1.0 softer, above stiffer. |
| `--debounce N` | `2` | Consecutive over-threshold reports needed to start a touch (1–10). 1 disables phantom-touch protection. |
| `--smooth F` | `0` draw, `0.5` osu | Position smoothing 0.0–0.9; fast moves are not lagged. |
| `--scale F` | `8.0` | Coordinate multiplier in osu! mode; larger means a smaller active area. |
| `--max-x N`, `--max-y N` | `4096` | Raw coordinate extents of the tablet. |
| `--size WxH` | unset | Physical active area in mm, e.g. `254x158`; sets axis resolution so applications can keep the aspect ratio. |
| `--no-proximity` | off | Do not send pen in/out of range events. |
| `--prox-timeout MS` | `100` | Milliseconds without reports before the pen is considered out of range. |
| `--calibrate` | | Show your pen's pressure and coordinate ranges and exit. |
| `--dump` | | Print raw tablet reports (diagnostics) and exit. |

## Tuning for your pen

1. Run `./switch-mode.sh calibrate`.
2. Sweep the pen over all edges of the active area and press as you normally draw (do not press harder than usual; above about 700 levels the pen may be damaged). Press `Ctrl+C`.
3. The driver prints recommended `--base`, `--threshold`, `--max` and `--max-x`/`--max-y`. Use them, e.g.:

   ```bash
   ./switch-mode.sh draw --base 1740 --threshold 120 --max 650 --curve 0.8
   ```

## Troubleshooting

- **Phantom touches:** raise `--debounce` (3–4) or `--threshold`. If they occur while the pen is lifted, attach the output of `./switch-mode.sh dump` to an issue.
- **Pen reacts too high above the tablet:** hover range is decided by the tablet firmware, and the reports known to us contain no height data, so the driver can only detect "out of range" by a pause in reports (`--prox-timeout`). To look for a height byte, run `./switch-mode.sh dump`, hold the pen over the tablet, lift it slowly to its maximum height, remove it, then press `Ctrl+C` and attach the output to an issue.
- **Pressure too strong or weak:** tune `--max` (lower is more sensitive) and `--curve` (below 1.0 is softer).
- **osu! cursor jitters or feels sluggish:** tune `--smooth` (higher damps jitter, lower is snappier; `0` disables).
- **Cursor stops short of or runs past the tablet edge:** check `--max-x`/`--max-y` with `calibrate`.
- **Odd pen behaviour:** try `--no-proximity`.

## Multiple monitors

On X11, find the `virtual_tablet Pen` id with `xinput` and map it:

```bash
xinput map-to-output 10 "DP-2"
```

On Wayland, the output area is configured in your desktop environment's tablet settings (e.g. KDE Plasma).

## Buttons

Tablet buttons send keys (logical numbers, mapping in `src/virtual_device.rs`): 0 Tab, 1 Space, 2 Alt, 3 Ctrl, 4 Scroll Down, 5 Scroll Up, 6 `[`, 7 Ctrl+Num−, 8 Ctrl+Num+, 9 E, 12 B, 13 `]`. Pen buttons: `BTN_STYLUS`, `BTN_STYLUS2`.

## Development

```bash
cargo build --release
cargo test
```

## References

- [Tool that enables expanded mode for the tablet, by DIGImend](https://github.com/DIGImend/10moons-tools)
- [Example user-space driver, by alex-s-v](https://github.com/alex-s-v/10moons-driver)
- Original driver [marvinbelfort/mx002_linux_driver](https://github.com/marvinbelfort/mx002_linux_driver) this fork is based on.
