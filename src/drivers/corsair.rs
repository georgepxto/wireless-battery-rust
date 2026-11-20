//! Headsets, mouses e teclados Corsair sem fio (VID 1B1C).
//! - Familia VOID (VOID, VOID PRO, VOID ELITE, HS70...): output C9 64 -> [2] % (bit 7 = microfone
//!   levantado), [4] estado (0 desligado, 1-2 ok, 4-5 carregando). (HeadsetControl)
//! - "Bragi" (Virtuoso, VOID V2, HS80 MAX, mouses e teclados Slipstream): 02 <alvo> 02 <prop> = GET;
//!   resposta [1] origem, [2] 02, [3] estado (0 ok), [4..7] valor LE. Prop 0F = bateria (0..1000),
//!   10 = carga (1 carregando, 3 cheio), 12 = product id do aparelho pareado. Alvo 08 e o proprio
//!   aparelho ou receptor, 09+ os pareados. So GET: nunca liga o "modo software".
//!
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), corsair.cpp. Os headsets Wireless V2
//! recebem antes o "acordar" leve (firmware e heartbeat do dongle, heartbeat do headset) descrito em
//! renanleonel/windows-taskbar-battery (MIT), corsair.rs.

use super::{clamp100, hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::{DeviceInfo, HidDevice};

const VID: u16 = 0x1B1C;
const VOID_PIDS: &[u16] = &[
    0x1B1C, 0x1B27, 0x0A14, 0x0A16, 0x0A17, 0x0A1D, 0x0A1A, 0x1B2A, 0x1B23, 0x1B29, 0x0A55, 0x0A51, 0x0A52, 0x0A38, 0x0A4F, 0x0A2B, 0x0A75, 0x0A56,
];
const BRAGI_PIDS: &[u16] = &[0x2A08, 0x2A02, 0x0A97, 0x0A64, 0x0A62, 0x0A3E, 0x0A3D, 0x2B00];
/// VOID WIRELESS V2, VIRTUOSO MAX, HS80 MAX.
const WIRELESS_V2: &[u16] = &[0x2A08, 0x2A02, 0x0A97];
/// Aparelhos atras de um receptor, pelo product id que informam (prop 12).
const PAIRED: &[(u32, &str, Kind)] = &[(0x1B7E, "Corsair Dark Core RGB Pro SE", Kind::Mouse)];

pub fn parse_void(r: &[u8]) -> Option<(i32, bool, bool)> {
    if r.len() < 5 {
        return None;
    }
    let st = r[4];
    let charging = st == 4 || st == 5;
    let percent = if st == 1 || st == 2 || charging { clamp100((r[2] & 0x7F) as i32) } else { -1 };
    Some((percent, charging, st != 0))
}

fn poll_void(ctx: &Ctx, c: &DeviceInfo) -> Option<Status> {
    let d = ctx.open(c)?;
    d.flush();
    d.write(&[0xC9, 0x64]).ok()?;
    let r = d.read(1000).filter(|r| r.len() >= 5)?;
    let (percent, charging, connected) = parse_void(&r)?;
    let name = c.product_string().filter(|n| !n.is_empty()).unwrap_or("Corsair wireless headset");
    let mut s = Status::new(format!("corsair:{}", hex4(c.product_id())), name, Kind::Headset);
    s.connected = connected;
    s.charging = charging;
    s.percent = percent;
    Some(s)
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Unknown,
    Prefixed,
    Plain,
}

struct Bragi {
    d: HidDevice,
    numbered: bool,
    mode: Mode,
}

impl Bragi {
    /// Duas molduras: "prefixada" (Bragi antigo) 02 <alvo> 02 <prop>, e "simples" (receptores
    /// Slipstream novos, ex.: 1B1C:2B00) 00 <alvo> 02 <prop>. A primeira que responde fica valendo.
    fn get(&mut self, target: u8, prop: u8) -> Option<u32> {
        if self.mode != Mode::Plain {
            if let Some(v) = self.get_framed(false, target, prop) {
                self.mode = Mode::Prefixed;
                return Some(v);
            }
        }
        if self.mode == Mode::Prefixed {
            return None;
        }
        let v = self.get_framed(true, target, prop)?;
        self.mode = Mode::Plain;
        Some(v)
    }

    fn get_framed(&self, plain: bool, target: u8, prop: u8) -> Option<u32> {
        let req: Vec<u8> = if plain {
            vec![0x00, target, 0x02, prop]
        } else if self.numbered {
            vec![0x02, target, 0x02, prop]
        } else {
            vec![0x00, 0x02, target, 0x02, prop]
        };
        self.d.flush();
        self.d.write(&req).ok()?;
        let source = if target == 0x08 { 0x00 } else { 0x01 };
        let o = if plain { 0 } else { 1 };
        let end = Deadline::in_ms(300);
        while !end.expired() {
            let r = self.d.read(end.left())?;
            if r.len() < o + 7 || r[o + 1] != 0x02 || r[o] != source {
                continue; // notificacao ou resposta para outro app
            }
            if r[o + 2] != 0 {
                return None;
            }
            return Some(u32::from_le_bytes([r[o + 3], r[o + 4], r[o + 5], r[o + 6]]));
        }
        None
    }

    /// Wireless V2: acorda o link sem trocar o headset para o modo software (que estala).
    fn wake(&self) {
        let send = |endpoint: u8, cmd: u8| {
            let req = if self.numbered { vec![0x02, endpoint, 0x02, cmd] } else { vec![0x00, 0x02, endpoint, 0x02, cmd] };
            let _ = self.d.write(&req);
            let _ = self.d.read(150);
        };
        self.d.flush();
        send(0x08, 0x13); // firmware do dongle
        send(0x08, 0x12); // heartbeat do dongle
        send(0x09, 0x12); // heartbeat do headset
    }
}

/// Mouses Corsair no protocolo CompX de 17 bytes (report 08, colecao FF02:0002): 03 = link,
/// 04 = bateria ([6] %, [7] carregando). (HaloBattery; krisgfx9 corsair.cpp)
const COMPX: &[(u16, &str)] = &[(0x2B2A, "Corsair Sabre V2 Pro Ultralight"), (0x2B34, "Corsair Sabre v2 PRO WIRELESS MG")];

fn compx_query(d: &HidDevice, cmd: u8) -> Option<Vec<u8>> {
    d.flush();
    d.write(&super::kysona::request(cmd)).ok()?;
    let end = Deadline::in_ms(500);
    while !end.expired() {
        let r = d.read(end.left()).filter(|r| r.len() >= 17)?;
        if r[0] == 0x08 && r[1] == cmd && r[16] == super::kysona::checksum(&r[..16]) {
            return Some(r);
        }
    }
    None
}

fn poll_compx(ctx: &mut Ctx, vid: u16, pid: u16, name: &str) {
    for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFF02 && c.usage() == 0x0002 && c.output_len() >= 17 && c.input_len() >= 17) {
        let Some(d) = ctx.open(&c) else { continue };
        let mut s = Status::new(format!("corsair:{}", hex4(pid)), name, Kind::Mouse);
        let mut answered = false;
        if let Some(r) = compx_query(&d, 0x03) {
            answered = true;
            s.connected = r[6] != 0;
        }
        if !answered || s.connected {
            if let Some(r) = compx_query(&d, 0x04) {
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
            continue;
        }
        if s.connected {
            ctx.add(s, vid, pid);
        } else {
            ctx.claim(vid, pid);
        }
        return;
    }
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(VID) {
        if let Some(&(_, name)) = COMPX.iter().find(|m| m.0 == pid) {
            poll_compx(ctx, vid, pid, name);
            continue;
        }
        if VOID_PIDS.contains(&pid) {
            for c in ctx.find(vid, pid, |c| (c.usage_page() == 0xFFC5 || c.usage_page() == 0xFF00) && c.output_len() > 0) {
                if let Some(s) = poll_void(ctx, &c) {
                    ctx.add(s, vid, pid);
                    break;
                }
            }
            continue;
        }
        let known = BRAGI_PIDS.contains(&pid);
        let colls = ctx.find(vid, pid, |c| {
            let sizes = matches!(c.output_len(), 64 | 65) && matches!(c.input_len(), 64 | 65);
            sizes && (c.usage_page() == 0xFF42 || (known && c.usage_page() >= 0xFF00) || (known && c.interface_number() == 4))
        });
        for c in colls {
            let Some(d) = ctx.open(&c) else { continue };
            let mut b = Bragi { d, numbered: c.output_len() == 64, mode: Mode::Unknown };
            if WIRELESS_V2.contains(&pid) {
                b.wake();
            }
            let product = c.product_string().unwrap_or("").to_string();
            let mut found = Vec::new();
            // Primeiro os pareados atras de um receptor, depois o proprio aparelho (cabo / Bluetooth).
            for target in [0x09u8, 0x0A, 0x0B, 0x08] {
                if target == 0x08 && !found.is_empty() {
                    break;
                }
                let mut level = b.get(target, 0x0F);
                for _ in 0..2 {
                    if level.is_some_and(|l| l == 0 || l > 1000) {
                        level = b.get(target, 0x0F);
                    }
                }
                let Some(level) = level.filter(|l| *l <= 1000) else { continue };
                let mut name = if product.is_empty() { "Corsair wireless device".to_string() } else { product.clone() };
                let mut kind = Kind::from_name(&name);
                if target != 0x08 {
                    if let Some(dev_pid) = b.get(target, 0x12) {
                        if let Some(m) = PAIRED.iter().find(|m| m.0 == dev_pid) {
                            name = m.1.into();
                            kind = m.2;
                        } else if dev_pid >> 8 == 0x0A {
                            kind = Kind::Headset;
                            if product.contains("Receiver") {
                                name = "Corsair wireless headset".into();
                            }
                        }
                    }
                }
                // Headsets Corsair e os receptores deles usam product ids 0Axx.
                if kind == Kind::Other && pid >> 8 == 0x0A {
                    kind = Kind::Headset;
                }
                if kind == Kind::Other {
                    kind = ctx.kind_from_collections(vid, pid);
                }
                let mut s = Status::new(format!("corsair:{}:{target}", hex4(pid)), name, kind);
                s.percent = clamp100((level as i32 + 5) / 10);
                s.charging = b.get(target, 0x10).is_some_and(|c| c == 1 || c == 3);
                found.push(s);
            }
            if !found.is_empty() {
                for s in found {
                    ctx.out.push(s.into_observation());
                }
                ctx.claim(vid, pid);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn void_status() {
        assert_eq!(parse_void(&[0x64, 0, 0x80 | 73, 0xB1, 1]), Some((73, false, true)));
        assert_eq!(parse_void(&[0x64, 0, 40, 0xB1, 5]), Some((40, true, true)));
        assert_eq!(parse_void(&[0x64, 0, 0, 0xB1, 0]), Some((-1, false, false)));
        assert_eq!(parse_void(&[0x64, 0, 40]), None);
    }
}
