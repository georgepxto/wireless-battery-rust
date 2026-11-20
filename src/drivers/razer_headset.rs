//! Headsets Razer BlackShark / Barracuda com protocolo proprio (nao o feature report de 90 bytes):
//! - BlackShark V3 / V3 Pro / V3 Pro for Xbox (0576/0577, 0579/057A, 0A4E/0A55), interface 5: output 02 de
//!   64 bytes, [62] = XOR 0..61, classe 21 = bateria -> [13] %, [14] carregando. O headset derruba o link
//!   2.4 GHz se for consultado a cada poucos segundos: no maximo a cada 5 min. (OpenRazer PR #2794)
//! - BlackShark V3 X (057D dongle, 057C cabo): feature 07, classe 07 cmd 80/84. (OpenRazer PR #2899)
//! - BlackShark V2 HyperSpeed (0565): frame MediaTek, GET 21 / 2A. (justik13, MIT)
//! - BlackShark V2 Pro 2020 (0528): feature FF na FF00:0001. (OpenRazer #1280, HaloBattery)
//! - BlackShark V2 Pro 2023 (0555): envelope "PA" com modo remoto. (OpenRazer PR #2862, HaloBattery)
//! - Barracuda X 2022 (0550 YS-Tech com CRC-16, 0552 Macronix): comandos Airoha GET 21 / 2A no modo remoto.
//!   (mehdibouchami / nightdevx / KhromotozzDevOut)
//!
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), razer_headset.cpp.

use super::{hex4, logitech_g435::crc16, sleep_ms, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const VID: u16 = 0x1532;
const MIN_INTERVAL: Duration = Duration::from_secs(300);

static CACHE: Mutex<Option<HashMap<u16, (Instant, Status)>>> = Mutex::new(None);

fn cached(pid: u16) -> Option<Status> {
    let g = CACHE.lock().unwrap();
    g.as_ref()?.get(&pid).filter(|(t, _)| t.elapsed() < MIN_INTERVAL).map(|(_, s)| s.clone())
}

fn remember(pid: u16, s: &Status) {
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(pid, (Instant::now(), s.clone()));
}

fn headset(pid: u16, name: &str) -> Status {
    Status::new(format!("blackshark:{}", hex4(pid)), name, Kind::Headset)
}

// ---- BlackShark V3 / V3 Pro ------------------------------------------------------------------------

fn packet(dir: u8, cls: u8) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0] = 0x02;
    b[2] = 0x60;
    b[6] = 0x04;
    b[9] = dir;
    b[10] = cls;
    b[62] = b[..62].iter().fold(0, |a, x| a ^ x);
    b
}

fn v3_get(d: &HidDevice, cls: u8) -> Option<Vec<u8>> {
    d.write(&packet(0x80, cls)).ok()?;
    let end = Deadline::in_ms(600);
    while !end.expired() {
        let r = d.read(end.left()).filter(|r| r.len() >= 15)?;
        if r[0] == 0x02 && r[2] == 0x60 && r[10] == cls {
            return Some(r);
        }
    }
    None
}

fn poll_v3(ctx: &mut Ctx) {
    for pid in [0x0576u16, 0x0577, 0x0579, 0x057A, 0x0A4E, 0x0A55] {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let colls = ctx.find(VID, pid, |c| c.interface_number() == 5 && c.output_len() >= 64 && c.input_len() >= 15);
        let Some(c) = colls.first() else { continue };
        if let Some(s) = cached(pid) {
            ctx.add(s, VID, pid);
            continue;
        }
        let Some(d) = ctx.open(c) else { continue };
        // Handshake que o headset espera antes das leituras.
        let _ = d.write(&packet(0x00, 0x02));
        let _ = d.write(&packet(0x00, 0x2A));
        let _ = d.write(&packet(0x80, 0x2A));
        sleep_ms(250);
        d.flush();
        let Some(r) = v3_get(&d, 0x21).filter(|r| r[13] <= 100) else { continue };
        let cable_charging = v3_get(&d, 0x2A).is_some_and(|c| c[13] == 1);
        let name = match pid {
            0x0A4E | 0x0A55 => "Razer BlackShark V3 Pro for Xbox",
            0x0576 | 0x0577 => "Razer BlackShark V3 Pro",
            _ => "Razer BlackShark V3",
        };
        let mut s = headset(pid, name);
        s.percent = r[13] as i32;
        s.charging = r[14] != 0 || cable_charging;
        remember(pid, &s);
        ctx.add(s, VID, pid);
    }
}

// ---- BlackShark V3 X ----------------------------------------------------------------------------

static V3X_TXID: AtomicU8 = AtomicU8::new(0x40);

fn v3x_get(d: &HidDevice, cmd: u8) -> Option<u8> {
    for _ in 0..4 {
        let tx = V3X_TXID.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let mut req = vec![0u8; 64];
        req[0] = 0x07;
        req[2] = tx;
        req[6] = 0x02;
        req[7] = 0x07;
        req[8] = cmd;
        req[62] = req[3..=60].iter().fold(0, |a, x| a ^ x);
        d.send_feature_report(&req).ok()?;
        sleep_ms(60);
        let Some(r) = d.get_feature(0x07).filter(|r| r.len() >= 63) else { continue };
        if r[2] != tx || r[7] != 0x07 || r[8] != cmd || r[6] < 2 {
            continue;
        }
        if r[1] != 0x02 && r[1] != 0x01 {
            return None;
        }
        return Some(r[10]);
    }
    None
}

fn poll_v3x(ctx: &mut Ctx) {
    for pid in [0x057Du16, 0x057C] {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let colls = ctx.find(VID, pid, |c| c.interface_number() == 0 && c.feature_len() >= 64);
        let Some(c) = colls.first() else { continue };
        if let Some(s) = cached(pid) {
            ctx.add(s, VID, pid);
            continue;
        }
        let Some(d) = ctx.open_feature_only(c) else { continue };
        let Some(level) = v3x_get(&d, 0x80).filter(|l| *l <= 100) else { continue };
        let mut s = headset(pid, "Razer BlackShark V3 X");
        s.percent = level as i32;
        s.charging = v3x_get(&d, 0x84) == Some(1);
        remember(pid, &s);
        ctx.add(s, VID, pid);
    }
}

// ---- BlackShark V2 HyperSpeed -------------------------------------------------------------------

static HS_SEQ: AtomicU8 = AtomicU8::new(0);

fn hs_query(d: &HidDevice, cmd: u8) -> Option<u8> {
    let seq = 0x60 | (HS_SEQ.fetch_add(1, Ordering::Relaxed) & 0x1F);
    let mut b = vec![0u8; 64];
    b[0] = 0x02;
    b[2] = seq;
    b[6] = 0x04;
    b[9] = 0x80;
    b[10] = cmd;
    b[62] = b[..62].iter().fold(0, |a, x| a ^ x);
    d.flush();
    d.write(&b).ok()?;
    let end = Deadline::in_ms(650);
    while !end.expired() {
        let r = d.read(end.left())?;
        if r.len() < 64 || r[0] != 0x02 || r[2] != seq || r[10] != cmd || r[12] < 1 {
            continue;
        }
        if r[1..62].iter().fold(0, |a, x| a ^ x) != r[62] {
            continue;
        }
        return Some(r[13]);
    }
    None
}

fn poll_v2_hyperspeed(ctx: &mut Ctx) {
    let pid = 0x0565;
    if ctx.is_claimed(VID, pid) {
        return;
    }
    let colls = ctx.find(VID, pid, |c| matches!(c.usage_page(), 0xFF14 | 0xFF00) && c.usage() == 1 && c.output_len() >= 64 && c.input_len() >= 64);
    // A colecao FF14 primeiro, para nao mexer nas de audio.
    for page in [0xFF14u16, 0xFF00] {
        for c in colls.iter().filter(|c| c.usage_page() == page) {
            let Some(d) = ctx.open(c) else { continue };
            let Some(level) = hs_query(&d, 0x21).filter(|l| *l <= 100) else { continue };
            let mut s = headset(pid, "Razer BlackShark V2 HyperSpeed");
            s.percent = level as i32;
            s.charging = hs_query(&d, 0x2A).is_some_and(|c| c > 0);
            ctx.add(s, VID, pid);
            return;
        }
    }
}

// ---- BlackShark V2 Pro 2020 ---------------------------------------------------------------------

fn poll_v2_pro_2020(ctx: &mut Ctx) {
    let pid = 0x0528;
    const REPLY: [u8; 9] = [0xFF, 0x0F, 0x05, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05];
    const STALE: [u8; 9] = [0xFF, 0x01, 0x00, 0xFE, 0x12, 0x04, 0x1F, 0x08, 0x05];
    if ctx.is_claimed(VID, pid) {
        return;
    }
    for c in ctx.find(VID, pid, |c| c.interface_number() == 3 && c.usage_page() == 0xFF00 && c.usage() == 1 && c.feature_len() == 64) {
        let Some(d) = ctx.open_feature_only(&c) else { continue };
        if d.send_feature_report(&[0xFF, 0x0A, 0x00, 0xFD, 0x04, 0x12, 0xF1, 0x02, 0x05]).is_err() {
            continue;
        }
        for read in 0..5 {
            sleep_ms(100); // o Synapse espera ~100 ms o receptor buscar o estado do headset
            let Some(r) = d.get_feature(0xFF).filter(|r| r.len() == 64) else { continue };
            let fresh = r.starts_with(&REPLY);
            let offline = read == 4 && r.starts_with(&STALE);
            if !fresh && !offline {
                continue;
            }
            let mut s = headset(pid, "Razer BlackShark V2 Pro (2020)");
            let mv = if fresh { u16::from_be_bytes([r[12], r[13]]) } else { 0 };
            s.connected = mv != 0;
            if s.connected {
                s.charging = r[11] == 0x09;
                // Carregando, o receptor pode pular para 80% mesmo vindo de 10%.
                if !s.charging && r[14] <= 100 {
                    s.percent = r[14] as i32;
                }
            }
            ctx.add(s, VID, pid);
            return;
        }
    }
}

// ---- BlackShark V2 Pro 2023 ---------------------------------------------------------------------

fn pa_frame(total: u8, inner: u8, kind: u8, cmd: u8, flag: u8) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0] = 0x02;
    b[1] = 0x80;
    b[2] = total;
    b[5] = b'P';
    b[6] = b'A';
    b[7] = inner;
    b[9] = kind;
    b[10] = cmd;
    b[11] = flag;
    b
}

fn pa_remote(d: &HidDevice, on: bool) {
    let _ = d.write(&pa_frame(0x07, 0x0E, 0x02, 0xE1, on as u8));
    sleep_ms(35);
}

fn pa_query(d: &HidDevice, cmd: u8) -> Option<u8> {
    d.flush();
    pa_remote(d, true); // o primeiro so acorda um link cochilando
    let mut value = None;
    for _ in 0..3 {
        pa_remote(d, true);
        if d.write(&pa_frame(0x08, 0x08, 0x03, cmd, 0)).is_err() {
            break;
        }
        let end = Deadline::in_ms(150);
        while !end.expired() {
            let Some(r) = d.read(end.left()).filter(|r| r.len() >= 16) else { break };
            if r[0] == 0x02 && r[12] == cmd && r[13] == 0x01 && r[14] >= 1 {
                value = Some(r[15]);
                break;
            }
        }
        if value.is_some() {
            break;
        }
    }
    pa_remote(d, false);
    value
}

fn poll_v2_pro_2023(ctx: &mut Ctx) {
    let pid = 0x0555;
    if ctx.is_claimed(VID, pid) {
        return;
    }
    for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFF00 && c.output_len() >= 64 && c.input_len() >= 16) {
        let Some(d) = ctx.open(&c) else { continue };
        let Some(level) = pa_query(&d, 0x21).filter(|l| *l <= 100) else { continue };
        let mut s = headset(pid, "Razer BlackShark V2 Pro (2023)");
        s.percent = level as i32;
        s.charging = pa_query(&d, 0x2A).is_some_and(|c| c > 0);
        ctx.add(s, VID, pid);
        return;
    }
}

// ---- Barracuda X 2022 ---------------------------------------------------------------------------

struct Barracuda {
    d: HidDevice,
    macronix: bool,
    stream: Vec<u8>,
    seq: u8,
}

impl Barracuda {
    fn open(d: HidDevice, macronix: bool) -> Option<Self> {
        if macronix {
            // Receptor 0552: modo app primeiro, [01 40] -> [01 40 01 01].
            d.flush();
            d.write(&[0x01, 0x40]).ok()?;
            let end = Deadline::in_ms(500);
            loop {
                let r = d.read(end.left()).filter(|r| r.len() >= 4)?;
                if r[0] == 0x01 && r[1] == 0x40 {
                    if r[2] == 1 && r[3] == 1 {
                        break;
                    }
                    return None;
                }
                if end.expired() {
                    return None;
                }
            }
        }
        Some(Self { d, macronix, stream: Vec::new(), seq: 0 })
    }

    /// Comando Airoha [50 41][grupo][seq][corpo..].
    fn command(&mut self, group: u8, body: &[u8]) -> bool {
        self.seq = self.seq % 127 + 1;
        let mut payload = vec![0x50, 0x41, group, self.seq];
        payload.extend_from_slice(body);
        let mut f = vec![0u8; 64];
        if self.macronix {
            f[0] = 0x01;
            f[1] = 0x80;
            f[2] = payload.len() as u8;
            f[3..3 + payload.len()].copy_from_slice(&payload);
            return self.d.write(&f).is_ok();
        }
        f[0] = 0x02;
        f[1] = 0x80;
        f[2] = payload.len() as u8;
        f[5..5 + payload.len()].copy_from_slice(&payload);
        let crc = crc16(&f);
        f[3..5].copy_from_slice(&crc.to_le_bytes());
        for attempt in 0..5 {
            if attempt > 0 {
                sleep_ms(50);
            }
            if self.d.write(&f).is_err() {
                return false;
            }
            // A ponte responde 'O' (aceito) ou 'F' (ocupado).
            match self.d.get_input_report(0x02).filter(|s| s.len() >= 2).map(|s| s[1]) {
                Some(b'O') => return true,
                Some(b'F') => continue,
                _ => return false,
            }
        }
        false
    }

    fn await_frame(&mut self, ms: u64, accept: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        let end = Deadline::in_ms(ms);
        loop {
            while self.stream.len() >= 2 {
                let start = self.stream.windows(2).position(|w| w == [0x50, 0x49]).unwrap_or(self.stream.len());
                self.stream.drain(..start);
                if self.stream.len() < 10 {
                    break;
                }
                let len = 10 + u16::from_le_bytes([self.stream[8], self.stream[9]]) as usize;
                if self.stream.len() < len {
                    break;
                }
                let frame: Vec<u8> = self.stream.drain(..len).collect();
                if accept(&frame) {
                    return Some(frame);
                }
            }
            if end.expired() {
                return None;
            }
            let r = self.d.read(end.left()).filter(|r| r.len() >= 2)?;
            if self.macronix {
                if r.len() >= 3 && r[0] == 0x01 && r[1] == 0x80 && r[2] as usize + 3 <= r.len() {
                    self.stream.extend_from_slice(&r[3..3 + r[2] as usize]);
                }
            } else if r[0] == 0x02 && r[1] as usize + 2 <= r.len() {
                self.stream.extend_from_slice(&r[2..2 + r[1] as usize]);
            }
        }
    }

    fn remote_mode(&mut self, on: bool) -> bool {
        if !self.command(0x0E, &[0x02, 0xE1, on as u8]) {
            return false;
        }
        let seq = self.seq;
        self.await_frame(800, |r| r.len() >= 13 && r[10] == 0x0E && r[11] == 0x80 | seq).is_some_and(|f| f[12] == 0)
    }

    fn get(&mut self, cmd: u8) -> Option<u8> {
        if !self.command(0x08, &[0x03, cmd, 0x00, 0x00]) {
            return None;
        }
        let f = self.await_frame(1000, |r| r.len() >= 13 && r[2] == 0x08 && r[10] == cmd && r[11] == 0x01)?;
        (f[12] >= 1 && f.len() >= 14).then(|| f[13])
    }
}

fn poll_barracuda(ctx: &mut Ctx) {
    for pid in [0x0550u16, 0x0552] {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let macronix = pid == 0x0552;
        let usage = if macronix { 0 } else { 1 };
        for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFF00 && c.usage() == usage && c.output_len() >= 64 && c.input_len() >= 64) {
            let Some(mut link) = ctx.open(&c).and_then(|d| Barracuda::open(d, macronix)) else { continue };
            if !link.remote_mode(true) {
                continue; // receptor sem headset (desligado, longe ou no Bluetooth)
            }
            let level = link.get(0x21);
            let cable = if level.is_some() { link.get(0x2A) } else { None };
            link.remote_mode(false);
            let Some(level) = level.filter(|l| *l <= 100) else { continue };
            let mut s = Status::new(format!("barracuda:{}", hex4(pid)), "Razer Barracuda X (2022)", Kind::Headset);
            s.percent = level as i32;
            s.charging = cable == Some(1);
            ctx.add(s, VID, pid);
            break;
        }
    }
}

pub fn poll(ctx: &mut Ctx) {
    poll_v2_hyperspeed(ctx);
    poll_barracuda(ctx);
    poll_v2_pro_2020(ctx);
    poll_v2_pro_2023(ctx);
    poll_v3x(ctx);
    poll_v3(ctx);
}
