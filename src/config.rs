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
    pub calibrate: bool,
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
            calibrate: false,
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
        let mut c = Config::default();
        let mut args = std::env::args().skip(1);
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
                "--calibrate" => c.calibrate = true,
                "--help" | "-h" => return Err(Self::usage()),
                other => return Err(format!("неизвестный аргумент: {other}\n{}", Self::usage())),
            }
        }
        if c.max_delta <= c.threshold {
            return Err("--max должен быть больше --threshold".into());
        }
        if c.curve <= 0.0 {
            return Err("--curve должен быть > 0".into());
        }
        Ok(c)
    }

    pub fn usage() -> String {
        "Использование: mx002 [--mode draw|osu] [--base N] [--threshold N] [--max N] \
         [--curve F] [--scale F] [--calibrate]\n\
         \n  --mode       draw (по умолчанию) или osu\
         \n  --base       сырое давление в покое (по умолчанию 1740)\
         \n  --threshold  порог касания (150)\
         \n  --max        нажим, дающий 100% давления (700)\
         \n  --curve      кривая давления, 1.0 линейно, <1.0 мягче (1.0)\
         \n  --scale      множитель координат для osu (8.0)\
         \n  --calibrate  показать диапазон давления вашего пера и выйти"
            .into()
    }
}

fn parse<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("не удалось разобрать значение: {s}"))
}
