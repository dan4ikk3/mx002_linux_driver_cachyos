use std::collections::HashMap;
use std::io::Error;

use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AbsInfo, AbsoluteAxisType, AttributeSet, EventType, InputEvent, Key, UinputAbsSetup,
};

use crate::config::{Config, Mode};

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

pub struct DeviceDispatcher {
    config: Config,
    tablet_last_raw_pressed_buttons: u16,
    pen_last_raw_pressed_button: u8,
    tablet_button_id_to_key_code_map: HashMap<u8, Vec<Key>>,
    pen_button_id_to_key_code_map: HashMap<u8, Vec<Key>>,
    virtual_pen: VirtualDevice,
    virtual_keyboard: VirtualDevice,
    was_touching: bool,
    last_abs: Option<(i32, i32, i32)>,
}

impl DeviceDispatcher {
    const PRESSED: i32 = 1;
    const RELEASED: i32 = 0;

    /// Максимум оси давления на виртуальном устройстве.
    pub const PRESSURE_MAX: i32 = 8191;

    // Режим osu!: координаты планшета 0..4096 растягиваются вокруг центра.
    const OSU_RAW_CENTER: i32 = 2048;
    const OSU_VIRTUAL_MAX: i32 = 8191;
    const OSU_VIRTUAL_CENTER: i32 = 4095;
    // Режим рисования: полная область планшета.
    const DRAW_MAX: i32 = 4096;

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
            config.mode,
            &pen_map.values().flatten().cloned().collect::<Vec<Key>>(),
        )
        .expect("Error building virtual pen");
        let virtual_keyboard = Self::virtual_keyboard_builder(
            kbd_name,
            &tablet_map.values().flatten().cloned().collect::<Vec<Key>>(),
        )
        .expect("Error building virtual keyboard");

        DeviceDispatcher {
            config,
            tablet_last_raw_pressed_buttons: 0xFFFF,
            pen_last_raw_pressed_button: 0,
            tablet_button_id_to_key_code_map: tablet_map,
            pen_button_id_to_key_code_map: pen_map,
            virtual_pen,
            virtual_keyboard,
            was_touching: false,
            last_abs: None,
        }
    }

    pub fn dispatch(&mut self, raw_data: &RawDataReader) {
        self.emit_pen_events(raw_data);
        self.emit_tablet_events(raw_data);
    }

    // ---------------------------------------------------------------- клавиатура

    fn virtual_keyboard_builder(name: &str, keys: &[Key]) -> Result<VirtualDevice, Error> {
        let mut key_set = AttributeSet::<Key>::new();
        for key in keys {
            key_set.insert(*key);
        }
        VirtualDeviceBuilder::new()?
            .name(name)
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
            let state = if is_pressed { Self::PRESSED } else { Self::RELEASED };
            if let Some(keys) = self.tablet_button_id_to_key_code_map.get(&i) {
                // Отпускаем в обратном порядке, чтобы комбинации (Ctrl+...) были корректны.
                if is_pressed {
                    for key in keys {
                        events.push(InputEvent::new(EventType::KEY, key.code(), state));
                    }
                } else {
                    for key in keys.iter().rev() {
                        events.push(InputEvent::new(EventType::KEY, key.code(), state));
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

    fn virtual_pen_builder(name: &str, mode: Mode, pen_keys: &[Key]) -> Result<VirtualDevice, Error> {
        let (max_xy, res) = match mode {
            Mode::Draw => (Self::DRAW_MAX, 1),
            Mode::Osu => (Self::OSU_VIRTUAL_MAX, 10),
        };
        let abs_x = UinputAbsSetup::new(AbsoluteAxisType::ABS_X, AbsInfo::new(0, 0, max_xy, 0, 0, res));
        let abs_y = UinputAbsSetup::new(AbsoluteAxisType::ABS_Y, AbsInfo::new(0, 0, max_xy, 0, 0, res));
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
            .with_absolute_axis(&abs_x)?
            .with_absolute_axis(&abs_y)?
            .with_absolute_axis(&abs_pressure)?
            .with_keys(&key_set)?
            .build()
    }

    fn process_pressure(&self, raw_pressure: i32) -> (bool, i32) {
        self.config.pressure(self.was_touching, raw_pressure, Self::PRESSURE_MAX)
    }

    fn map_xy(&self, raw_x: i32, raw_y: i32) -> (i32, i32) {
        match self.config.mode {
            Mode::Draw => (raw_x, raw_y),
            Mode::Osu => {
                let s = self.config.osu_scale;
                let f = |v: i32| {
                    (((v - Self::OSU_RAW_CENTER) as f32 * s) as i32 + Self::OSU_VIRTUAL_CENTER)
                        .clamp(0, Self::OSU_VIRTUAL_MAX)
                };
                (f(raw_x), f(raw_y))
            }
        }
    }

    fn emit_pen_events(&mut self, raw_data: &RawDataReader) {
        let mut events: Vec<InputEvent> = Vec::with_capacity(8);

        // кнопки пера
        let raw_pen_buttons = raw_data.pen_buttons();
        self.pen_button_events(raw_pen_buttons, &mut events);
        self.pen_last_raw_pressed_button = raw_pen_buttons;

        // оси
        let (x, y) = self.map_xy(raw_data.x_axis(), raw_data.y_axis());
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
}
