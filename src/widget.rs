//! Janelas. Adaptado de renanleonel/windows-taskbar-battery (MIT).
//!
//! - Uma janela oculta "controller" recebe os avisos do sistema (TaskbarCreated quando o
//!   Explorer reinicia, troca de tema, dispositivo plugado), guarda o timer e abre o menu.
//! - O widget visivel e uma janela filha transparente da barra de tarefas (Shell_TrayWnd),
//!   logo a esquerda da area de notificacao. Morre junto com o Explorer e e recriado.
//! - Uma thread de leitura varre os dispositivos e avisa o controller com WM_APP_UPDATE.

use crate::device::{Kind, Reading, Status};
use crate::scan::Scanner;
use crate::settings::{self, Display};
use crate::{init_com, notify, render, wide};
use std::cell::RefCell;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_APP_UPDATE: u32 = WM_APP + 1;
const WM_APP_MENU: u32 = WM_APP + 2;
const TIMER_PLACE: usize = 1;
/// WM_POWERBROADCAST: voltou da suspensao/hibernacao.
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

const CMD_REFRESH: usize = 1;
const CMD_AUTOSTART: usize = 2;
const CMD_EXIT: usize = 3;
const CMD_ALERTS: usize = 4;
const CMD_SHOW_ALL: usize = 5;
/// Marcar/desmarcar dispositivo no modo "todos".
const CMD_TOGGLE_BASE: usize = 100;
/// "Somente este dispositivo".
const CMD_ONLY_BASE: usize = 200;

/// HID e controles: a cada 60 s. O Bluetooth e lido a cada 3 ciclos (ver scan.rs).
/// A bateria muda devagar; plugar/remover algo ou voltar da suspensao forca uma leitura.
const POLL: Duration = Duration::from_secs(60);
/// Com jogo ou outro app em tela cheia nao ha leitura nenhuma; confere a cada 10 s se saiu.
const PAUSED_CHECK: Duration = Duration::from_secs(10);

static READINGS: Mutex<Option<Vec<Reading>>> = Mutex::new(None);
static WAKE: OnceLock<Sender<()>> = OnceLock::new();
/// `--demo`: leituras falsas cobrindo todos os estados visuais.
pub static DEMO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct App {
    controller: HWND,
    widget: HWND,
    tooltip: HWND,
    taskbar_created: u32,
    light: bool,
    width: i32,
}

thread_local! {
    static APP: RefCell<App> = const {
        RefCell::new(App {
            controller: null_mut(),
            widget: null_mut(),
            tooltip: null_mut(),
            taskbar_created: 0,
            light: false,
            width: 0,
        })
    };
}

/// Emprestimos curtos; nunca segure um durante uma chamada Win32 que despacha mensagens.
fn with<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(&mut a.borrow_mut()))
}

pub fn run() {
    unsafe {
        let mutex_name = wide(r"Local\WirelessBatteryTray");
        CreateMutexW(null(), 0, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return; // ja esta rodando
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        settings::refresh_autostart_path();
        settings::register_app_id();

        let hinst = GetModuleHandleW(null());
        let ctrl_class = wide("WirelessBatteryController");
        register(hinst, &ctrl_class, controller_proc);
        register(hinst, &wide("WirelessBatteryWidget"), widget_proc);

        let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        let controller = CreateWindowExW(
            WS_EX_TOOLWINDOW, ctrl_class.as_ptr(), ctrl_class.as_ptr(), WS_POPUP,
            0, 0, 0, 0, null_mut(), null_mut(), hinst, null(),
        );
        with(|a| {
            a.controller = controller;
            a.taskbar_created = taskbar_created;
            a.light = settings::light_theme();
        });

        let (tx, rx) = mpsc::channel();
        let _ = WAKE.set(tx);
        let controller_addr = controller as usize;
        std::thread::spawn(move || worker(controller_addr, rx));

        ensure_widget();
        SetTimer(controller, TIMER_PLACE, 1000, None);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe fn register(hinst: HINSTANCE, class: &[u16], proc_: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT) {
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc_),
        hInstance: hinst,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    RegisterClassW(&wc);
}

pub(crate) fn demo_readings() -> Vec<Reading> {
    let r = |id: &str, name: &str, kind, status| Reading { id: id.into(), name: name.into(), kind, status };
    vec![
        r("demo:headset", "MCHOSE V9 Pro", Kind::Headset, Status::Battery { percent: 64, charging: true }),
        r("demo:earbuds", "AirPods Pro", Kind::Earbuds, Status::Battery { percent: 12, charging: false }),
        r("demo:mouse", "PRO X SUPERLIGHT 2", Kind::Mouse, Status::Battery { percent: 87, charging: false }),
        r("demo:keyboard", "AULA F75", Kind::Keyboard, Status::Battery { percent: 45, charging: false }),
        r("demo:gamepad", "Xbox Wireless Controller", Kind::Gamepad, Status::ChargingNoLevel),
    ]
}

fn worker(controller: usize, rx: Receiver<()>) {
    if DEMO.load(std::sync::atomic::Ordering::Relaxed) {
        *READINGS.lock().unwrap() = Some(demo_readings());
        unsafe { PostMessageW(controller as HWND, WM_APP_UPDATE, 0, 0) };
        return;
    }
    init_com();
    let mut scanner = Scanner::new();
    let mut alerts = notify::LowBatteryAlerts::default();
    // Windows.Gaming.Input so preenche a lista de controles depois do primeiro uso:
    // a segunda leitura sai em 3 s para o controle aparecer logo.
    let mut wait = Duration::from_secs(3);
    let mut forced = true;
    let mut was_paused = false;
    loop {
        let paused = fullscreen_app();
        if paused {
            // Nada de HID nem Bluetooth enquanto o jogo roda; a barra fica escondida mesmo.
            wait = PAUSED_CHECK;
        } else {
            // Saiu do jogo: le na hora, incluindo o Bluetooth.
            let readings = scanner.scan(forced || was_paused);
            alerts.check(&readings);
            *READINGS.lock().unwrap() = Some(readings);
            unsafe { PostMessageW(controller as HWND, WM_APP_UPDATE, 0, 0) };
            trim_working_set();
            if was_paused {
                wait = POLL;
            }
        }
        was_paused = paused;

        forced = match rx.recv_timeout(wait) {
            Ok(()) => {
                // Eventos de dispositivo chegam em rajadas: espera assentar e le uma vez.
                std::thread::sleep(Duration::from_millis(1500));
                while rx.try_recv().is_ok() {}
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if !paused {
            wait = POLL;
        }
    }
}

/// O app fica parado entre as leituras: devolve a memoria ao Windows ate a proxima.
/// As paginas vao para a lista de espera e voltam rapido quando usadas.
/// (renanleonel/windows-taskbar-battery, MIT)
fn trim_working_set() {
    unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
}

/// Jogo em tela cheia (exclusiva ou sem bordas), apresentacao ou outro app em tela cheia.
fn fullscreen_app() -> bool {
    let mut state = 0;
    let ok = unsafe { SHQueryUserNotificationState(&mut state) } == 0;
    ok && matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
}

fn wake() {
    if let Some(tx) = WAKE.get() {
        let _ = tx.send(());
    }
}

fn readings() -> Vec<Reading> {
    READINGS.lock().unwrap().clone().unwrap_or_default()
}

/// O que vai para a barra, conforme o modo escolhido no menu.
fn shown(all: Vec<Reading>) -> Vec<Reading> {
    match settings::display() {
        Display::All => {
            let hidden = settings::hidden_devices();
            all.into_iter().filter(|r| !hidden.contains(&r.id) && r.status != Status::NotConnected).collect()
        }
        // No modo "somente um", o dispositivo fica na barra mesmo desconectado ("—"),
        // para nao sumir sem explicacao.
        Display::Only(id) => all.into_iter().filter(|r| r.id == id).collect(),
    }
}

unsafe fn find_tray() -> HWND {
    FindWindowW(wide("Shell_TrayWnd").as_ptr(), null())
}

unsafe fn ensure_widget() {
    let widget = with(|a| a.widget);
    if !widget.is_null() && IsWindow(widget) != 0 {
        return;
    }
    let tray = find_tray();
    if tray.is_null() {
        return; // Explorer ainda nao subiu; o timer tenta de novo.
    }
    let class = wide("WirelessBatteryWidget");
    let w = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class.as_ptr(), null(), WS_CHILD | WS_CLIPSIBLINGS,
        0, 0, 1, 1, tray, null_mut(), GetModuleHandleW(null()), null(),
    );
    let (controller, old_tip) = with(|a| (a.controller, a.tooltip));
    if !old_tip.is_null() {
        DestroyWindow(old_tip); // a ferramenta dele era o widget antigo (morto)
    }
    let tip = if w.is_null() { null_mut() } else { create_tooltip(controller, w) };
    with(|a| {
        a.widget = w;
        a.tooltip = tip;
        a.width = 0;
    });
    redraw();
}

/// Dica ao passar o mouse: nome completo e estado de cada dispositivo.
unsafe fn create_tooltip(owner: HWND, widget: HWND) -> HWND {
    let icc = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_WIN95_CLASSES };
    InitCommonControlsEx(&icc);
    let tip = CreateWindowExW(
        WS_EX_TOPMOST, wide("tooltips_class32").as_ptr(), null(), WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
        CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
        owner, null_mut(), GetModuleHandleW(null()), null(),
    );
    if tip.is_null() {
        return tip;
    }
    let mut text = wide("");
    let mut ti = tool_info(widget, owner, &mut text);
    SendMessageW(tip, TTM_ADDTOOLW, 0, &mut ti as *mut _ as LPARAM);
    SendMessageW(tip, TTM_SETMAXTIPWIDTH, 0, 600); // habilita varias linhas
    tip
}

unsafe fn tool_info(widget: HWND, owner: HWND, text: &mut [u16]) -> TTTOOLINFOW {
    let mut ti: TTTOOLINFOW = zeroed();
    ti.cbSize = size_of::<TTTOOLINFOW>() as u32;
    ti.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
    ti.hwnd = owner;
    ti.uId = widget as usize;
    ti.lpszText = text.as_mut_ptr();
    ti
}

unsafe fn set_tooltip_text(lines: &[String]) {
    let (tip, widget, controller) = with(|a| (a.tooltip, a.widget, a.controller));
    if tip.is_null() {
        return;
    }
    let mut text = wide(&lines.join("\r\n"));
    let ti = tool_info(widget, controller, &mut text);
    SendMessageW(tip, TTM_UPDATETIPTEXTW, 0, &ti as *const _ as LPARAM);
}

unsafe fn redraw() {
    let (widget, light) = with(|a| (a.widget, a.light));
    if widget.is_null() || IsWindow(widget) == 0 {
        return;
    }
    let tray = find_tray();
    if tray.is_null() {
        return;
    }
    let mut tr: RECT = zeroed();
    GetWindowRect(tray, &mut tr);
    let dpi = GetDpiForWindow(tray).max(96);
    let shown = shown(readings());
    set_tooltip_text(&shown.iter().map(|r| format!("{}: {}", r.name, r.status)).collect::<Vec<_>>());
    let items: Vec<_> = shown.iter().map(|r| (r.kind, r.status)).collect();
    let width = if items.is_empty() { 0 } else { render::draw(widget, &items, dpi, tr.bottom - tr.top, light) };
    with(|a| a.width = width);
    place();
}

/// Mantem o widget logo a esquerda da area de notificacao e acima do conteudo XAML da barra.
unsafe fn place() {
    let (widget, width) = with(|a| (a.widget, a.width));
    if widget.is_null() || IsWindow(widget) == 0 {
        return;
    }
    if width == 0 {
        ShowWindow(widget, SW_HIDE); // nada para mostrar
        return;
    }
    let tray = find_tray();
    if tray.is_null() {
        return;
    }
    let mut tr: RECT = zeroed();
    GetWindowRect(tray, &mut tr);
    let notify = FindWindowExW(tray, null_mut(), wide("TrayNotifyWnd").as_ptr(), null());
    let mut right = tr.right;
    if !notify.is_null() {
        let mut nr: RECT = zeroed();
        GetWindowRect(notify, &mut nr);
        if nr.right > nr.left {
            right = nr.left;
        }
    }
    let margin = 4 * GetDpiForWindow(tray).max(96) as i32 / 96;
    let (x, h) = (right - tr.left - width - margin, tr.bottom - tr.top);

    let mut cur: RECT = zeroed();
    GetWindowRect(widget, &mut cur);
    let moved = cur.left - tr.left != x || cur.top != tr.top || cur.right - cur.left != width || cur.bottom - cur.top != h;
    let on_top = GetWindow(tray, GW_CHILD) == widget;
    if moved || !on_top || IsWindowVisible(widget) == 0 {
        SetWindowPos(widget, HWND_TOP, x, 0, width, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let devices = readings();
    let hidden = settings::hidden_devices();
    let display = settings::display();
    let menu = CreatePopupMenu();
    let submenu = CreatePopupMenu();
    let mut labels = Vec::new(); // mantem os buffers UTF-16 vivos enquanto o menu existe
    let mut add = |m: HMENU, flags: u32, id: usize, text: &str| {
        let t = wide(text);
        AppendMenuW(m, flags, id, t.as_ptr());
        labels.push(t);
    };
    let checked = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };

    // Lista: marcado = aparece na barra no modo "todos".
    for (i, r) in devices.iter().enumerate() {
        let label = format!("{}  —  {}", r.name, r.status).replace('&', "&&");
        add(menu, MF_STRING | checked(!hidden.contains(&r.id)), CMD_TOGGLE_BASE + i, &label);
    }
    if devices.is_empty() {
        add(menu, MF_STRING | MF_GRAYED, 0, "Nenhum dispositivo encontrado");
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());

    // Submenu "Mostrar na barra": todos ou somente um.
    add(submenu, MF_STRING, CMD_SHOW_ALL, "Todos os dispositivos");
    if !devices.is_empty() {
        AppendMenuW(submenu, MF_SEPARATOR, 0, null());
    }
    for (i, r) in devices.iter().enumerate() {
        add(submenu, MF_STRING, CMD_ONLY_BASE + i, &format!("Somente {}", r.name).replace('&', "&&"));
    }
    let selected = match &display {
        Display::All => CMD_SHOW_ALL,
        Display::Only(id) => devices.iter().position(|r| &r.id == id).map_or(0, |i| CMD_ONLY_BASE + i),
    };
    if selected != 0 {
        let last = CMD_ONLY_BASE + devices.len().saturating_sub(1);
        CheckMenuRadioItem(submenu, CMD_SHOW_ALL as u32, last.max(CMD_SHOW_ALL) as u32, selected as u32, MF_BYCOMMAND);
    }
    add(menu, MF_STRING | MF_POPUP, submenu as usize, "Mostrar na barra");
    AppendMenuW(menu, MF_SEPARATOR, 0, null());

    add(menu, MF_STRING, CMD_REFRESH, "Atualizar agora");
    add(menu, MF_STRING | checked(settings::alerts_enabled()), CMD_ALERTS, "Avisos de bateria baixa");
    add(menu, MF_STRING | checked(settings::autostart_enabled()), CMD_AUTOSTART, "Iniciar com o Windows");
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    add(menu, MF_STRING, CMD_EXIT, "Sair");

    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RIGHTALIGN, pt.x, pt.y, 0, hwnd, null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu); // destroi o submenu junto
    drop(labels);

    match cmd as usize {
        0 => {}
        CMD_REFRESH => wake(),
        CMD_AUTOSTART => settings::set_autostart(!settings::autostart_enabled()),
        CMD_ALERTS => settings::set_alerts(!settings::alerts_enabled()),
        CMD_SHOW_ALL => {
            settings::set_display(&Display::All);
            redraw();
        }
        CMD_EXIT => {
            let widget = with(|a| a.widget);
            if !widget.is_null() {
                DestroyWindow(widget);
            }
            DestroyWindow(hwnd);
        }
        c if c >= CMD_ONLY_BASE => {
            if let Some(r) = devices.get(c - CMD_ONLY_BASE) {
                settings::set_display(&Display::Only(r.id.clone()));
                redraw();
            }
        }
        c if c >= CMD_TOGGLE_BASE => {
            if let Some(r) = devices.get(c - CMD_TOGGLE_BASE) {
                settings::toggle_hidden(&r.id);
                redraw();
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let taskbar_created = with(|a| a.taskbar_created);
    match msg {
        WM_APP_UPDATE => redraw(),
        WM_TIMER => {
            ensure_widget();
            place();
        }
        WM_SETTINGCHANGE => {
            with(|a| a.light = settings::light_theme());
            redraw();
        }
        WM_DEVICECHANGE => {
            wake();
            return 1;
        }
        WM_POWERBROADCAST => {
            if wp == PBT_APMRESUMEAUTOMATIC {
                wake(); // PC voltou da suspensao: os dispositivos podem ter mudado
            }
            return 1;
        }
        WM_APP_MENU => show_menu(hwnd),
        WM_DESTROY => PostQuitMessage(0),
        m if m == taskbar_created && m != 0 => {
            // Explorer reiniciou: o widget antigo morreu com a barra antiga.
            with(|a| a.widget = null_mut());
            ensure_widget();
        }
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    0
}

unsafe extern "system" fn widget_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Clique esquerdo ou direito abre o menu.
        WM_LBUTTONUP | WM_RBUTTONUP => {
            let controller = with(|a| a.controller);
            PostMessageW(controller, WM_APP_MENU, 0, 0);
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
