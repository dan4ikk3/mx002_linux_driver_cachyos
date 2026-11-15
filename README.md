# MX002 Linux User-Space Driver

This README provides information about the Linux user-space driver for the graphics tablet sold in Brazil as the Multilaser MX002. The driver supports various rebranded versions of the same device.

# Changes made (fork)

- Режимы `draw` и `osu` теперь выбираются аргументом `--mode`, один исходник вместо двух копий.
- Все события пера (X, Y, давление, касание, кнопки) уходят одним кадром, а не 4–5 отдельными.
- В режиме рисования `BTN_TOUCH` не был объявлен у виртуального устройства, и ядро молча его отбрасывало. Исправлено.
- Шкала давления: 100% достигается при `--max` (по умолчанию 700 — то, что реально используется), гистерезис касания, настраиваемая кривая `--curve`.
- Корректное завершение при отключении планшета (раньше — загрузка CPU на 100%), быстрый выход по Ctrl+C.

## Как у обычных планшетов (draw)

- Перо отправляет вход/выход из зоны (`BTN_TOOL_PEN`): `--prox-timeout MS` (по умолчанию 100), отключается `--no-proximity`.
- Устройства представляются как USB `08f2:6811`.
- Выход пера из зоны сейчас определяется по паузе в отчётах. Чтобы сделать это точно, запустите `./switch-mode.sh dump` (подержите перо над планшетом, уберите его, понажимайте кнопки, затем Ctrl+C) и приложите вывод к issue.
- Размер области и пропорции: `--max-x/--max-y` (сырые координаты, проверьте через `calibrate`) и `--size WxH` (мм). С `--size` оси получают реальное разрешение, и компоситор может сохранять пропорции.
- Кнопки с общим модификатором (Ctrl на кнопках 3, 7, 8) больше не отпускают Ctrl, пока удерживается другая.

## osu!

- Включено адаптивное сглаживание позиции (`--smooth 0.5`, диапазон 0.0..0.9): дрожание сенсора, которое множитель `--scale` усиливает в 8 раз, подавляется, а быстрые движения проходят без задержки. Для чистой «сырой» позиции: `--smooth 0`.
- Время чтения USB сокращено до 25 мс, события отправляются только при изменении.

## Настройка давления

```bash
./switch-mode.sh calibrate        # понажимайте пером как при рисовании, затем Ctrl+C
./switch-mode.sh draw --base 1740 --threshold 120 --max 650 --curve 0.8
```

`--calibrate` печатает рекомендуемые `--base/--threshold/--max` для вашего пера. Не давите сильнее обычного: по оценке автора оригинала, выше ~700 уровней есть риск повредить перо.

## Supported Devices

This driver is intended for use with the following devices:
- Multilaser MX002
- VINSA 1060Plus
- Gotop Information Inc. [T501] Driver Inside Tablet
- 10moons G10/1060plus

To check if this driver is compatible with your device, you can run the following command:

```bash
lsusb | grep 08f2:6811
```

If your device is supported, the output should look similar to this:

```
Bus 001 Device 005: ID 08f2:6811 Gotop Information Inc. [T501] Driver Inside Tablet
```

## Features

- Utilizes the tablet's full area.
- All 12 physical buttons are functional.
- Both stylus buttons are operational.
- Supports about 700 levels of pressure sensitivity. While the device is marketed with 8192 levels, the hardware only reports 1024, of which roughly 700 are usable without risking damage to the stylus.

## Installation

To build and run the driver, follow these steps:  
[You will need rust installed](https://wiki.archlinux.org/title/rust)

```bash
git clone git@github.com:marvinbelfort/mx002_linux_driver.git
cd mx002_linux_driver
cargo build --release
```

The driver will be located in `target/release/mx002`.

## Usage

To run the driver, you have two options:

1. **Run as Root:**

   ```bash
   sudo ./mx002
   ```

2. **Create a udev Rule:**

   For Arch Linux (this may vary by distribution):

   ```bash
   echo 'SUBSYSTEM=="usb", ATTRS{idVendor}=="08f2", ATTRS{idProduct}=="6811", TAG+="uaccess"' > /etc/udev/rules.d/99-mx002.rules
   sudo udevadm control --reload-rules
   sudo udevadm trigger
   ```

   This allows any user to run the driver without needing sudo.

## Multiple Monitors:

Show input devices with:
```bash
xinput

⎡ Virtual core pointer                    	id=2	[master pointer  (3)]
... 
⎜   ↳ virtual_tablet Pen (0)                  	id=10	[slave  pointer  (2)]
⎣ Virtual core keyboard                   	id=3	[master keyboard (2)]
...
    ↳ virtual_tablet                          	id=8	[slave  keyboard (3)]
    ↳ virtual_tablet                          	id=9	[slave  keyboard (3)]
```

If you can't see the entry "virtual_tablet Pen", move the pen a bit and run the command again.
Remember it's id (10 in the example)

List existing monitors:
```
xrandr | rg '\bconnected\b'

HDMI-0 connected primary 2560x1080+0+0 (normal left inverted right x axis y axis) 673mm x 284mm
DP-2 connected 1680x1050+2560+84 (normal left inverted right x axis y axis) 473mm x 296mm
```

So, to map the pen's movement only to monitor 2, use:

```bash
xinput map-to-output 10 "DP-2"
```



## TODOs

- Enable more direct configuration of emitted key combinations.
- Implement mapping of the tablet area vs. multiple monitors.
- Exhaust the SET REPORT HID protocol combinations to determine if any of them enable the advertised pressure of 8192. (but I really suspect that's all there is to it.)
- Add support for the 10 virtual buttons.

## References

- [Tool that enables expanded mode for the tablet, by DigiMend](https://github.com/DIGImend/10moons-tools)
- [Learning about the possibility of creating user-space drivers](https://github.com/alex-s-v/10moons-driver)

This code is a combination of the two above, with some improvements.

