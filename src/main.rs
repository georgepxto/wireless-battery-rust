#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Wireless Battery — bateria de mouse, teclado, headset e controles sem fio
//! na bandeja do Windows. Win32 puro para a interface; as leituras rodam numa
//! thread separada para o menu nunca travar.
//!
//!   wireless-battery.exe           icone na bandeja
//!   wireless-battery.exe --once    imprime os dispositivos encontrados e sai
//!   wireless-battery.exe --hid     lista todas as interfaces HID (diagnostico)

mod aula;
mod bluetooth;
mod device;
mod icon;
mod logitech;
mod mchose;
mod xbox;

use std::cell::RefCell;
use std::collections::HashSet;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use device::{Reading, Status, Tracker};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 1;
/// lParam = Box<Vec<Reading>> vindo da thread de leitura.
const WM_READINGS: u32 = WM_APP + 2;
const TIMER_DEBOUNCE: usize = 2;
const LOW_THRESHOLD: u8 = 20;

/// HID (Logitech, MCHOSE, AULA) e controles: a cada 20 s.
const POLL: Duration = Duration::from_secs(20);
/// Bluetooth acorda o radio do aparelho: a cada 3 ciclos (60 s).
const BT_EVERY: u32 = 3;

const ID_REFRESH: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_EXIT: usize = 3;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "WirelessBattery";

static WORKER: OnceLock<Sender<()>> = OnceLock::new();

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn copy_into(dst: &mut [u16], s: &str) {
    let max = dst.len() - 1;
    let mut n = 0;
    for c in s.encode_utf16().take(max) {
        dst[n] = c;
        n += 1;
    }
    dst[n] = 0;
}

/// COM multithread na thread atual (necessario para as APIs WinRT).
fn init_com() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
}

// ---------------------------------------------------------------- leitura

/// Uma rodada de leitura de todas as fontes.
struct Scanner {
    api: Option<hidapi::HidApi>,
    logi: logitech::Logitech,
    tracker: Tracker,
    bt: Vec<device::Observation>,
    tick: u32,
}

impl Scanner {
    fn new() -> Self {
        Self {
            api: hidapi::HidApi::new().ok(),
            logi: logitech::Logitech::new(),
            tracker: Tracker::default(),
            bt: Vec::new(),
            tick: 0,
        }
    }

    fn scan(&mut self, force_bt: bool) -> Vec<Reading> {
        let mut obs = Vec::new();
        if let Some(api) = self.api.as_mut() {
            let _ = api.refresh_devices();
            obs.extend(self.logi.scan(api));
            obs.extend(mchose::scan(api));
            obs.extend(aula::scan(api));
        }
        obs.extend(xbox::scan());
        if force_bt || self.tick.is_multiple_of(BT_EVERY) {
            self.bt = bluetooth::scan();
        }
        obs.extend(self.bt.iter().cloned());
        self.tick += 1;
        self.tracker.update(obs)
    }
}

fn worker(hwnd: isize, rx: mpsc::Receiver<()>) {
    init_com();
    let mut scanner = Scanner::new();
    // Windows.Gaming.Input so preenche a lista de controles depois do primeiro uso:
    // uma segunda leitura logo no inicio mostra o controle sem esperar 20 s.
    let mut wait = Duration::from_secs(3);
    let mut force_bt = false;
    loop {
        let readings = scanner.scan(force_bt);
        let ptr = Box::into_raw(Box::new(readings));
        let ok = unsafe { PostMessageW(hwnd as HWND, WM_READINGS, 0, ptr as LPARAM) };
        if ok == 0 {
            drop(unsafe { Box::from_raw(ptr) });
            return; // janela fechada
        }
        match rx.recv_timeout(wait) {
            Ok(()) => force_bt = true,
            Err(RecvTimeoutError::Timeout) => force_bt = false,
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while rx.try_recv().is_ok() {}
        wait = POLL;
    }
}

fn request_refresh() {
    if let Some(tx) = WORKER.get() {
        let _ = tx.send(());
    }
}

// ---------------------------------------------------------------- modo console

fn console_mode(arg: &str) {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    println!();
    match arg {
        "--hid" => match hidapi::HidApi::new() {
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
        },
        _ => {
            init_com();
            let mut scanner = Scanner::new();
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
    }
}

// ---------------------------------------------------------------- bandeja

struct State {
    readings: Vec<Reading>,
    icon: HICON,
    alerted: HashSet<String>,
    taskbar_created: u32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        readings: Vec::new(),
        icon: null_mut(),
        alerted: HashSet::new(),
        taskbar_created: 0,
    });
}

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        if arg == "--once" || arg == "--hid" {
            console_mode(&arg);
            return;
        }
    }

    unsafe {
        let name = wide("Local\\WirelessBatteryTray");
        let _mutex = CreateMutexW(null(), 0, name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }

        let hinst = GetModuleHandleW(null());
        let class = wide("WirelessBatteryWnd");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&wc);
        // Janela oculta. Nao e message-only para receber "TaskbarCreated"
        // quando o Explorer reinicia.
        let hwnd = CreateWindowExW(
            0, class.as_ptr(), class.as_ptr(), WS_OVERLAPPED,
            0, 0, 0, 0, null_mut(), null_mut(), hinst, null(),
        );
        if hwnd.is_null() {
            return;
        }

        let tc = wide("TaskbarCreated");
        let msg_id = RegisterWindowMessageW(tc.as_ptr());
        STATE.with(|s| s.borrow_mut().taskbar_created = msg_id);

        add_tray(hwnd);

        let (tx, rx) = mpsc::channel();
        let _ = WORKER.set(tx);
        let addr = hwnd as isize;
        std::thread::spawn(move || worker(addr, rx));

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn base_nid(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { zeroed() };
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

fn add_tray(hwnd: HWND) {
    let mut nid = base_nid(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    let icon = icon::render("-", theme_color());
    nid.hIcon = icon;
    copy_into(&mut nid.szTip, "Wireless Battery: procurando dispositivos...");
    unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
    swap_icon(icon);
}

fn swap_icon(new: HICON) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.icon.is_null() {
            unsafe { DestroyIcon(s.icon) };
        }
        s.icon = new;
    });
}

/// Branco na barra escura, quase preto na barra clara.
fn theme_color() -> u32 {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let val = wide("SystemUsesLightTheme");
    let mut data: u32 = 0;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, null_mut(),
            &mut data as *mut u32 as *mut _, &mut size)
    };
    if r == 0 && data == 1 { 0x1A1A1A } else { 0xFFFFFF }
}

fn label(r: &Reading) -> String {
    format!("{}: {}", r.name, r.status)
}

fn connected(r: &Reading) -> bool {
    !matches!(r.status, Status::NotConnected)
}

/// Menor nivel entre os dispositivos com porcentagem.
fn lowest(readings: &[Reading]) -> Option<u8> {
    readings
        .iter()
        .filter_map(|r| match r.status {
            Status::Battery { percent, .. } => Some(percent),
            _ => None,
        })
        .min()
}

fn apply(hwnd: HWND, readings: Vec<Reading>) {
    let (text, rgb) = match lowest(&readings) {
        None => ("-".to_string(), theme_color()),
        Some(p) if p <= LOW_THRESHOLD => (p.to_string(), 0xFF453A),
        Some(p) => (p.to_string(), theme_color()),
    };

    let active: Vec<String> = readings.iter().filter(|r| connected(r)).map(label).collect();
    let tip = if active.is_empty() { "Nenhum dispositivo sem fio conectado".to_string() } else { active.join("\n") };

    let mut nid = base_nid(hwnd);
    nid.uFlags = NIF_ICON | NIF_TIP;
    let new_icon = icon::render(&text, rgb);
    nid.hIcon = new_icon;
    copy_into(&mut nid.szTip, &tip);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    swap_icon(new_icon);

    // Aviso de bateria baixa, uma vez por descarga.
    let mut to_alert = Vec::new();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        for r in &readings {
            match r.status {
                Status::Battery { percent, charging: false, stale: false } if percent <= LOW_THRESHOLD => {
                    if s.alerted.insert(r.id.clone()) {
                        to_alert.push(label(r));
                    }
                }
                Status::Battery { percent, .. } if percent > LOW_THRESHOLD + 5 => {
                    s.alerted.remove(&r.id);
                }
                Status::ChargingNoLevel => {
                    s.alerted.remove(&r.id);
                }
                _ => {}
            }
        }
        s.readings = readings;
    });
    if !to_alert.is_empty() {
        let mut nid = base_nid(hwnd);
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_WARNING;
        copy_into(&mut nid.szInfoTitle, "Bateria baixa");
        copy_into(&mut nid.szInfo, &to_alert.join("\n"));
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    }
}

fn exe_path() -> String {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(null_mut(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn autostart_enabled() -> bool {
    let key = wide(RUN_KEY);
    let val = wide(RUN_VALUE);
    let mut size = 0u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size) == 0
    }
}

fn set_autostart(on: bool) {
    let key = wide(RUN_KEY);
    let val = wide(RUN_VALUE);
    unsafe {
        if on {
            let data = wide(&format!("\"{}\"", exe_path()));
            RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), REG_SZ,
                data.as_ptr() as *const _, (data.len() * 2) as u32);
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr());
        }
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        let readings = STATE.with(|s| s.borrow().readings.clone());
        if readings.is_empty() {
            let t = wide("Nenhum dispositivo encontrado");
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, t.as_ptr());
        }
        for r in &readings {
            let t = wide(&label(r).replace('&', "&&"));
            let flags = if connected(r) { MF_STRING } else { MF_STRING | MF_GRAYED };
            AppendMenuW(menu, flags, 0, t.as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let t = wide("Atualizar agora");
        AppendMenuW(menu, MF_STRING, ID_REFRESH, t.as_ptr());
        let t = wide("Iniciar com o Windows");
        let flags = if autostart_enabled() { MF_STRING | MF_CHECKED } else { MF_STRING };
        AppendMenuW(menu, flags, ID_AUTOSTART, t.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let t = wide("Sair");
        AppendMenuW(menu, MF_STRING, ID_EXIT, t.as_ptr());

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match cmd as usize {
            ID_REFRESH => request_refresh(),
            ID_AUTOSTART => set_autostart(!autostart_enabled()),
            ID_EXIT => {
                DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = STATE.with(|s| s.borrow().taskbar_created);
    if msg != 0 && msg == taskbar_created {
        add_tray(hwnd);
        let readings = STATE.with(|s| s.borrow().readings.clone());
        apply(hwnd, readings);
        return 0;
    }
    match msg {
        WM_READINGS => {
            let readings = *Box::from_raw(lparam as *mut Vec<Reading>);
            apply(hwnd, readings);
            0
        }
        WM_TRAY => {
            let event = lparam as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_menu(hwnd);
            }
            0
        }
        WM_TIMER if wparam == TIMER_DEBOUNCE => {
            KillTimer(hwnd, TIMER_DEBOUNCE);
            request_refresh();
            0
        }
        // Dispositivo plugado/removido ou volta da suspensao: le de novo em 2 s
        // (agrupa rajadas de eventos).
        WM_DEVICECHANGE | WM_POWERBROADCAST => {
            SetTimer(hwnd, TIMER_DEBOUNCE, 2_000, None);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        // Troca de tema claro/escuro: redesenha o icone.
        WM_SETTINGCHANGE => {
            let readings = STATE.with(|s| s.borrow().readings.clone());
            apply(hwnd, readings);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            let nid = base_nid(hwnd);
            Shell_NotifyIconW(NIM_DELETE, &nid);
            swap_icon(null_mut());
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
