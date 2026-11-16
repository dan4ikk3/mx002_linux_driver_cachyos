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

/// Показывает диапазон сырого давления: нажмите пером на планшет (нормально, без фанатизма)
/// и завершите Ctrl+C — получите рекомендуемые значения --base и --max.
fn calibrate(dev: &PhysicalDevice, reader: &mut RawDataReader, stop: &AtomicBool, cfg: &Config) {
    println!("Калибровка: проведите пером по краям планшета и понажимайте так, как привыкли рисовать. Ctrl+C — закончить.");
    let (mut max_raw, mut min_raw) = (i32::MIN, i32::MAX);
    let (mut max_x, mut max_y) = (0, 0);
    while !stop.load(Ordering::Relaxed) {
        if let Ok(n) = dev.read_device_responses(&mut reader.data) {
            if n >= RawDataReader::MIN_REPORT_LEN {
                let p = reader.pressure();
                max_x = max_x.max(reader.x_axis());
                max_y = max_y.max(reader.y_axis());
                max_raw = max_raw.max(p);
                min_raw = min_raw.min(p);
                print!("\rraw={p:5} idle={max_raw:5} deepest={min_raw:5}  x_max={max_x:5} y_max={max_y:5}  ");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
        }
    }
    println!();
    if max_raw == i32::MIN {
        println!("Данных не получено.");
        return;
    }
    let span = max_raw - min_raw;
    println!("Диапазон сырого давления: {min_raw}..{max_raw} (разброс {span}).");
    println!(
        "Рекомендация: mx002 --base {max_raw} --threshold {} --max {}",
        (span as f32 * 0.1).round() as i32,
        (span as f32 * 0.95).round() as i32
    );
    println!("(сейчас base={}, threshold={}, max={})", cfg.base, cfg.threshold, cfg.max_delta);
    println!("Координаты: x_max={max_x}, y_max={max_y} (проведите пером по всем краям планшета).");
    println!("Если значения заметно отличаются от 4096, добавьте --max-x {max_x} --max-y {max_y}");
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
