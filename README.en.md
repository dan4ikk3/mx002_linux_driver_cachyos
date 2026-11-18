# MX002 — Linux user-space driver for a graphics tablet

[Русская версия](README.md)

A user-space driver for the graphics tablet sold in Brazil as the Multilaser MX002. It also works with other rebranded versions of the same device. The driver reads raw tablet reports over USB and creates virtual input devices (pen and buttons) that X11, Wayland and any application can use.

Two modes:

- **draw** — drawing: full active area, pressure, phantom-touch protection;
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
- Optional pen proximity events (`BTN_TOOL_PEN`, `--proximity`).
- Garbage reports from outside the sensing range (impossible pressure values) are not treated as a press.
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
| `--valid-max N` | `1100` | If `base − raw` exceeds this, the report is treated as garbage (pen out of range), not a press. Must be larger than `--max`. |
| `--proximity` | off | Send pen in/out of range events (detected by a pause in reports; enable only if needed). |
| `--prox-timeout MS` | `100` | With `--proximity` only: milliseconds without reports before the pen counts as out of range. |
| `--verbose`, `-v` | | Print touch start/end (with raw values) and discarded reports. Diagnostics. |
| `--calibrate` | | Phase-based calibration (out of range, hovering, drawing): tunes the options and exits. |
| `--dump` | | Print raw tablet reports (diagnostics) and exit. |

## Tuning for your pen

Defaults do not fit every pen and tablet. Calibrate (about one minute):

1. Run `./switch-mode.sh calibrate`.
2. Follow the three phases (3 seconds to get ready before each):
   - **Phase 1/3, 4 s:** remove the pen from the tablet completely and do not touch it.
   - **Phase 2/3, 6 s:** hold the pen 1–3 cm above the tablet and move it slowly **without touching**.
   - **Phase 3/3, 12 s:** draw as usual: strokes over the whole area, light and hard presses, reach the edges. Do not press harder than usual (above about 700 levels the pen may be damaged).
3. The driver prints a ready command, e.g. `mx002 --base 1740 --threshold 120 --max 650 --valid-max 900 --max-x 4096 --max-y 4096`. Run the driver with these options.
4. If phase 2 shows that hovering gives a press almost as strong as drawing, the driver warns you: the threshold has to be high.

## Pen clicks when lifted, "sticks" or presses by itself

The tablet keeps sending reports while the pen is within sensor range (several centimetres), and a touch is detected only from the raw pressure value. Noise above the tablet or garbage values outside the range can exceed the threshold, so the cursor stops and applications see a click. In order:

1. **Update and rebuild:** `git pull && cargo build --release`.
2. **See what happens:** `sudo ./target/release/mx002 --mode osu --verbose`. Each touch prints a line like `touch STARTED raw=1500 delta=240 x=... y=...`. Move the pen away: if a touch starts while the pen is lifted, note its `delta` (the "press" caused by interference). A line saying pressure is out of the valid range means a garbage report was discarded (normal).
3. **Calibrate** (see above), the most reliable way.
4. **Or tune manually:**
   - false touch with a small `delta` (e.g. 200–400): raise `--threshold` above it with a 20–30 margin;
   - false touch with a huge `delta` (above any real press, e.g. 1700 at `raw=0`): lower `--valid-max` below it but above `--max`;
   - rare single spikes: raise `--debounce` to 3–4 (touch starts 1–2 reports later).
5. **Do not enable `--proximity` unless needed.** It detects "out of range" by a pause in reports; if the tablet only reports on change, the pen "leaves" in the middle of a still press and the click is lost. It is off by default.
6. **osu!:** to rule out smoothing, try `--smooth 0`; if it helps, tune 0.2–0.3.
7. **Still broken?** Run `./switch-mode.sh dump`, hold the pen over the tablet, lift it slowly to the maximum height and remove it, then `Ctrl+C`. Attach the dump and the `--verbose` output to an issue.

**Cursor moves with the pen lifted 4–5 cm:** hover range is decided by the tablet firmware, and the reports known to us contain no height data, so the driver can only avoid counting a click (above) but cannot stop the cursor at a height. If the dump reveals a byte that changes while lifting the pen, such a limit can be added.

## Troubleshooting

- **Clicks when the pen is lifted, sticking, phantom touches:** see the section above.
- **Cursor moves with the pen lifted 4–5 cm:** see the section above; hover range is decided by the tablet firmware.
- **Pressure too strong or weak:** tune `--max` (lower is more sensitive) and `--curve` (below 1.0 is softer).
- **osu! cursor jitters or feels sluggish:** tune `--smooth` (higher damps jitter, lower is snappier; `0` disables).
- **Cursor stops short of or runs past the tablet edge:** check `--max-x`/`--max-y` with `calibrate`.
- **Access errors for the tablet or `/dev/uinput`:** run with `sudo` or set up the udev rules (see above).

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
