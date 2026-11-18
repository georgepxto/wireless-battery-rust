//! ASUS ROG (VID 0B05): teclados, mouses, o headset Pelta e o ROG Omni Receiver (1ACE).
//! Consulta [report id][12][sub]: sub 01 = teclado, 07 = mouse. Resposta [id][12][sub]...:
//! teclado [6] % (Falchion [11]), [9] == 1 carregando; mouse [5] nivel (x25 nos antigos), [10] carregando.
//! [1..2] == FF AA: o dispositivo atras do receptor nao respondeu (dormindo / desligado).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), rog.cpp; protocolo e modelos de
//! SteveWang92/peripheral-battery-tray (MIT, baseado no G-Helper), 1gcat/BatteryTool,
//! jimm0thy/pelta-control (MIT) e B1Chat/BatteryWatch (MIT).

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::{DeviceInfo, HidDevice};
use std::collections::HashSet;
use std::sync::Mutex;

struct Model {
    pid: u16,
    name: &'static str,
    keyboard: bool,
    report_id: u8,
    iface: i32,
    col: Option<&'static str>,
    offset: usize,
    scale: i32,
}

const fn kb(pid: u16, name: &'static str, report_id: u8, iface: i32, col: Option<&'static str>, offset: usize) -> Model {
    Model { pid, name, keyboard: true, report_id, iface, col, offset, scale: 1 }
}
const fn ms(pid: u16, name: &'static str, report_id: u8, iface: i32, col: Option<&'static str>, offset: usize, scale: i32) -> Model {
    Model { pid, name, keyboard: false, report_id, iface, col, offset, scale }
}
const C2: Option<&str> = Some("col02");
const C3: Option<&str> = Some("col03");

const MODELS: &[Model] = &[
    kb(0x1A85, "ROG Azoth", 2, 2, C2, 6),
    kb(0x1A85, "ROG Azoth", 0, 1, None, 6),
    kb(0x1A83, "ROG Azoth", 0, 1, None, 6),
    kb(0x1B42, "ROG Azoth Extreme", 2, 2, C2, 6),
    kb(0x1B3F, "ROG Azoth Extreme", 0, 1, None, 6),
    kb(0x1CF1, "ROG Azoth Extreme SE", 2, 2, C2, 6),
    kb(0x1CEF, "ROG Azoth Extreme SE", 0, 1, None, 6),
    kb(0x1C25, "ROG Azoth X", 0, 1, None, 6),
    kb(0x1C24, "ROG Azoth X", 0, 1, None, 6),
    kb(0x193E, "ROG Falchion", 0, 1, None, 11),
    kb(0x193C, "ROG Falchion", 0, 1, None, 11),
    kb(0x1B04, "ROG Falchion RX", 0, -1, None, 11),
    kb(0x1B06, "ROG Falchion RX Low Profile", 2, 2, C2, 11),
    kb(0x1AB0, "ROG Strix Scope II 96", 2, 2, C2, 6),
    kb(0x1AAE, "ROG Strix Scope II 96", 0, 1, None, 6),
    kb(0x1B7A, "ROG Strix Scope II 96 RX", 2, 2, C2, 6),
    kb(0x1B78, "ROG Strix Scope II 96 RX", 0, 1, None, 6),
    kb(0x1A07, "ROG Strix Scope RX TKL", 0, -1, None, 6),
    ms(0x18E5, "ROG Chakram", 0, 0, None, 5, 25),
    ms(0x18E3, "ROG Chakram", 0, 0, None, 5, 25),
    ms(0x1A1A, "ROG Chakram X", 0, 0, None, 5, 1),
    ms(0x1A18, "ROG Chakram X", 0, 0, None, 5, 1),
    ms(0x18A0, "ROG Gladius II Wireless", 0, 2, None, 5, 25),
    ms(0x197F, "ROG Gladius III Wireless", 0, 0, None, 5, 1),
    ms(0x197D, "ROG Gladius III Wireless", 0, 0, None, 5, 1),
    ms(0x1A72, "ROG Gladius III Aimpoint", 3, 2, C3, 5, 1),
    ms(0x1A72, "ROG Gladius III Aimpoint", 0, 0, None, 5, 1),
    ms(0x1A70, "ROG Gladius III Aimpoint", 0, 0, None, 5, 1),
    ms(0x1B0C, "ROG Gladius III Aimpoint EVA-02", 0, 0, None, 5, 1),
    ms(0x1B0A, "ROG Gladius III Aimpoint EVA-02", 0, 0, None, 5, 1),
    ms(0x1A94, "ROG Harpe Ace Aim Lab Edition", 3, 2, C3, 5, 1),
    ms(0x1A94, "ROG Harpe Ace Aim Lab Edition", 0, 0, None, 5, 1),
    ms(0x1A92, "ROG Harpe Ace Aim Lab Edition", 0, 0, None, 5, 1),
    ms(0x1B68, "ROG Harpe Ace Extreme", 3, 2, C3, 5, 1),
    ms(0x1B69, "ROG Harpe Ace Extreme", 3, 2, C3, 5, 1),
    ms(0x1B67, "ROG Harpe Ace Extreme", 0, 0, None, 5, 1),
    ms(0x1B65, "ROG Harpe Ace Mini", 3, 2, C3, 5, 1),
    ms(0x1B63, "ROG Harpe Ace Mini", 0, 0, None, 5, 1),
    ms(0x1AD0, "ROG Harpe II Ace", 3, 2, C3, 5, 1),
    ms(0x1C69, "ROG Harpe II Ace", 0, 0, None, 5, 1),
    ms(0x1960, "ROG Keris Wireless", 0, 0, None, 5, 25),
    ms(0x195E, "ROG Keris Wireless", 0, 0, None, 5, 25),
    ms(0x1A59, "ROG Keris Wireless EVA Edition", 0, 0, None, 5, 25),
    ms(0x1A57, "ROG Keris Wireless EVA Edition", 0, 0, None, 5, 25),
    ms(0x1A68, "ROG Keris Wireless Aimpoint", 3, 2, C3, 5, 1),
    ms(0x1A6A, "ROG Keris Wireless Aimpoint", 3, 2, C3, 5, 1),
    ms(0x1A68, "ROG Keris Wireless Aimpoint", 0, 0, None, 5, 1),
    ms(0x1A66, "ROG Keris Wireless Aimpoint", 0, 0, None, 5, 1),
    ms(0x1B1A, "ROG Keris II Ace", 3, 2, C3, 5, 1),
    ms(0x1B18, "ROG Keris II Ace", 3, 2, C3, 5, 1),
    ms(0x1B16, "ROG Keris II Ace", 0, 0, None, 5, 1),
    ms(0x1C0E, "ROG Keris II Origin", 3, 2, C3, 5, 1),
    ms(0x1C0C, "ROG Keris II Origin", 0, 0, None, 5, 1),
    ms(0x1D4E, "ROG Keris II Origin KJP", 3, 2, C3, 5, 1),
    ms(0x1D4C, "ROG Keris II Origin KJP", 0, 0, None, 5, 1),
    ms(0x1908, "ROG Pugio II", 0, 0, None, 5, 25),
    ms(0x1906, "ROG Pugio II", 0, 0, None, 5, 25),
    ms(0x1979, "ROG Spatha X", 0, 0, None, 5, 1),
    ms(0x1977, "ROG Spatha X", 0, 0, None, 5, 1),
    ms(0x18B4, "ROG Strix Carry", 0, 1, None, 7, 25),
    ms(0x1949, "ROG Strix Impact II Wireless", 0, 0, None, 5, 25),
    ms(0x1947, "ROG Strix Impact II Wireless", 0, 0, None, 5, 25),
    ms(0x1AD7, "ROG Strix Impact III Wireless", 3, 2, C3, 5, 1),
];

const VID: u16 = 0x0B05;
const OMNI: u16 = 0x1ACE;
const PELTA: u16 = 0x1B84;
/// Modelos que ja responderam de verdade num canal do receptor.
static AWAKE: Mutex<Option<HashSet<u16>>> = Mutex::new(None);

fn path_has(c: &DeviceInfo, tag: &str) -> bool {
    c.path().to_string_lossy().to_ascii_lowercase().contains(tag)
}

enum Reply {
    Ok(Vec<u8>),
    Asleep,
    None,
}

fn query(d: &HidDevice, rid: u8, cmd: u8, sub: u8) -> Reply {
    d.flush();
    if d.write(&[rid, cmd, sub]).is_err() {
        return Reply::None;
    }
    let end = Deadline::in_ms(500);
    while !end.expired() {
        let Some(mut r) = d.read(end.left()).filter(|r| r.len() >= 11) else { return Reply::None };
        if rid == 0 {
            r.insert(0, 0); // devolve o lugar do report id para os offsets baterem
        }
        if r.len() < 12 {
            continue;
        }
        if r[1] == 0xFF && r[2] == 0xAA {
            return Reply::Asleep;
        }
        if r[1] == cmd && r[2] == sub {
            return Reply::Ok(r);
        }
    }
    Reply::None
}

fn read_model(d: &HidDevice, m: &Model) -> Option<Status> {
    let res = query(d, m.report_id, 0x12, if m.keyboard { 0x01 } else { 0x07 });
    let key = format!("rog:{}:{}", hex4(m.pid), if m.keyboard { "kb" } else { "ms" });
    let mut s = Status::new(key, m.name, if m.keyboard { Kind::Keyboard } else { Kind::Mouse });
    match res {
        Reply::None => return None,
        Reply::Asleep => s.asleep = true,
        Reply::Ok(r) if m.keyboard => {
            if r[m.offset] <= 100 {
                s.percent = r[m.offset] as i32;
            }
            s.charging = r[9] == 1;
        }
        Reply::Ok(r) => {
            let v = r[m.offset] as i32 * m.scale;
            if (1..=100).contains(&v) {
                s.percent = v;
            } else {
                s.asleep = true; // 0 = indisponivel, nao vazio
            }
            s.charging = r[10] != 0;
        }
    }
    Some(s)
}

fn add_once(ctx: &mut Ctx, seen: &mut Vec<String>, s: Status) {
    if !seen.contains(&s.name) {
        seen.push(s.name.clone());
        ctx.out.push(s.into_observation());
    }
}

/// Omni Receiver: [01 A0 00] na col01 lista os PIDs pareados; cada um tem seu canal no receptor.
fn poll_omni(ctx: &mut Ctx, seen: &mut Vec<String>) {
    if ctx.is_claimed(VID, OMNI) {
        return;
    }
    let list = ctx.find(VID, OMNI, |c| c.interface_number() == 2 && c.output_len() >= 12 && path_has(c, "col01"));
    let Some(first) = list.first() else { return };
    let Some(d) = ctx.open(first) else { return };
    let Reply::Ok(r) = query(&d, 0x01, 0xA0, 0x00) else { return };
    ctx.claim(VID, OMNI);
    let mut at = 5;
    while at + 1 < r.len() {
        let pid = u16::from_le_bytes([r[at], r[at + 1]]);
        if pid == 0 {
            break;
        }
        if let Some(m) = MODELS.iter().find(|m| m.pid == pid && m.report_id != 0) {
            let col = m.col.unwrap_or("");
            for c in ctx.find(VID, OMNI, |c| c.interface_number() == 2 && c.output_len() >= 12 && path_has(c, col)) {
                if let Some(s) = ctx.open(&c).and_then(|dev| read_model(&dev, m)) {
                    add_once(ctx, seen, s);
                    break;
                }
            }
        }
        at += 4;
    }
}

/// Headset ROG Pelta no dongle 1B84: report CC, sem endpoint OUT (vai por HidD_SetOutputReport).
fn pelta_query(d: &HidDevice, cmd: u8) -> Reply {
    d.flush();
    let end = Deadline::in_ms(1200);
    let mut r = d.set_output_and_read(&[0xCC, 0x12, cmd], 1200);
    while let Some(x) = r.filter(|x| x.len() >= 8) {
        if x[0] == 0xCC && x[1] == 0xFF && x[2] == 0xAA {
            return Reply::Asleep;
        }
        if x[0] == 0xCC && x[1] == 0x12 {
            if x[5] == 0xFF && x[6] == 0xAA {
                return Reply::Asleep;
            }
            if x[2] == cmd {
                return Reply::Ok(x);
            }
        }
        if end.expired() {
            break;
        }
        r = d.read(end.left());
    }
    Reply::None
}

fn poll_pelta(ctx: &mut Ctx) {
    if ctx.is_claimed(VID, PELTA) {
        return;
    }
    for c in ctx.find(VID, PELTA, |c| c.interface_number() == 3 && c.usage_page() == 0xFF00 && c.usage() == 0x0001 && c.input_len() >= 8 && c.output_len() >= 8) {
        let Some(d) = ctx.open(&c) else { continue };
        let mut s = Status::new(format!("rog:{}:hs", hex4(PELTA)), "ROG Pelta", Kind::Headset);
        match pelta_query(&d, 0x07) {
            Reply::None => continue,
            Reply::Asleep => s.asleep = true,
            Reply::Ok(r) => {
                if r[6] <= 100 {
                    s.percent = r[6] as i32;
                } else {
                    s.asleep = true; // FE = nivel ainda desconhecido
                }
                if let Reply::Ok(ch) = pelta_query(&d, 0x08) {
                    s.charging = ch[5] == 1;
                }
            }
        }
        ctx.out.push(s.into_observation());
        ctx.claim(VID, PELTA);
        return;
    }
}

pub fn poll(ctx: &mut Ctx) {
    let mut seen = Vec::new(); // um por nome (cabo + receptor juntos enquanto carrega)
    poll_pelta(ctx);
    poll_omni(ctx, &mut seen);
    for m in MODELS {
        if ctx.is_claimed(VID, m.pid) {
            continue;
        }
        let colls = ctx.find(VID, m.pid, |c| {
            c.output_len() >= 12 && c.usage_page() >= 0xFF00 && (m.iface < 0 || c.interface_number() == m.iface) && m.col.is_none_or(|t| path_has(c, t))
        });
        for c in &colls {
            let Some(s) = ctx.open(c).and_then(|d| read_model(&d, m)) else { continue };
            // Um canal de receptor responde "dormindo" mesmo sem nada pareado: so mostra depois
            // de uma resposta real.
            let mut awake = AWAKE.lock().unwrap();
            let awake = awake.get_or_insert_with(HashSet::new);
            if m.report_id != 0 && s.asleep && !awake.contains(&m.pid) {
                continue;
            }
            if m.report_id != 0 && !s.asleep {
                awake.insert(m.pid);
            }
            add_once(ctx, &mut seen, s);
            ctx.claim(VID, m.pid);
            break;
        }
    }
}
