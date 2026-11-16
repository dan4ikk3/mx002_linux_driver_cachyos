//! Параметры драйвера (аргументы командной строки).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Draw,
    Osu,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    /// Сырое значение давления «в покое» (перо не касается). Подбирается `--calibrate`.
    pub base: i32,
    /// Порог касания: (base - raw) должно превысить это значение.
    pub threshold: i32,
    /// (base - raw), при котором выдаётся максимальное давление.
    pub max_delta: i32,
    /// Кривая давления: 1.0 — линейно, <1.0 — мягче (легче дойти до сильного нажима).
    pub curve: f32,
    /// Множитель координат в режиме osu!.
    pub osu_scale: f32,
    /// Сглаживание позиции пера: 0.0 — выкл., 0.9 — максимум. Быстрые движения не страдают.
    pub smooth: f32,
    /// Размер рабочей области сырых координат планшета.
    pub max_x: i32,
    pub max_y: i32,
    /// Физический размер рабочей области в мм (для разрешения осей и сохранения пропорций).
    pub size_mm: Option<(f32, f32)>,
    /// Отправлять вход/выход пера из зоны (BTN_TOOL_PEN), как обычные планшеты.
    /// Сколько отчётов подряд давление должно быть выше порога, чтобы началось касание
    /// (защита от фантомных нажатий). 1 — без защиты.
    pub debounce: u32,
    pub proximity: bool,
    /// Через сколько мс без отчётов считаем, что перо вышло из зоны.
    pub prox_timeout_ms: u64,
    pub calibrate: bool,
    /// Диагностика: печатать сырые отчёты планшета (для поиска байта «перо вне зоны»).
    pub dump: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Draw,
            base: 1740,
            threshold: 150,
            // README: железо отдаёт ~1024 уровня, безопасно используется ~700.
            max_delta: 700,
            curve: 1.0,
            osu_scale: 8.0,
            smooth: 0.0,
            max_x: 4096,
            max_y: 4096,
            size_mm: None,
            debounce: 2,
            proximity: true,
            prox_timeout_ms: 100,
            calibrate: false,
            dump: false,
        }
    }
}

impl Config {
    /// Порог отпускания (гистерезис).
    pub fn release_threshold(&self) -> i32 {
        self.threshold * 7 / 10
    }

    /// Давление -> (касается ли, значение 0..=pressure_max).
    /// Гистерезис: касание начинается выше `threshold`, а заканчивается ниже
    /// `release_threshold`, поэтому линия не «рвётся» на самом лёгком нажиме.
    pub fn pressure(&self, was_touching: bool, raw: i32, pressure_max: i32) -> (bool, i32) {
        let delta = self.base - raw;
        let limit = if was_touching { self.release_threshold() } else { self.threshold };
        if delta <= limit {
            return (false, 0);
        }
        let span = (self.max_delta - self.threshold).max(1) as f32;
        let t = ((delta - self.threshold) as f32 / span).clamp(0.0, 1.0);
        let value = (t.powf(self.curve) * pressure_max as f32).round() as i32;
        (true, value.clamp(1, pressure_max))
    }

    pub fn from_args() -> Result<Config, String> {
        Self::parse(std::env::args().skip(1))
    }

    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
        let mut c = Config::default();
        let mut smooth_set = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| -> Result<String, String> {
                args.next().ok_or_else(|| format!("{name} требует значение"))
            };
            match arg.as_str() {
                "--mode" | "-m" => {
                    c.mode = match value("--mode")?.as_str() {
                        "draw" => Mode::Draw,
                        "osu" => Mode::Osu,
                        other => return Err(format!("неизвестный режим: {other} (draw | osu)")),
                    }
                }
                "--base" => c.base = parse(&value("--base")?)?,
                "--threshold" => c.threshold = parse(&value("--threshold")?)?,
                "--max" => c.max_delta = parse(&value("--max")?)?,
                "--curve" => c.curve = parse(&value("--curve")?)?,
                "--scale" => c.osu_scale = parse(&value("--scale")?)?,
                "--smooth" => {
                    c.smooth = parse(&value("--smooth")?)?;
                    smooth_set = true;
                }
                "--max-x" => c.max_x = parse(&value("--max-x")?)?,
                "--max-y" => c.max_y = parse(&value("--max-y")?)?,
                "--size" => {
                    let v = value("--size")?;
                    let (w, h) = v
                        .split_once('x')
                        .ok_or("--size ожидает формат ШИРИНАxВЫСОТА в мм, например 254x158")?;
                    c.size_mm = Some((parse(w)?, parse(h)?));
                }
                "--debounce" => c.debounce = parse(&value("--debounce")?)?,
                "--no-proximity" => c.proximity = false,
                "--prox-timeout" => c.prox_timeout_ms = parse(&value("--prox-timeout")?)?,
                "--calibrate" => c.calibrate = true,
                "--dump" => c.dump = true,
                "--help" | "-h" => return Err(Self::usage()),
                other => return Err(format!("неизвестный аргумент: {other}\n{}", Self::usage())),
            }
        }
        if c.max_delta <= c.threshold {
            return Err("--max должен быть больше --threshold".into());
        }
        // В osu! дрожание сенсора усиливается множителем координат, поэтому сглаживание
        // там включено по умолчанию (быстрые движения проходят без задержки).
        if !smooth_set && c.mode == Mode::Osu {
            c.smooth = 0.5;
        }
        if !(1..=10).contains(&c.debounce) {
            return Err("--debounce должен быть в диапазоне 1..10".into());
        }
        if c.max_x <= 0 || c.max_y <= 0 {
            return Err("--max-x и --max-y должны быть > 0".into());
        }
        if !(0.0..=0.9).contains(&c.smooth) {
            return Err("--smooth должен быть в диапазоне 0.0..0.9".into());
        }
        if c.curve <= 0.0 {
            return Err("--curve должен быть > 0".into());
        }
        Ok(c)
    }

    pub fn usage() -> String {
        "Использование: mx002 [--mode draw|osu] [--base N] [--threshold N] [--max N] \
         [--curve F] [--scale F] [--smooth F]\n\
         \x20        [--max-x N] [--max-y N] [--size WxH] [--debounce N] [--no-proximity] [--prox-timeout MS] [--calibrate] [--dump]\n\
         \n  --mode       draw (по умолчанию) или osu\
         \n  --base       сырое давление в покое (по умолчанию 1740)\
         \n  --threshold  порог касания (150)\
         \n  --max        нажим, дающий 100% давления (700)\
         \n  --curve      кривая давления, 1.0 линейно, <1.0 мягче (1.0)\
         \n  --scale      множитель координат для osu (8.0)\
         \n  --smooth     сглаживание позиции 0.0..0.9 (draw: 0, osu: 0.5)\
         \n  --max-x/-y   размер области сырых координат (4096) — подбирается --calibrate\
         \n  --size WxH   физический размер рабочей области, мм (для пропорций и разрешения осей)\
         \n  --debounce   отчётов подряд выше порога до касания, защита от фантомных нажатий (2)\
         \n  --no-proximity  не отправлять вход/выход пера из зоны\
         \n  --prox-timeout  мс без отчётов до «пера нет в зоне» (100)\
         \n  --calibrate  показать диапазон давления и координат вашего пера и выйти\
         \n  --dump       печатать сырые отчёты планшета (диагностика) и выйти"
            .into()
    }
}

fn parse<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("не удалось разобрать значение: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Config, String> {
        Config::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_for_draw() {
        let c = parse_args(&[]).unwrap();
        assert_eq!(c.mode, Mode::Draw);
        assert_eq!(c.smooth, 0.0);
        assert!(c.proximity);
    }

    #[test]
    fn osu_enables_smoothing_unless_overridden() {
        assert_eq!(parse_args(&["--mode", "osu"]).unwrap().smooth, 0.5);
        assert_eq!(parse_args(&["--mode", "osu", "--smooth", "0"]).unwrap().smooth, 0.0);
        assert_eq!(parse_args(&["--smooth", "0.3", "--mode", "osu"]).unwrap().smooth, 0.3);
    }

    #[test]
    fn size_and_axes() {
        let c = parse_args(&["--size", "254x158", "--max-x", "4000", "--max-y", "2500"]).unwrap();
        assert_eq!(c.size_mm, Some((254.0, 158.0)));
        assert_eq!((c.max_x, c.max_y), (4000, 2500));
        assert!(parse_args(&["--size", "254"]).is_err());
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse_args(&["--mode", "foo"]).is_err());
        assert!(parse_args(&["--max", "100"]).is_err()); // не больше threshold
        assert!(parse_args(&["--smooth", "1.5"]).is_err());
        assert!(parse_args(&["--curve", "0"]).is_err());
        assert!(parse_args(&["--max-x", "0"]).is_err());
        assert!(parse_args(&["--bogus"]).is_err());
    }

    #[test]
    fn debounce_option() {
        assert_eq!(parse_args(&[]).unwrap().debounce, 2);
        assert_eq!(parse_args(&["--debounce", "1"]).unwrap().debounce, 1);
        assert!(parse_args(&["--debounce", "0"]).is_err());
        assert!(parse_args(&["--debounce", "11"]).is_err());
    }

    #[test]
    fn proximity_flags() {
        let c = parse_args(&["--no-proximity", "--prox-timeout", "250"]).unwrap();
        assert!(!c.proximity);
        assert_eq!(c.prox_timeout_ms, 250);
    }
}
