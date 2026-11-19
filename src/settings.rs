//! Preferencias no registro (HKCU\Software\wireless-battery): o que aparece na barra,
//! dispositivos ocultos, avisos de bateria baixa, iniciar com o Windows e tema.
//! Baseado em renanleonel/windows-taskbar-battery (MIT).

use crate::wide;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "WirelessBattery";
const APP_KEY: &str = r"Software\wireless-battery";

/// O que a barra mostra.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Display {
    /// Todos os dispositivos conectados, menos os desmarcados no menu.
    All,
    /// So o dispositivo com este id (aparece com "—" quando desconectado).
    Only(String),
}

pub fn display() -> Display {
    match read_string(APP_KEY, "Display") {
        Some(id) if !id.is_empty() && id != "all" => Display::Only(id),
        _ => Display::All,
    }
}

pub fn set_display(d: &Display) {
    let value = match d {
        Display::All => "all",
        Display::Only(id) => id.as_str(),
    };
    write_string(APP_KEY, "Display", value);
}

pub fn light_theme() -> bool {
    read_dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "SystemUsesLightTheme") == Some(1)
}

pub fn autostart_enabled() -> bool {
    let (k, v) = (wide(RUN_KEY), wide(RUN_VALUE));
    let mut size = 0u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size)
            == ERROR_SUCCESS
    }
}

pub fn set_autostart(on: bool) {
    let (k, v) = (wide(RUN_KEY), wide(RUN_VALUE));
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            write_string(RUN_KEY, RUN_VALUE, &format!("\"{}\"", exe.display()));
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr());
        }
    }
}

/// Se o autostart esta ligado, atualiza o caminho caso o .exe tenha mudado de pasta.
pub fn refresh_autostart_path() {
    if autostart_enabled() {
        set_autostart(true);
    }
}

pub fn alerts_enabled() -> bool {
    read_dword(APP_KEY, "LowBatteryAlerts") != Some(0)
}

pub fn set_alerts(on: bool) {
    write_dword(APP_KEY, "LowBatteryAlerts", u32::from(on));
}

/// Icones na cor do fabricante, com brilho e barra de nivel (ligado por padrao).
pub fn brand_colors_enabled() -> bool {
    read_dword(APP_KEY, "BrandColors") != Some(0)
}

pub fn set_brand_colors(on: bool) {
    write_dword(APP_KEY, "BrandColors", u32::from(on));
}

/// Registra o AppUserModelID para o Windows aceitar as notificacoes (toasts).
pub fn register_app_id() {
    let key = format!(r"Software\Classes\AppUserModelId\{}", crate::notify::APP_ID);
    write_string(&key, "DisplayName", "Wireless Battery");
}

/// Ids dos dispositivos desmarcados no menu.
pub fn hidden_devices() -> Vec<String> {
    read_string(APP_KEY, "Hidden").map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
}

pub fn toggle_hidden(id: &str) {
    let mut hidden = hidden_devices();
    match hidden.iter().position(|h| h == id) {
        Some(i) => {
            hidden.remove(i);
        }
        None => hidden.push(id.to_string()),
    }
    write_string(APP_KEY, "Hidden", &hidden.join("\n"));
}

fn write_string(key: &str, name: &str, value: &str) {
    let (k, n, data) = (wide(key), wide(name), wide(value));
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_SZ, data.as_ptr() as *const c_void, (data.len() * 2) as u32);
    }
}

fn read_string(key: &str, name: &str) -> Option<String> {
    let (k, n) = (wide(key), wide(name));
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size) != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as *mut c_void, &mut size) != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn read_dword(key: &str, name: &str) -> Option<u32> {
    let (k, n) = (wide(key), wide(name));
    let (mut value, mut size) = (0u32, 4u32);
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_DWORD, null_mut(),
            &mut value as *mut u32 as *mut c_void, &mut size)
    };
    (r == ERROR_SUCCESS).then_some(value)
}

fn write_dword(key: &str, name: &str, value: u32) {
    let (k, n) = (wide(key), wide(name));
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_DWORD, &value as *const u32 as *const c_void, 4);
    }
}
