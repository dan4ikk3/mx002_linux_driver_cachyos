mod config;
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

    if config.calibrate {
        calibrate(&physical_device, &mut data_reader, &stop, &config);
        return;
    }

    let mut dispatcher = DeviceDispatcher::new(config.clone());
    println!("The driver is running ({:?} mode).", config.mode);

    while !stop.load(Ordering::Relaxed) {
        match physical_device.read_device_responses(&mut data_reader.data) {
            Ok(n) if n >= RawDataReader::MIN_REPORT_LEN => dispatcher.dispatch(&data_reader),
            Ok(_) | Err(RusbError::Timeout) | Err(RusbError::Interrupted) => {}
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
    println!("Калибровка: водите пером и понажимайте так, как привыкли рисовать. Ctrl+C — закончить.");
    let (mut max_raw, mut min_raw) = (i32::MIN, i32::MAX);
    while !stop.load(Ordering::Relaxed) {
        if let Ok(n) = dev.read_device_responses(&mut reader.data) {
            if n >= RawDataReader::MIN_REPORT_LEN {
                let p = reader.pressure();
                max_raw = max_raw.max(p);
                min_raw = min_raw.min(p);
                print!("\rraw={p:5}  idle(max)={max_raw:5}  deepest(min)={min_raw:5}   ");
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
}
