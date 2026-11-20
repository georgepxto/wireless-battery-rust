//! Aparelhos no canal de 64 bytes FF1C:0092 (report 4) da Evision / CHERRY / ATK / VGN:
//! frame [0] report 4, [1..2] soma (LE) de bytes[3..], [3] tipo, [4..] payload; status = tipo 0x1A.
//! - Akko V9 Dash Pro (38EE:000C) e Dash V9 Ultra (38EE:000F, "Akko 8K Receiver").
//! - CHERRY MX 3.0S Wireless (dongle 046A:00EA, cabo 00E3).
//! - Teclados Evision no receptor 320F:5088 / cabo 5055: ATK V75X e os VGN / VXE (S99, V98 Pro, N75 Pro,
//!   VXE75, V87), Feker Alice80 ("Wireless Gaming Keyboard"); conferidos pelo nome do produto.
//! - ATK Z87 (receptor 373B:1012).
//! - CHERRY XTRFY 1K Mouse Receiver (046A:0330): moldura CompX de 17 bytes, report 08.
//!
//! Teclados no receptor sao consultados no maximo a cada 10 min, para nao manter o teclado acordado.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0): akko.cpp, cherry.cpp e
//! atk_keyboard.cpp; protocolo de B1Chat/BatteryWatch (MIT), HeyOkay/HaloBattery (MIT), ATK HUB e VGN HUB.

use super::{kysona, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn status_query() -> Vec<u8> {
    let mut f = vec![0u8; 64];
    f[0] = 0x04;
    f[3] = 0x1A;
    f[4] = 0x03;
    f[7] = 0x55;
    let sum: u32 = f[3..].iter().map(|b| *b as u32).sum();
    f[1..3].copy_from_slice(&(sum as u16).to_le_bytes());
    f
}

/// Envia a consulta de status e devolve a resposta [04 .. .. 1A ... % carregando].
fn query_status(d: &HidDevice, plausible: bool) -> Option<Vec<u8>> {
    d.flush();
    let q = status_query();
    for _ in 0..2 {
        d.write(&q).ok()?;
        let end = Deadline::in_ms(400);
        while !end.expired() {
            let Some(r) = d.read(end.left()) else { break };
            if r.len() >= 10 && r[0] == 0x04 && r[3] == 0x1A && (!plausible || r[8] <= 100) {
                return Some(r);
            }
        }
    }
    None
}

fn ff1c(c: &crate::hid::DeviceInfo) -> bool {
    c.usage_page() == 0xFF1C && c.usage() == 0x0092 && c.input_len() == 64 && c.output_len() == 64
}

pub fn poll_akko(ctx: &mut Ctx) {
    for (pid, name) in [(0x000Cu16, "Akko V9 Dash Pro"), (0x000F, "Akko Dash V9 Ultra")] {
        if ctx.is_claimed(0x38EE, pid) {
            continue;
        }
        for c in ctx.find(0x38EE, pid, ff1c) {
            let Some(r) = ctx.open(&c).and_then(|d| query_status(&d, true)) else { continue };
            let mut s = Status::new(format!("akko:38ee:{pid:04x}"), name, Kind::Mouse);
            s.percent = r[8] as i32;
            s.charging = r[9] != 0;
            ctx.add(s, 0x38EE, pid);
            break;
        }
    }
}

pub fn poll_cherry(ctx: &mut Ctx) {
    // Cabo primeiro: com ele o teclado esta ligado e acordado. Uma entrada so.
    'outer: for pid in [0x00E3u16, 0x00EA] {
        if ctx.is_claimed(0x046A, pid) {
            continue;
        }
        for c in ctx.find(0x046A, pid, ff1c) {
            let Some(r) = ctx.open(&c).and_then(|d| query_status(&d, false)) else { continue };
            let mut s = Status::new("cherry:mx30s", "CHERRY MX 3.0S Wireless", Kind::Keyboard);
            if r[8] <= 100 {
                s.percent = r[8] as i32;
                s.charging = r[9] != 0;
            } else {
                s.asleep = true;
            }
            ctx.add(s, 0x046A, pid);
            break 'outer;
        }
    }
    poll_xtrfy(ctx);
}

fn xtrfy_exchange(d: &HidDevice, cmd: u8) -> Option<Vec<u8>> {
    d.flush();
    d.write(&kysona::request(cmd)).ok()?;
    let end = Deadline::in_ms(500);
    while !end.expired() {
        let r = d.read(end.left())?;
        if r.len() == 17 && r[0] == 0x08 && r[1] == cmd && r[..16].iter().fold(0u8, |a, b| a.wrapping_add(*b)).wrapping_add(r[16]) == 0x55 {
            return Some(r);
        }
    }
    None
}

fn poll_xtrfy(ctx: &mut Ctx) {
    let (vid, pid) = (0x046A, 0x0330);
    if ctx.is_claimed(vid, pid) {
        return;
    }
    let Some(c) = ctx.find(vid, pid, |c| c.usage_page() == 0xFF02 && c.usage() == 0x0002 && c.input_len() == 17 && c.output_len() == 17).into_iter().next() else { return };
    let Some(d) = ctx.open(&c) else { return };
    let mut s = Status::new("cherry:xtrfy", "Cherry XTRFY wireless mouse", Kind::Mouse);
    let mut answered = false;
    if let Some(r) = xtrfy_exchange(&d, 0x03) {
        answered = true;
        s.connected = r[6] != 0;
    }
    if !answered || s.connected {
        if let Some(r) = xtrfy_exchange(&d, 0x04) {
            answered = true;
            s.connected = true;
            if r[6] <= 100 {
                s.percent = r[6] as i32;
            }
            s.charging = r[7] != 0;
        } else if answered {
            s.asleep = true;
        }
    }
    if !answered {
        return; // mouse desligado: o ultimo valor fica
    }
    if !s.connected {
        ctx.claim(vid, pid);
        return;
    }
    ctx.add(s, vid, pid);
}

// ---- teclados Evision (ATK V75X, VGN, VXE, Feker) e ATK Z87 --------------------------------------

/// (nome do produto no receptor ou no cabo, pergunta como o VGN HUB)
const EVISION: &[(&str, bool)] = &[
    ("ATK V75X 2.4G Dongle", false),
    ("ATK V75X", false),
    ("VGN S99 2.4G Dongle", true),
    ("VGN S99", true),
    ("VGN V98pro V2 2.4G Dongle", true),
    ("VGN V98pro V2", true),
    ("VGN V98 PRO 2.4G Dongle", true),
    ("VGN V98pro", true),
    ("VGN N75 PRO 2.4G Dongle", true),
    ("VGN N75 PRO", true),
    ("VGN VXE75 V2 2.4G Dongle", true),
    ("VGN VXE75 V2", true),
    ("VGN VXE75 2.4G Dongle", true),
    ("VGN VXE75", true),
    ("VXE V87 2.4G Dongle", true),
    ("VXE V87", true),
    ("VXE V87 RGB", true),
    ("Wireless Gaming Keyboard", false), // nome generico do receptor (Feker Alice80 e outros)
];

static SEQ: Mutex<u16> = Mutex::new(0);

fn next_seq() -> u16 {
    let mut s = SEQ.lock().unwrap();
    *s = *s % 0xFFFF + 1;
    *s
}

/// Pedido [04][seq lo][seq hi][1A][sub: 00, 06 no VGN, 02 nos mouses Redragon]; resposta com o mesmo seq: [8] %, [9] 0 bateria / 1-2 carregando.
/// `None` tambem quando o teclado esta dormindo (Z87 responde [3] = FF).
pub fn read_evision(d: &HidDevice, sub: u8, wait_ms: u64) -> Option<(i32, bool)> {
    let seq = next_seq().to_le_bytes();
    d.flush();
    let mut req = vec![0u8; 32];
    req[0] = 0x04;
    req[1] = seq[0];
    req[2] = seq[1];
    req[3] = 0x1A;
    req[4] = sub;
    d.write(&req).ok()?;
    let end = Deadline::in_ms(wait_ms);
    while !end.expired() {
        let Some(r) = d.read(end.left().min(100)) else { continue };
        if r.len() < 4 || r[0] != 0x04 || r[1] != seq[0] || r[2] != seq[1] {
            continue;
        }
        if r[3] == 0xFF {
            return None;
        }
        if r.len() < 10 || r[3] != 0x1A || r[5] != 0 || r[6] != 0 {
            continue;
        }
        if r[8] > 100 || r[9] > 2 {
            return None;
        }
        return Some((r[8] as i32, r[9] == 1 || r[9] == 2));
    }
    None
}

/// Ultima leitura e quando perguntar de novo: Evision 320F e Z87.
type Cached = (Option<Instant>, Option<(i32, bool)>);

static EV_CACHE: Mutex<[Cached; 2]> = Mutex::new([(None, None), (None, None)]);

pub fn poll_atk_keyboard(ctx: &mut Ctx) {
    poll_z87(ctx);
    let (vid, receiver, cable) = (0x320F, 0x5088, 0x5055);
    for pid in [cable, receiver] {
        if ctx.is_claimed(vid, pid) {
            continue;
        }
        let is_cable = pid == cable;
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFF1C && c.usage() == 0x0092) {
            let product = c.product_string().unwrap_or("").trim_end().to_string();
            let Some(&(model, vgn)) = EVISION.iter().find(|m| m.0.eq_ignore_ascii_case(&product)) else { continue };
            let name = if model.len() > 12 && model.to_ascii_lowercase().ends_with(" 2.4g dongle") { &model[..model.len() - 12] } else { model };
            let mut cache = EV_CACHE.lock().unwrap();
            let entry = &mut cache[0];
            let ask = is_cable || entry.0.is_none_or(|t| t.elapsed() >= Duration::from_secs(600));
            let mut s = Status::new("atk_keyboard:5088", name, Kind::Keyboard);
            let reading = if ask {
                entry.0 = Some(Instant::now());
                let r = ctx.open(&c).and_then(|d| read_evision(&d, if vgn { 0x06 } else { 0 }, 2500));
                if r.is_some() {
                    entry.1 = r;
                }
                r
            } else {
                entry.1
            };
            match reading {
                Some((p, chg)) => {
                    s.percent = p;
                    s.charging = chg;
                }
                None => s.asleep = true,
            }
            drop(cache);
            ctx.add(s, vid, pid);
            if is_cable {
                ctx.claim(vid, receiver);
            }
            return;
        }
    }
}

fn poll_z87(ctx: &mut Ctx) {
    let (vid, pid) = (0x373B, 0x1012);
    if ctx.is_claimed(vid, pid) {
        return;
    }
    let found = ctx.find(vid, pid, |c| ff1c(c) && c.product_string().unwrap_or("").trim_end().eq_ignore_ascii_case("ATK Z87 Dongle"));
    let mut cache = EV_CACHE.lock().unwrap();
    let Some(c) = found.first() else {
        cache[1] = (None, None); // ao reconectar, nao herda a leitura antiga
        return;
    };
    let entry = &mut cache[1];
    if entry.0.is_none_or(|t| Instant::now() >= t) {
        let r = ctx.open(c).and_then(|d| read_evision(&d, 0, 1000));
        entry.1 = r;
        // Sem resposta: tenta de novo em 30 s; com resposta, em 10 min.
        entry.0 = Some(Instant::now() + Duration::from_secs(if r.is_some() { 600 } else { 30 }));
    }
    let mut s = Status::new("atk_keyboard:1012", "ATK Z87", Kind::Keyboard);
    match entry.1 {
        Some((p, chg)) => {
            s.percent = p;
            s.charging = chg;
        }
        None => s.asleep = true,
    }
    drop(cache);
    ctx.add(s, vid, pid);
}
