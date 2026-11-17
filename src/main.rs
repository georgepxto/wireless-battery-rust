#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Wireless Battery — bateria de mouse, teclado, headset, fones e controles sem fio
//! direto na barra de tarefas do Windows.
//!
//!   wireless-battery.exe           widget na barra de tarefas
//!   wireless-battery.exe --demo    widget com leituras de exemplo (todos os estados)
//!   wireless-battery.exe --once    imprime os dispositivos encontrados e sai
//!                                  (com --verbose, tambem as respostas brutas)
//!   wireless-battery.exe --hid     lista todas as interfaces HID (diagnostico)

mod aula;
mod bluetooth;
mod device;
mod headsetcontrol;
mod json;
mod logitech;
mod mchose;
mod notify;
mod playstation;
#[cfg(feature = "probe")]
mod probe;
mod render;
mod scan;
mod settings;
#[cfg(feature = "snapshot")]
mod snapshot;
mod widget;
mod xbox;

use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};

/// `--once --verbose`: os leitores imprimem as respostas brutas dos dispositivos.
static VERBOSE: AtomicBool = AtomicBool::new(false);

pub(crate) fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// COM multithread na thread atual (necessario para as APIs WinRT).
pub(crate) fn init_com() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--once") => {
            attach_console();
            VERBOSE.store(std::env::args().any(|a| a == "--verbose"), Ordering::Relaxed);
            init_com();
            let mut scanner = scan::Scanner::new();
            let _ = scanner.scan(true);
            std::thread::sleep(Duration::from_millis(800));
            let readings = scanner.scan(false);
            if readings.is_empty() {
                println!("Nenhum dispositivo com bateria encontrado.");
            }
            for r in readings {
                println!("{:<10} {:<36} {:<20} [{}]", format!("{:?}", r.kind), r.name, r.status.to_string(), r.id);
            }
        }
        Some("--hid") => {
            attach_console();
            match hidapi::HidApi::new() {
                Ok(api) => {
                    for d in api.device_list() {
                        println!(
                            "{:04X}:{:04X}  if={:<2} usage_page={:04X} usage={:04X}  {} {}",
                            d.vendor_id(), d.product_id(), d.interface_number(), d.usage_page(), d.usage(),
                            d.manufacturer_string().unwrap_or(""), d.product_string().unwrap_or(""),
                        );
                    }
                }
                Err(e) => println!("hidapi: {e}"),
            }
        }
        #[cfg(feature = "probe")]
        Some("--probe-mchose") => {
            attach_console();
            probe::run();
        }
        #[cfg(feature = "snapshot")]
        Some("--snapshot") => {
            attach_console();
            let dir = std::env::args().nth(2).unwrap_or_else(|| "docs".into());
            snapshot::run(std::path::Path::new(&dir));
        }
        Some("--demo") => {
            widget::DEMO.store(true, std::sync::atomic::Ordering::Relaxed);
            widget::run();
        }
        _ => widget::run(),
    }
}

/// O release nao tem console proprio: escreve no PowerShell/cmd de onde foi chamado.
/// Se a saida ja foi redirecionada (`> arquivo`), mantem o redirecionamento.
fn attach_console() {
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out as isize == -1 {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    println!();
}
