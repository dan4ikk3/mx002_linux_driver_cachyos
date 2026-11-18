use std::collections::HashMap;
use std::io::Error;
use std::time::{Duration, Instant};

use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AbsInfo, AbsoluteAxisType, AttributeSet, BusType, EventType, InputEvent, InputId, Key,
    UinputAbsSetup,
};

use crate::config::{Config, Mode};
use crate::filter::{PenFilter, TouchTracker};

const USB_VID: u16 = 0x08f2;
const USB_PID: u16 = 0x6811;

#[derive(Default)]
pub struct RawDataReader {
    pub data: Vec<u8>,
}

impl RawDataReader {
    const X_AXIS_HIGH: usize = 1;
    const X_AXIS_LOW: usize = 2;
    const Y_AXIS_HIGH: usize = 3;
    const Y_AXIS_LOW: usize = 4;
    const PRESSURE_HIGH: usize = 5;
    const PRESSURE_LOW: usize = 6;
    const PEN_BUTTONS: usize = 9;
    const TABLET_BUTTONS_HIGH: usize = 12;
    const TABLET_BUTTONS_LOW: usize = 11;
    /// Минимальная длина отчёта, в которой есть все поля выше.
    pub const MIN_REPORT_LEN: usize = 13;

    pub fn new() -> Self {
        RawDataReader {
            data: vec![0u8; 64],
        }
    }

    fn u16_from_2_u8(&self, high: u8, low: u8) -> u16 {
        (high as u16) << 8 | low as u16
    }

    pub fn x_axis(&self) -> i32 {
        self.u16_from_2_u8(self.data[Self::X_AXIS_HIGH], self.data[Self::X_AXIS_LOW]) as i32
    }

    pub fn y_axis(&self) -> i32 {
        self.u16_from_2_u8(self.data[Self::Y_AXIS_HIGH], self.data[Self::Y_AXIS_LOW]) as i32
    }

    /// Сырое значение давления (чем сильнее нажим, тем оно меньше).
    pub fn pressure(&self) -> i32 {
        self.u16_from_2_u8(
            self.data[Self::PRESSURE_HIGH],
            self.data[Self::PRESSURE_LOW],
        ) as i32
    }

    fn tablet_buttons_as_binary_flags(&self) -> u16 {
        self.u16_from_2_u8(
            self.data[Self::TABLET_BUTTONS_HIGH],
            self.data[Self::TABLET_BUTTONS_LOW],
        ) | (0xcc << 8)
    }

    fn pen_buttons(&self) -> u8 {
        self.data[Self::PEN_BUTTONS]
    }
}

/// Счётчик удержаний клавиш: одна и та же клавиша (Ctrl) может быть на нескольких
/// кнопках планшета, и отпускание одной не должно снимать её, пока держится другая.
#[derive(Default)]
struct KeyRefs(HashMap<u16, u8>);

impl KeyRefs {
    /// true — клавишу надо реально нажать.
    fn press(&mut self, code: u16) -> bool {
        let n = self.0.entry(code).or_insert(0);
        *n += 1;
        *n == 1
    }

    /// true — клавишу надо реально отпустить.
    fn release(&mut self, code: u16) -> bool {
        match self.0.get_mut(&code) {
            Some(n) if *n > 1 => {
                *n -= 1;
                false
            }
            Some(_) => {
                self.0.remove(&code);
                true
            }
            None => false,
        }
    }
}

pub struct DeviceDispatcher {
    config: Config,
    tablet_last_raw_pressed_buttons: u16,
    pen_last_raw_pressed_button: u8,
    tablet_button_id_to_key_code_map: HashMap<u8, Vec<Key>>,
    pen_button_id_to_key_code_map: HashMap<u8, Vec<Key>>,
    virtual_pen: VirtualDevice,
    virtual_keyboard: VirtualDevice,
    key_refs: KeyRefs,
    filter: PenFilter,
    touch: TouchTracker,
    was_touching: bool,
    in_proximity: bool,
    last_report: Instant,
    invalid_pressure: bool,
    last_abs: Option<(i32, i32, i32)>,
}

impl DeviceDispatcher {
    const PRESSED: i32 = 1;
    const RELEASED: i32 = 0;

    /// Максимум оси давления на виртуальном устройстве.
    pub const PRESSURE_MAX: i32 = 8191;

    // Режим osu!: координаты планшета растягиваются вокруг центра.
    const OSU_VIRTUAL_MAX: i32 = 8191;
    const OSU_VIRTUAL_CENTER: i32 = 4095;

    pub fn new(config: Config) -> Self {
        let tablet_map: HashMap<u8, Vec<Key>> = [
            (0, vec![Key::KEY_TAB]),
            (1, vec![Key::KEY_SPACE]),
            (2, vec![Key::KEY_LEFTALT]),
            (3, vec![Key::KEY_LEFTCTRL]),
            (4, vec![Key::KEY_SCROLLDOWN]),
            (5, vec![Key::KEY_SCROLLUP]),
            (6, vec![Key::KEY_LEFTBRACE]),
            (7, vec![Key::KEY_LEFTCTRL, Key::KEY_KPMINUS]),
            (8, vec![Key::KEY_LEFTCTRL, Key::KEY_KPPLUS]),
            (9, vec![Key::KEY_E]),
            // 10, 11: физическим устройством не отправляются
            (12, vec![Key::KEY_B]),
            (13, vec![Key::KEY_RIGHTBRACE]),
        ]
        .into_iter()
        .collect();

        let pen_map: HashMap<u8, Vec<Key>> = [(4, vec![Key::BTN_STYLUS]), (6, vec![Key::BTN_STYLUS2])]
            .into_iter()
            .collect();

        let (pen_name, kbd_name) = match config.mode {
            Mode::Draw => ("virtual_tablet", "virtual_tablet"),
            Mode::Osu => ("virtual_tablet_pen", "virtual_tablet_keyboard"),
        };

        let virtual_pen = Self::virtual_pen_builder(
            pen_name,
            &config,
            &pen_map.values().flatten().cloned().collect::<Vec<Key>>(),
        )
        .expect("Error building virtual pen (нет доступа к /dev/uinput?)");
        let virtual_keyboard = Self::virtual_keyboard_builder(
            kbd_name,
            &tablet_map.values().flatten().cloned().collect::<Vec<Key>>(),
        )
        .expect("Error building virtual keyboard (нет доступа к /dev/uinput?)");

        DeviceDispatcher {
            filter: PenFilter::new(config.smooth),
            touch: TouchTracker::new(config.debounce),
            config,
            tablet_last_raw_pressed_buttons: 0xFFFF,
            pen_last_raw_pressed_button: 0,
            tablet_button_id_to_key_code_map: tablet_map,
            pen_button_id_to_key_code_map: pen_map,
            virtual_pen,
            virtual_keyboard,
            key_refs: KeyRefs::default(),
            was_touching: false,
            in_proximity: false,
            last_report: Instant::now(),
            invalid_pressure: false,
            last_abs: None,
        }
    }

    pub fn dispatch(&mut self, raw_data: &RawDataReader) {
        self.last_report = Instant::now();
        self.emit_pen_events(raw_data);
        self.emit_tablet_events(raw_data);
    }

    /// Вызывается, когда новых отчётов нет: определяет выход пера из зоны.
    pub fn idle(&mut self) {
        if !self.config.proximity || !self.in_proximity {
            return;
        }
        if self.last_report.elapsed() < Duration::from_millis(self.config.prox_timeout_ms) {
            return;
        }
        let mut events = Vec::with_capacity(4);
        if self.was_touching {
            events.push(InputEvent::new(EventType::KEY, Key::BTN_TOUCH.code(), Self::RELEASED));
            events.push(InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_PRESSURE.0, 0));
            self.was_touching = false;
        }
        events.push(InputEvent::new(EventType::KEY, Key::BTN_TOOL_PEN.code(), Self::RELEASED));
        self.in_proximity = false;
        self.last_abs = None;
        self.filter.reset();
        self.touch.reset();
        if let Err(e) = self.virtual_pen.emit(&events) {
            eprintln!("Error emitting pen proximity-out: {e}");
        }
    }

    // ---------------------------------------------------------------- клавиатура

    fn virtual_keyboard_builder(name: &str, keys: &[Key]) -> Result<VirtualDevice, Error> {
        let mut key_set = AttributeSet::<Key>::new();
        for key in keys {
            key_set.insert(*key);
        }
        VirtualDeviceBuilder::new()?
            .name(name)
            .input_id(InputId::new(BusType::BUS_USB, USB_VID, USB_PID, 1))
            .with_keys(&key_set)?
            .build()
    }

    fn emit_tablet_events(&mut self, raw_data: &RawDataReader) {
        let flags = raw_data.tablet_buttons_as_binary_flags();
        if flags == self.tablet_last_raw_pressed_buttons {
            return; // ничего не изменилось — не дёргаем ядро впустую
        }

        let mut events: Vec<InputEvent> = Vec::with_capacity(8);
        for i in (0..14u8).filter(|i| ![10, 11].contains(i)) {
            let mask = 1u16 << i;
            let is_pressed = flags & mask == 0;
            let was_pressed = self.tablet_last_raw_pressed_buttons & mask == 0;
            if is_pressed == was_pressed {
                continue;
            }
            let Some(keys) = self.tablet_button_id_to_key_code_map.get(&i) else {
                continue;
            };
            if is_pressed {
                for key in keys {
                    if self.key_refs.press(key.code()) {
                        events.push(InputEvent::new(EventType::KEY, key.code(), Self::PRESSED));
                    }
                }
            } else {
                // Обратный порядок: сначала снимаем клавишу, потом модификатор.
                for key in keys.iter().rev() {
                    if self.key_refs.release(key.code()) {
                        events.push(InputEvent::new(EventType::KEY, key.code(), Self::RELEASED));
                    }
                }
            }
        }
        self.tablet_last_raw_pressed_buttons = flags;

        if !events.is_empty() {
            // emit() сам добавляет SYN_REPORT — одна запись = один кадр.
            if let Err(e) = self.virtual_keyboard.emit(&events) {
                eprintln!("Error emitting keyboard events: {e}");
            }
        }
    }

    // ---------------------------------------------------------------- перо

    fn virtual_pen_builder(name: &str, config: &Config, pen_keys: &[Key]) -> Result<VirtualDevice, Error> {
        let (max_x, max_y, res_x, res_y) = match config.mode {
            Mode::Draw => {
                let (rx, ry) = Self::resolution(config, config.max_x, config.max_y, 1);
                (config.max_x, config.max_y, rx, ry)
            }
            Mode::Osu => {
                let (rx, ry) = Self::resolution(config, Self::OSU_VIRTUAL_MAX, Self::OSU_VIRTUAL_MAX, 10);
                (Self::OSU_VIRTUAL_MAX, Self::OSU_VIRTUAL_MAX, rx, ry)
            }
        };
        let abs_x = UinputAbsSetup::new(AbsoluteAxisType::ABS_X, AbsInfo::new(0, 0, max_x, 0, 0, res_x));
        let abs_y = UinputAbsSetup::new(AbsoluteAxisType::ABS_Y, AbsInfo::new(0, 0, max_y, 0, 0, res_y));
        let abs_pressure = UinputAbsSetup::new(
            AbsoluteAxisType::ABS_PRESSURE,
            AbsInfo::new(0, 0, Self::PRESSURE_MAX, 0, 0, 0),
        );

        let mut key_set = AttributeSet::<Key>::new();
        for key in pen_keys {
            key_set.insert(*key);
        }
        // BTN_TOUCH обязан быть в списке возможностей, иначе ядро молча
        // выбрасывает эти события (в режиме рисования так и было).
        for key in &[Key::BTN_TOOL_PEN, Key::BTN_LEFT, Key::BTN_RIGHT, Key::BTN_TOUCH] {
            key_set.insert(*key);
        }

        VirtualDeviceBuilder::new()?
            .name(name)
            .input_id(InputId::new(BusType::BUS_USB, USB_VID, USB_PID, 1))
            .with_absolute_axis(&abs_x)?
            .with_absolute_axis(&abs_y)?
            .with_absolute_axis(&abs_pressure)?
            .with_keys(&key_set)?
            .build()
    }

    /// Разрешение осей (единиц на мм). Без `--size` — прежние значения.
    fn resolution(config: &Config, max_x: i32, max_y: i32, fallback: i32) -> (i32, i32) {
        match config.size_mm {
            Some((w, h)) if w > 0.0 && h > 0.0 => (
                ((max_x as f32 / w).round() as i32).max(1),
                ((max_y as f32 / h).round() as i32).max(1),
            ),
            _ => (fallback, fallback),
        }
    }

    /// (касается ли, давление). Касание подтверждается несколькими отчётами подряд.
    fn process_pressure(&mut self, raw_pressure: i32) -> (bool, i32) {
        let (over, value) =
            self.config
                .pressure(self.touch.touching(), raw_pressure, Self::PRESSURE_MAX);
        let touching = self.touch.update(over);
        if self.config.verbose {
            let delta = self.config.base - raw_pressure;
            let invalid = delta > self.config.valid_max_delta;
            if invalid != self.invalid_pressure {
                eprintln!(
                    "[verbose] {} raw={raw_pressure} delta={delta}",
                    if invalid { "давление вне допустимого диапазона, отчёт игнорируется" } else { "диапазон снова допустим" }
                );
            }
            self.invalid_pressure = invalid;
        }
        (touching, if touching { value } else { 0 })
    }

    /// Сырые координаты (после фильтра) -> координаты виртуального устройства.
    fn map_xy(&self, x: f32, y: f32) -> (i32, i32) {
        let c = &self.config;
        match c.mode {
            Mode::Draw => (
                (x.round() as i32).clamp(0, c.max_x),
                (y.round() as i32).clamp(0, c.max_y),
            ),
            Mode::Osu => {
                let f = |v: f32, raw_max: i32| {
                    let center = raw_max as f32 / 2.0;
                    (((v - center) * c.osu_scale).round() as i32 + Self::OSU_VIRTUAL_CENTER)
                        .clamp(0, Self::OSU_VIRTUAL_MAX)
                };
                (f(x, c.max_x), f(y, c.max_y))
            }
        }
    }

    fn emit_pen_events(&mut self, raw_data: &RawDataReader) {
        let mut events: Vec<InputEvent> = Vec::with_capacity(10);

        // вход пера в зону — раньше любых осей и в том же кадре
        if self.config.proximity && !self.in_proximity {
            events.push(InputEvent::new(EventType::KEY, Key::BTN_TOOL_PEN.code(), Self::PRESSED));
            self.in_proximity = true;
            self.filter.reset();
            self.touch.reset();
            self.last_abs = None;
        }

        // кнопки пера
        let raw_pen_buttons = raw_data.pen_buttons();
        self.pen_button_events(raw_pen_buttons, &mut events);
        self.pen_last_raw_pressed_button = raw_pen_buttons;

        // оси
        let (fx, fy) = self
            .filter
            .apply(raw_data.x_axis() as f32, raw_data.y_axis() as f32);
        let (x, y) = self.map_xy(fx, fy);
        let (is_touching, pressure) = self.process_pressure(raw_data.pressure());

        if self.last_abs != Some((x, y, pressure)) {
            events.push(InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, x));
            events.push(InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_Y.0, y));
            events.push(InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_PRESSURE.0, pressure));
            self.last_abs = Some((x, y, pressure));
        }

        // касание
        if is_touching != self.was_touching {
            let state = if is_touching { Self::PRESSED } else { Self::RELEASED };
            events.push(InputEvent::new(EventType::KEY, Key::BTN_TOUCH.code(), state));
            self.was_touching = is_touching;
            if self.config.verbose {
                eprintln!(
                    "[verbose] касание {} raw={} delta={} x={x} y={y}",
                    if is_touching { "НАЧАЛОСЬ" } else { "закончилось" },
                    raw_data.pressure(),
                    self.config.base - raw_data.pressure()
                );
            }
        }

        // Всё в одном кадре: позиция, давление и касание приходят атомарно.
        if !events.is_empty() {
            if let Err(e) = self.virtual_pen.emit(&events) {
                eprintln!("Error emitting pen events: {e}");
            }
        }
    }

    fn pen_button_events(&self, pen_button: u8, events: &mut Vec<InputEvent>) {
        let transition = match (self.pen_last_raw_pressed_button, pen_button) {
            (2, x) if x == 6 || x == 4 => Some((Self::PRESSED, x)),
            (x, 2) if x == 6 || x == 4 => Some((Self::RELEASED, x)),
            _ => None,
        };
        if let Some((state, id)) = transition {
            if let Some(keys) = self.pen_button_id_to_key_code_map.get(&id) {
                for key in keys {
                    events.push(InputEvent::new(EventType::KEY, key.code(), state));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KeyRefs;
    use crate::config::Config;

    fn press(c: &Config, touching: bool, raw: i32) -> (bool, i32) {
        c.pressure(touching, raw, 8191)
    }

    #[test]
    fn pressure_curve() {
        let c = Config::default();
        assert_eq!(press(&c, false, 1740), (false, 0)); // покой
        assert_eq!(press(&c, false, 1740 - 150), (false, 0)); // ровно порог — ещё нет
        assert!(press(&c, false, 1740 - 151).0); // коснулись
        assert_eq!(press(&c, false, 1740 - 700), (true, 8191)); // 100% на --max
        assert_eq!(press(&c, false, 1740 - 1000), (true, 8191)); // выше — зажато
        assert_eq!(press(&c, false, 0), (false, 0)); // мусор вне зоны (raw=0) — не касание
        assert_eq!(press(&c, true, 1740 - 1101), (false, 0)); // за допустимым диапазоном
        // гистерезис: уже касаемся -> держим до release_threshold; из воздуха — нет
        assert!(press(&c, true, 1740 - 120).0);
        assert!(!press(&c, false, 1740 - 120).0);
        assert!(!press(&c, true, 1740 - 100).0);
        // монотонность
        let mut last = 0;
        for d in 151..700 {
            let v = press(&c, false, 1740 - d).1;
            assert!(v >= last);
            last = v;
        }
        // мягкая кривая даёт больше давления на средних нажимах
        let soft = Config { curve: 0.7, ..Config::default() };
        assert!(press(&soft, false, 1740 - 400).1 > press(&c, false, 1740 - 400).1);
    }

    #[test]
    fn shared_modifier_stays_pressed() {
        let mut k = KeyRefs::default();
        assert!(k.press(29)); // Ctrl (кнопка 3) — реально нажат
        assert!(!k.press(29)); // Ctrl (кнопка 7) — уже нажат
        assert!(!k.release(29)); // отпустили кнопку 7 — Ctrl ещё держится кнопкой 3
        assert!(k.release(29)); // отпустили кнопку 3 — теперь Ctrl отпущен
        assert!(!k.release(29)); // лишний release игнорируется
    }
}
