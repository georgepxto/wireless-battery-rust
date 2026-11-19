//! Controles 8BitDo sem fio (VID 2DC8).
//! 1. Reports de controle D-input / Bluetooth (pagina 0001, uso 0005, report 01 ou 04): byte 14,
//!    bits 0-6 = %, bit 7 = carregando (SDL_hidapi_8bitdo.c).
//! 2. Canal do fabricante (pagina FF7A ou FFA0, uso 0001, 64 bytes, output 81 / input 02). No modo
//!    X-input o dongle 2.4 GHz finge ser um controle Xbox 360 com fio, mas responde a consultas:
//!    [81 04 20 01] -> byte 18 = 1 se o controle esta ligado ao dongle;
//!    [81 04 21 01 00 00 06] -> byte 18 = % (1..100); [81 04 00 01] -> bytes 22..23 = PID interno
//!    (6013 = dongle 2.4G, 6012 = controle no cabo). Na base de carga o radio desliga e o dongle
//!    muda de 310B para 6013: o ultimo nivel fica visivel como "dormindo". (MiguelSanzBr/8bitdo-ultimate2-linux)
//!
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), eightbitdo.cpp.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::{DeviceInfo, HidDevice};
use std::sync::atomic::{AtomicI32, Ordering};

const VID: u16 = 0x2DC8;
const DONGLE: u16 = 0x6013;
const DONGLE_24G_IDLE: u16 = 0x3109;

/// (pid, nome, precisa do feature 06 antes de ler)
const MODELS: &[(u16, &str, bool)] = &[
    (0x310B, "8BitDo Ultimate 2 Wireless", false),
    (0x6012, "8BitDo Ultimate 2", false),
    (0x6013, "8BitDo Ultimate 2 Wireless", false),
    (0x202F, "8BitDo Ultimate 3-mode", false),
    (0x202E, "8BitDo Ultimate 3-mode", false),
    (0x301B, "8BitDo Ultimate 2C Wireless", false),
    (0x301A, "8BitDo Ultimate 2C Wireless", false),
    (0x310A, "8BitDo Ultimate 2C Wireless", false),
    (0x3012, "8BitDo Ultimate 2.4G", false),
    (0x3013, "8BitDo Ultimate 2.4G", false),
    (0x3109, "8BitDo Ultimate 2.4G", false),
    (0x6007, "8BitDo Ultimate Bluetooth", false),
    (0x6009, "8BitDo Pro 3", true),
    (0x6003, "8BitDo Pro 2", true),
    (0x6006, "8BitDo Pro 2", true),
    (0x6001, "8BitDo SN30 Pro", true),
    (0x6101, "8BitDo SN30 Pro", true),
    (0x6000, "8BitDo SF30 Pro", true),
    (0x6100, "8BitDo SF30 Pro", true),
];

static LAST_ULTIMATE2: AtomicI32 = AtomicI32::new(-1);

fn model(pid: u16) -> Option<&'static (u16, &'static str, bool)> {
    MODELS.iter().find(|m| m.0 == pid)
}

fn is_ultimate2(pid: u16) -> bool {
    matches!(pid, 0x310B | 0x6012 | 0x6013)
}

fn name(c: &DeviceInfo) -> String {
    if let Some(m) = model(c.product_id()) {
        return m.1.into();
    }
    match c.product_string().filter(|p| !p.is_empty()) {
        Some(p) if p.starts_with("8BitDo") || p.starts_with("8Bitdo") => p.into(),
        Some(p) => format!("8BitDo {p}"),
        None => "8BitDo Wireless Controller".into(),
    }
}

fn key(pid: u16) -> String {
    if is_ultimate2(pid) {
        "8bitdo:ultimate2".into()
    } else {
        format!("8bitdo:{}", hex4(pid))
    }
}

fn query(d: &HidDevice, req: &[u8], lo: u8, hi: u8) -> Option<Vec<u8>> {
    d.flush();
    d.write(req).ok()?;
    let end = Deadline::in_ms(300);
    while !end.expired() {
        let r = d.read(end.left()).filter(|r| r.len() >= 19)?;
        if r[0] == 0x02 && r[1] == 0x04 && r[4] == lo && r[5] == hi {
            return Some(r);
        }
    }
    None
}

fn vendor(ctx: &Ctx, c: &DeviceInfo) -> Option<Status> {
    let d = ctx.open(c)?;
    let pid = c.product_id();
    let ult2 = is_ultimate2(pid);
    let mut s = Status::new(key(pid), name(c), Kind::Gamepad);
    let last = LAST_ULTIMATE2.load(Ordering::Relaxed);
    let linked = query(&d, &[0x81, 0x04, 0x20, 0x01], 0x20, 0x01).map(|r| r[18] != 0);
    if linked != Some(false) {
        if let Some(r) = query(&d, &[0x81, 0x04, 0x21, 0x01, 0x00, 0x00, 0x06], 0x21, 0x01) {
            let pct = (r[18] & 0x7F) as i32;
            if (1..=100).contains(&pct) {
                s.percent = pct;
                s.charging = r[18] & 0x80 != 0 && pct < 100;
                if ult2 {
                    LAST_ULTIMATE2.store(pct, Ordering::Relaxed);
                }
                return Some(s);
            }
        }
    }
    let idle = |mut s: Status| {
        if ult2 && last >= 0 {
            s.asleep = true; // na base de carga: mostra o ultimo nivel
        } else {
            s.connected = false;
        }
        s
    };
    if let Some(r) = query(&d, &[0x81, 0x04, 0x00, 0x01], 0x00, 0x01).filter(|r| r.len() >= 24) {
        let internal = u16::from_le_bytes([r[22], r[23]]);
        if internal != DONGLE && internal != DONGLE_24G_IDLE {
            s.charging = true; // no cabo
            if ult2 && last >= 0 {
                s.percent = last;
            }
            return Some(s);
        }
        return Some(idle(s));
    }
    if pid == DONGLE || pid == DONGLE_24G_IDLE {
        return Some(idle(s));
    }
    None
}

fn dinput(ctx: &Ctx, c: &DeviceInfo) -> Option<Status> {
    let pid = c.product_id();
    let mut dev = None;
    if model(pid).is_some_and(|m| m.2) && c.feature_len() > 0 {
        if let Some(d) = ctx.open(c) {
            let _ = d.get_feature(0x06);
            dev = Some(d);
        }
    }
    let d = dev.or_else(|| ctx.open_read_only(c))?;
    let min = if pid == 0x6012 { 34 } else { 15 };
    let end = Deadline::in_ms(600);
    while !end.expired() {
        let Some(r) = d.read(100) else { continue };
        if r.len() >= min && (r[0] == 0x01 || r[0] == 0x04) && r[14] & 0x7F <= 100 {
            let mut s = Status::new(key(pid), name(c), Kind::Gamepad);
            s.percent = (r[14] & 0x7F) as i32;
            s.charging = r[14] & 0x80 != 0 && s.percent < 100;
            if is_ultimate2(pid) && s.percent > 0 {
                LAST_ULTIMATE2.store(s.percent, Ordering::Relaxed);
            }
            return Some(s);
        }
    }
    None
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(VID) {
        for c in ctx.find(vid, pid, |c| matches!(c.usage_page(), 0xFF7A | 0xFFA0) && c.usage() == 1 && c.input_len() >= 64 && c.output_len() >= 64) {
            if let Some(s) = vendor(ctx, &c) {
                ctx.add(s, vid, pid);
                break;
            }
        }
        if !ctx.is_claimed(vid, pid) && model(pid).is_some() {
            for c in ctx.find(vid, pid, |c| c.usage_page() == 0x01 && matches!(c.usage(), 0x04 | 0x05) && c.input_len() >= 15) {
                if let Some(s) = dinput(ctx, &c) {
                    ctx.add(s, vid, pid);
                    break;
                }
            }
        }
    }
}
