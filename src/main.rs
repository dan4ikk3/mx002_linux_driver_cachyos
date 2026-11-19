mod config;
mod filter;
mod physical_device;
mod virtual_device;

use rusb::Error as RusbError;
use signal_hook::consts::signal::*;
use signal_hook::flag::register;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use config::Config;
use physical_device::PhysicalDevice;
use virtual_device::{DeviceDispatcher, RawDataReader};

const VID: u16 = 0x08f2;
const PID: u16 = 0x6811;

fn main() {
    let config = match Config::from_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    for signal in [SIGINT, SIGTERM, SIGQUIT] {
        register(signal, Arc::clone(&stop)).expect("Error registering interrupt signals.");
    }

    let mut physical_device = PhysicalDevice::new(VID, PID);
    physical_device.init().set_full_mode();
    let mut data_reader = RawDataReader::new();

    if config.dump {
        dump(&physical_device, &mut data_reader, &stop, &config);
        return;
    }

    if config.calibrate {
        calibrate(&physical_device, &mut data_reader, &stop, &config);
        return;
    }

    let mut dispatcher = DeviceDispatcher::new(config.clone());
    println!("The driver is running ({:?} mode).", config.mode);

    while !stop.load(Ordering::Relaxed) {
        match physical_device.read_device_responses(&mut data_reader.data) {
            Ok(n) if n >= RawDataReader::MIN_REPORT_LEN => dispatcher.dispatch(&data_reader),
            Err(RusbError::Timeout) | Err(RusbError::Interrupted) => dispatcher.idle(),
            Ok(_) => {}
            Err(RusbError::NoDevice) | Err(RusbError::Io) => {
                eprintln!("The tablet was disconnected.");
                break;
            }
            Err(e) => eprintln!("USB read error: {e}"),
        }
    }
    println!();
    println!("The driver has exited.");
}

/// Статистика одной фазы калибровки.
#[derive(Default, Clone)]
struct PhaseStats {
    count: u32,
    raw_min: Option<i32>,
    raw_max: Option<i32>,
    x_max: i32,
    y_max: i32,
}

impl PhaseStats {
    fn add(&mut self, r: &RawDataReader) {
        let p = r.pressure();
        self.count += 1;
        self.raw_min = Some(self.raw_min.map_or(p, |m| m.min(p)));
        self.raw_max = Some(self.raw_max.map_or(p, |m| m.max(p)));
        self.x_max = self.x_max.max(r.x_axis());
        self.y_max = self.y_max.max(r.y_axis());
    }
}

/// Калибровка по фазам: вне зоны, над планшетом, рисование. По ним подбираются
/// --base, --threshold, --max, --valid-max, --max-x/--max-y.
fn calibrate(dev: &PhysicalDevice, reader: &mut RawDataReader, stop: &AtomicBool, cfg: &Config) {
    use std::io::Write;
    use std::time::{Duration, Instant};

    let phases = [
        ("1/3", "УБЕРИТЕ перо от планшета совсем (за пределы зоны) и не трогайте", 4u64),
        ("2/3", "держите перо НАД планшетом на высоте 1–3 см, медленно водите, НЕ касаясь", 6),
        ("3/3", "РИСУЙТЕ как обычно: штрихи по всей области, лёгкие и сильные нажимы, дойдите до краёв", 12),
    ];
    let mut stats: Vec<PhaseStats> = vec![PhaseStats::default(); phases.len()];

    'outer: for (idx, (num, text, secs)) in phases.iter().enumerate() {
        println!("\nФаза {num}: {text}.");
        for s in (1..=3).rev() {
            print!("  начало записи через {s}...\r");
            let _ = std::io::stdout().flush();
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(1) {
                if stop.load(Ordering::Relaxed) {
                    break 'outer;
                }
                let _ = dev.read_device_responses(&mut reader.data); // читаем и выбрасываем
            }
        }
        println!("  ЗАПИСЬ ({secs} с)...        ");
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(*secs) {
            if stop.load(Ordering::Relaxed) {
                break 'outer;
            }
            if let Ok(n) = dev.read_device_responses(&mut reader.data) {
                if n >= RawDataReader::MIN_REPORT_LEN {
                    stats[idx].add(reader);
                    print!("\r  raw={:5} (base-raw={:5})   ", reader.pressure(), cfg.base - reader.pressure());
                    let _ = std::io::stdout().flush();
                }
            }
        }
        println!();
    }

    let [oor, hover, draw] = [stats[0].clone(), stats[1].clone(), stats[2].clone()];
    println!("\n=== Результат ===");
    let show = |name: &str, s: &PhaseStats| match (s.raw_min, s.raw_max) {
        (Some(lo), Some(hi)) => println!("{name}: отчётов {}, raw {lo}..{hi}", s.count),
        _ => println!("{name}: отчётов нет"),
    };
    show("Вне зоны   ", &oor);
    show("Над планшетом", &hover);
    show("Рисование  ", &draw);

    let (Some(hover_max), Some(draw_min)) = (hover.raw_max.or(draw.raw_max), draw.raw_min) else {
        println!("Недостаточно данных (нужны фазы 2 и 3). Запустите калибровку ещё раз.");
        return;
    };
    let base = hover_max;
    let hover_delta = hover.raw_min.map_or(0, |m| base - m).max(0);
    let deepest = (base - draw_min).max(1);

    let threshold = (hover_delta + 30).max(30);
    let max = (deepest as f32 * 0.95).round() as i32;
    println!("\nНад планшетом давление отклонялось от покоя максимум на {hover_delta}, в рисовании — до {deepest}.");
    if threshold as f32 > deepest as f32 * 0.6 {
        println!("ВНИМАНИЕ: в воздухе перо даёт «нажим» почти как при рисовании — порог придётся задать высоким, лёгкие касания могут не засчитываться.");
    }

    let mut valid = cfg.valid_max_delta;
    match (oor.raw_min, oor.count) {
        (_, 0) => println!("Вне зоны планшет не присылает отчётов — это нормально."),
        (Some(lo), _) => {
            let oor_delta = base - lo;
            if oor_delta > deepest + 40 {
                valid = deepest + (oor_delta - deepest) / 2;
                println!("Вне зоны отчёты приходят с «нажимом» {oor_delta} — это выходит за диапазон рисования, их можно отбросить: --valid-max {valid}.");
            } else {
                println!("Вне зоны отчёты приходят с давлением {oor_delta}, похожим на рабочее — по давлению их отличить нельзя.");
            }
        }
        _ => {}
    }

    let max_x = hover.x_max.max(draw.x_max);
    let max_y = hover.y_max.max(draw.y_max);
    println!("\nРекомендуемые параметры:");
    println!(
        "  mx002 --base {base} --threshold {threshold} --max {max} --valid-max {} --max-x {max_x} --max-y {max_y}",
        valid.max(max + 1)
    );
    println!("(сейчас base={}, threshold={}, max={}, valid-max={}, max-x={}, max-y={})",
        cfg.base, cfg.threshold, cfg.max_delta, cfg.valid_max_delta, cfg.max_x, cfg.max_y);
}

/// Печатает отчёты, когда меняется что-то кроме X/Y/давления (байты 1..6), и декодированные
/// X, Y и «нажим» (base - raw). Как пользоваться: подержите перо над планшетом, медленно
/// поднимайте его до максимальной высоты, уберите совсем, понажимайте кнопки пера и планшета,
/// затем Ctrl+C. Какие байты меняются при подъёме — те и несут информацию о высоте/зоне.
fn dump(dev: &PhysicalDevice, reader: &mut RawDataReader, stop: &AtomicBool, cfg: &Config) {
    println!("Дамп отчётов: печатаются изменения байт 0 и 7+ (X/Y/давление декодируются). Ctrl+C — выход.");
    let mut last: Vec<u8> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        if let Ok(n) = dev.read_device_responses(&mut reader.data) {
            let n = n.min(reader.data.len());
            let mut key = reader.data[..n].to_vec();
            for b in key.iter_mut().take(7).skip(1) {
                *b = 0;
            }
            if key != last {
                let hex: Vec<String> = reader.data[..n].iter().map(|b| format!("{b:02x}")).collect();
                if n >= RawDataReader::MIN_REPORT_LEN {
                    println!(
                        "[{n:2}] {}  | x={:5} y={:5} press={:4}",
                        hex.join(" "),
                        reader.x_axis(),
                        reader.y_axis(),
                        cfg.base - reader.pressure()
                    );
                } else {
                    println!("[{n:2}] {}", hex.join(" "));
                }
                last = key;
            }
        }
    }
}
