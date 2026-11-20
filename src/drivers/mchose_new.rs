//! Linhas novas da MCHOSE, cada uma com o seu protocolo:
//! - Headset V9 Pro 2 (dongle 3837:6011), protocolo "MC" do M HUB na colecao FF07:0222, report 0x55.
//! - Mouses "novos" Nordic (K7 V2 Ultra+ no receptor 3837:1014 ou no cabo 4028, e os que o M HUB
//!   agrupa com ele) e o teclado K87 V3 (receptor 3837:1015): frame 0x4D "device info" (cmd 0x0900).
//! - Teclados G75 V2 / G87 V2 / K99 V3 no receptor comum 3837:3033: frames "GLW", leitura do "func
//!   config" (bateria no byte 32 do payload).
//! - Mouses A5 Pro / A5 Pro Max da primeira geracao (VID 2023): feature report de 64 bytes, cmd 0x83.
//!
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0): mchose_headset.cpp,
//! mchose_modern.cpp, mchose_keyboard.cpp e mchose_a5.cpp, onde estao as fontes de cada protocolo
//! (o driver web do proprio M HUB, HifumiDaisuki01/MchoseBatteryTray, z750sasr/mchose-a5-pro-max-web-driver,
//! HeyOkay/HaloBattery e Klegus/mchose-macos). Os teclados sao consultados com intervalo longo para
//! nao acordar um teclado ocioso.

use super::{clamp100, hex4, sleep_ms, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::{DeviceInfo, HidDevice};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const VID: u16 = 0x3837;

fn strip_24g(name: &str) -> String {
    name.strip_suffix(" 2.4G").unwrap_or(name).to_string()
}

// ---- headset V9 Pro 2 ---------------------------------------------------------------------------

enum Reply {
    Off,
    Level(i32, bool),
}

/// Resposta de bateria (subcomando 0x0B) ou notificacao (0x22): body[0] = 0 desligado/dormindo,
/// body[1] = carregando (bit 7) | % (bits 0..6).
pub fn parse_mc(r: &[u8]) -> Option<(bool, i32, bool)> {
    if r.len() < 9 || r[0] != 0x55 {
        return None;
    }
    let f = &r[1..];
    if f[0] != 0xAA || f[1] != 0x01 || f[4] != 0x00 {
        return None;
    }
    let reply = f[5] == 0x01 && f[6] == 0x0B;
    let notify = f[5] == 0x04 && f[6] == 0x22;
    if !reply && !notify {
        return None;
    }
    let len = f[3] as usize;
    if len < 3 || 6 + len >= f.len() {
        return None;
    }
    let check = if f[2] == 1 { f[2] ^ f[3] ^ f[4] ^ f[5] ^ f[6..6 + len].iter().fold(0, |a, b| a ^ b) } else { 0 };
    if f[6 + len] != check {
        return None;
    }
    let body = &f[7..];
    if body[0] == 0 {
        return Some((false, -1, false));
    }
    let pct = (body[1] & 0x7F) as i32;
    (pct <= 100).then_some((true, pct, body[1] & 0x80 != 0))
}

fn query_v9pro2(d: &HidDevice) -> Option<Reply> {
    d.flush();
    d.write(&[0x55, 0xAA, 0x01, 0x00, 0x01, 0x00, 0x01, 0x0B, 0x00]).ok()?;
    let end = Deadline::in_ms(1000);
    while !end.expired() {
        let r = d.read(end.left())?;
        if let Some((on, pct, chg)) = parse_mc(&r) {
            return Some(if on { Reply::Level(pct, chg) } else { Reply::Off });
        }
    }
    None
}

pub fn poll_headset(ctx: &mut Ctx) {
    let pid = 0x6011;
    if ctx.is_claimed(VID, pid) {
        return;
    }
    for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFF07 && c.usage() == 0x0222 && c.output_len() >= 64 && c.input_len() >= 64) {
        let Some(reply) = ctx.open(&c).and_then(|d| query_v9pro2(&d)) else { continue };
        let name = c.product_string().filter(|n| !n.is_empty()).unwrap_or("MCHOSE V9 Pro 2");
        let mut s = Status::new("mchose:v9pro2", name, Kind::Headset);
        match reply {
            Reply::Off => s.asleep = true, // dongle respondeu, headset desligado: mantem o ultimo nivel
            Reply::Level(p, chg) => {
                s.percent = p;
                s.charging = chg;
            }
        }
        ctx.add(s, VID, pid);
        break;
    }
}

// ---- mouses Nordic e teclado K87 V3 (frame 0x4D) ------------------------------------------------

/// (pid, teclado)
const MODERN: &[(u16, bool)] = &[(0x1014, false), (0x1018, false), (0x1016, false), (0x102C, false), (0x102D, false), (0x4028, false), (0x1015, true)];

fn xor(b: &[u8], from: usize, to: usize) -> u8 {
    b.iter().take(to + 1).skip(from).fold(0, |a, x| a ^ x)
}

/// 1 = leitura, 0 = mouse nao ligado ao receptor, -1 = outra coisa.
pub fn parse_info(r: &[u8], keyboard: bool) -> (i32, i32, bool) {
    const D: usize = 8;
    if r.len() < D + 2 || r[0] != 0x4D || u16::from_le_bytes([r[4], r[5]]) != 0x0900 {
        return (-1, 0, false);
    }
    let len = r[3] as usize;
    if D + len >= r.len() {
        return (-1, 0, false);
    }
    // Mouse desligado: so o cabecalho, um 0xFF no lugar do modelo e dados zerados.
    if len < 14 {
        return if !keyboard || (len == 1 && r[D] == 0xFF) { (0, 0, false) } else { (-1, 0, false) };
    }
    if r[D + len] != xor(r, 2, 7 + len) || u16::from_le_bytes([r[D], r[D + 1]]) != VID {
        return (-1, 0, false);
    }
    if keyboard && (len < 20 || r[D + 2] != 0x23 || r[D + 3] != 0x20) {
        return (-1, 0, false);
    }
    if r[D + 10] == 0 {
        return (0, 0, false);
    }
    if r[D + 12] > 100 {
        return (-1, 0, false);
    }
    (1, r[D + 12] as i32, r[D + 11] == 1)
}

fn query_info(d: &HidDevice, c: &DeviceInfo, keyboard: bool) -> (i32, i32, bool) {
    let deadline = Deadline::in_ms(1000);
    for attempt in 0..3 {
        if attempt > 0 {
            if deadline.left() <= 150 {
                break;
            }
            sleep_ms(150);
        }
        let mut req = vec![0u8; c.output_len().max(9) as usize];
        req[0] = 0x4D;
        req[1] = 0x01;
        req[2] = 0x01;
        req[4] = 0x00;
        req[5] = 0x09;
        req[8] = xor(&req, 2, 7);
        if d.write(&req).is_err() {
            continue;
        }
        let end = Deadline::in_ms(250);
        while !end.expired() && !deadline.expired() {
            let Some(r) = d.read(end.left().min(deadline.left())) else { break };
            let got = parse_info(&r, keyboard);
            if got.0 >= 0 {
                return got;
            }
        }
    }
    (-1, 0, false)
}

/// Ultima leitura do teclado e quando pode perguntar de novo (a cada 3 min).
static KEYBOARDS: Mutex<Option<HashMap<String, (Instant, Status)>>> = Mutex::new(None);

pub fn poll_modern(ctx: &mut Ctx) {
    let mut guard = KEYBOARDS.lock().unwrap();
    let keyboards = guard.get_or_insert_with(HashMap::new);
    let present: Vec<String> = ctx.find(VID, 0x1015, |_| true).iter().map(|c| c.path().to_string_lossy().into_owned()).collect();
    keyboards.retain(|k, _| present.contains(k));
    for &(pid, keyboard) in MODERN {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let colls = ctx.find(VID, pid, |c| {
            if keyboard {
                c.usage_page() == 0xFF80 && c.usage() == 1 && c.output_len() == 64 && c.input_len() == 64
            } else {
                matches!(c.usage_page(), 0xFF01 | 0xFF0B) && c.output_len() >= 64 && c.input_len() >= 21
            }
        });
        for c in colls {
            let path = c.path().to_string_lossy().into_owned();
            if keyboard {
                if let Some((next, s)) = keyboards.get(&path) {
                    if Instant::now() < *next {
                        ctx.add(s.clone(), VID, pid);
                        break;
                    }
                }
            }
            let Some(d) = ctx.open(&c) else { continue };
            d.flush();
            let (state, pct, chg) = query_info(&d, &c, keyboard);
            if state < 0 && !keyboard {
                continue;
            }
            let default = if keyboard { "MCHOSE K87 V3" } else { "MCHOSE mouse" };
            let name = strip_24g(c.product_string().filter(|n| !n.is_empty()).unwrap_or(default));
            let mut s = Status::new(format!("mchose:{}:{}", hex4(VID), hex4(pid)), name, if keyboard { Kind::Keyboard } else { Kind::Mouse });
            if state == 1 {
                s.percent = clamp100(pct);
                s.charging = chg;
            } else {
                s.asleep = true;
            }
            if keyboard {
                keyboards.insert(path, (Instant::now() + Duration::from_secs(180), s.clone()));
            }
            ctx.add(s, VID, pid);
            break;
        }
    }
}

// ---- teclados GLW no receptor 3837:3033 ---------------------------------------------------------

pub fn parse_func_config(r: &[u8]) -> Option<(i32, bool)> {
    const P: usize = 8;
    const BAT: usize = 32;
    if r.len() < P + BAT + 1 || r[0] != 0xAA || r[1] != 0x05 || r[5] != 0 || r[6] != 0 {
        return None;
    }
    let size = r[4] as usize;
    if size < BAT + 1 || P + size > r.len() {
        return None;
    }
    let sum = r[4..8].iter().chain(&r[P..P + size]).fold(0u32, |a, b| a + *b as u32);
    if (sum & 0xFF) as u8 != r[3] {
        return None;
    }
    let pct = (r[P + BAT] & 0x7F) as i32;
    (pct <= 100).then_some((pct, r[P + BAT] & 0x80 != 0))
}

fn query_glw(d: &HidDevice) -> Option<(i32, bool)> {
    d.flush();
    for attempt in 0..2 {
        if attempt > 0 {
            sleep_ms(150);
        }
        // Leitura de 56 bytes no offset 0: [report id 0] flag, comando, 00, checksum, tamanho, offset.
        if d.write(&[0x00, 0x55, 0x05, 0x00, 56, 56, 0x00, 0x00, 0x00]).is_err() {
            continue;
        }
        let end = Deadline::in_ms(500);
        while !end.expired() {
            let Some(r) = d.read(end.left()) else { break };
            if let Some(v) = parse_func_config(&r) {
                return Some(v);
            }
        }
    }
    None
}

/// (quando perguntou, ultima leitura): o receptor e consultado no maximo a cada 10 min.
type Level = Option<(i32, bool)>;

static GLW: Mutex<(Option<Instant>, Level)> = Mutex::new((None, None));

pub fn poll_keyboard(ctx: &mut Ctx) {
    let pid = 0x3033;
    if ctx.is_claimed(VID, pid) {
        return;
    }
    let cs = ctx.find(VID, pid, |c| {
        c.input_len() == 65 && c.output_len() == 65 && ((c.usage_page() == 0x0001 && c.usage() == 0) || (c.usage_page() == 0xFF31 && c.usage() == 0x0074))
    });
    let Some(first) = cs.first() else { return };
    let mut g = GLW.lock().unwrap();
    let ask = g.0.is_none_or(|t| t.elapsed() >= Duration::from_secs(600));
    let mut reading = None;
    if ask {
        g.0 = Some(Instant::now());
        reading = cs.iter().find_map(|c| ctx.open(c).and_then(|d| query_glw(&d)));
    }
    if reading.is_none() && !ask {
        reading = g.1;
    }
    if reading.is_none() && g.1.is_none() {
        return; // nunca respondeu: receptor QHW (K87S) ou link morto
    }
    if reading.is_some() {
        g.1 = reading;
    }
    let mut s = Status::new("mchose:3837:3033", strip_24g(first.product_string().filter(|n| !n.is_empty()).unwrap_or("MCHOSE keyboard")), Kind::Keyboard);
    match reading {
        Some((p, chg)) => {
            s.percent = clamp100(p);
            s.charging = chg;
        }
        None => s.asleep = true,
    }
    ctx.add(s, VID, pid);
}

// ---- A5 Pro / A5 Pro Max (VID 2023) -------------------------------------------------------------

const A5: &[(u16, &str, bool)] = &[
    (0xF013, "MCHOSE A5 Pro / Pro Max", false),
    (0xF015, "MCHOSE A5 Pro / Pro Max", false),
    (0xF017, "MCHOSE A5 Pro", true),
    (0xF018, "MCHOSE A5 Pro Max", false),
    (0xF019, "MCHOSE A5 Pro Max", true),
];

/// r[0] e o report id no Windows; o marcador A1 vem em seguida.
pub fn parse_a5(r: &[u8]) -> Option<(i32, bool)> {
    if r.len() < 9 || (r[0] != 0xA1 && r[1] != 0xA1) {
        return None;
    }
    let at = if r[1] == 0xA1 { 1 } else { 0 };
    if r[at + 4] != 0x00 || r[at + 5] != 0x83 || r[at + 7] == 0xFF {
        return None;
    }
    Some(((r[at + 7] as i32).min(100), r[at + 6] != 0))
}

/// 1 = leu, 0 = colecao funciona mas o mouse nao respondeu, -1 = nao deu para falar.
fn query_a5(ctx: &Ctx, c: &DeviceInfo) -> (i32, i32, bool) {
    let Some(d) = ctx.open_feature_only(c) else { return (-1, 0, false) };
    let mut req = vec![0u8; c.feature_len() as usize];
    req[3] = 0x02; // rota: mouse
    req[4] = 2;
    req[6] = 0x83;
    if d.send_feature_report(&req).is_err() {
        return (-1, 0, false);
    }
    sleep_ms(34);
    for attempt in 0..8 {
        if attempt == 3 {
            if d.send_feature_report(&req).is_err() {
                return (-1, 0, false);
            }
            sleep_ms(34);
        }
        if let Some((l, chg)) = d.get_feature(0).and_then(|r| parse_a5(&r)) {
            return (1, l, chg);
        }
        sleep_ms(30);
    }
    (0, 0, false)
}

static A5_ANSWERED: Mutex<Vec<u16>> = Mutex::new(Vec::new());

pub fn poll_a5(ctx: &mut Ctx) {
    for &(pid, name, cable) in A5 {
        if ctx.is_claimed(0x2023, pid) {
            continue;
        }
        let mut colls = ctx.find(0x2023, pid, |c| c.usage_page() == 0xFFFF && c.feature_len() >= 65);
        colls.sort_by_key(|c| c.interface_number() != 2); // interface 2 primeiro
        let (mut state, mut level, mut chg) = (-1, 0, false);
        for c in &colls {
            let (st, l, ch) = query_a5(ctx, c);
            if st > state {
                (state, level, chg) = (st, l, ch);
            }
            if st == 1 {
                break;
            }
        }
        if state < 0 {
            continue;
        }
        let mut answered = A5_ANSWERED.lock().unwrap();
        let mut s = Status::new(format!("mchose-a5:{}", hex4(pid)), name, Kind::Mouse);
        if state == 1 {
            if !answered.contains(&pid) {
                answered.push(pid);
            }
            s.percent = level;
            s.charging = chg;
        } else {
            if cable || !answered.contains(&pid) {
                continue;
            }
            s.asleep = true;
        }
        drop(answered);
        ctx.add(s, 0x2023, pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_battery_reply() {
        // Resposta com checksum XOR: flags 1, tamanho 3 (subcomando + 2 bytes de corpo).
        let mut f = vec![0x55, 0xAA, 0x01, 0x01, 0x03, 0x00, 0x01, 0x0B, 0x01, 0x80 | 64];
        let chk = f[3] ^ f[4] ^ f[5] ^ f[6] ^ f[7] ^ f[8] ^ f[9];
        f.push(chk);
        f.resize(64, 0);
        assert_eq!(parse_mc(&f), Some((true, 64, true)));
        let mut off = f.clone();
        off[8] = 0;
        off[10] = off[3] ^ off[4] ^ off[5] ^ off[6] ^ off[7] ^ off[8] ^ off[9];
        assert_eq!(parse_mc(&off), Some((false, -1, false)));
    }

    #[test]
    fn modern_info() {
        let mut r = vec![0u8; 64];
        r[..8].copy_from_slice(&[0x4D, 0x01, 0x01, 14, 0x00, 0x09, 0x00, 0x00]);
        r[8..10].copy_from_slice(&0x3837u16.to_le_bytes());
        r[18] = 2; // ligado ao receptor
        r[19] = 0;
        r[20] = 29;
        r[22] = xor(&r, 2, 21);
        assert_eq!(parse_info(&r, false), (1, 29, false));
        let asleep = [0x4D, 0x01, 0x01, 0x01, 0x00, 0x09, 0x00, 0x00, 0xFF, 0x00];
        assert_eq!(parse_info(&asleep, false).0, 0);
    }

    #[test]
    fn a5_reply() {
        assert_eq!(parse_a5(&[0x00, 0xA1, 0, 0, 0, 0x00, 0x83, 1, 77]), Some((77, true)));
        assert_eq!(parse_a5(&[0x00, 0xA1, 0, 0, 0, 0x00, 0x83, 0, 0xFF]), None);
    }
}
