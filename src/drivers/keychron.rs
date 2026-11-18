//! Mouses Keychron (VID 3434) e o trackball Nape Pro.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), keychron.cpp.
//!
//! Do Launcher web da Keychron: output report de 64 bytes sem id [01][00][81][01][0...][checksum],
//! checksum (byte 63) = 0xA1 - soma(bytes 0..62). A resposta classica traz o VID do mouse.
//! Mouses 8K (TurboLink): mesmo pedido com o bit 6 do primeiro byte (41 00 81 01).
//! Mouses 4K (4K Link, colecao FF0A:0001): protocolo "4k"; 40 00 81 00 devolve o PID do mouse.
//! Nape Pro: colecao raw HID FF60:0061 do receptor Link-KM (3434:D026) ou do cabo (0440);
//! B2 lista os slots e A7 31 e encaminhado ao trackball. Fontes: nvalis/nape_pro,
//! gtun/KeychronNapeBatteryWidget, ky0209/NapeBar.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

fn set_checksum(p: &mut [u8; 64]) {
    let sum = p[..63].iter().fold(0u8, |a, x| a.wrapping_add(*x));
    p[63] = 0xA1u8.wrapping_sub(sum);
}

fn clean_name(n: &str) -> String {
    if n.is_empty() || n.contains("Link") || n.contains("Receiver") {
        "Keychron mouse".into()
    } else {
        n.into()
    }
}

/// Envia [cmd][00][81][sub] e espera ate 600 ms uma resposta que comece com cmd e tenha sub no byte 3.
fn query(d: &HidDevice, cmd: u8, sub: u8) -> Option<Vec<u8>> {
    let mut p = [0u8; 64];
    p[0] = cmd;
    p[2] = 0x81;
    p[3] = sub;
    set_checksum(&mut p);
    let mut req = vec![0u8]; // report id 0
    req.extend_from_slice(&p);
    d.flush();
    d.write(&req).ok()?;
    let end = Deadline::in_ms(600);
    while !end.expired() {
        let r = d.read(end.left())?;
        if r.len() < 13 {
            return None;
        }
        if r[0] == cmd && r[3] == sub {
            return Some(r);
        }
    }
    None
}

const LINK_KM: u16 = 0xD026;
const NAPE_USB: u16 = 0x0440;
const NAPE_IN_SLOT: u16 = 0x4004;

fn raw_query(d: &HidDevice, cmd: &[u8]) -> Option<Vec<u8>> {
    let mut req = vec![0u8];
    req.extend_from_slice(cmd);
    req.resize(33, 0);
    d.flush();
    d.write(&req).ok()?;
    let end = Deadline::in_ms(600);
    while !end.expired() {
        let r = d.read(end.left())?;
        if r.len() >= cmd.len() + 2 && r.starts_with(cmd) {
            return Some(r);
        }
    }
    None
}

fn poll_nape(ctx: &mut Ctx) {
    for pid in [LINK_KM, NAPE_USB] {
        for c in ctx.find(0x3434, pid, |c| c.usage_page() == 0xFF60 && c.usage() == 0x61 && c.output_len() == 33) {
            let Some(d) = ctx.open(&c) else { continue };
            if pid == LINK_KM {
                // So pergunta quando um Nape Pro esta num slot e conectado.
                let Some(r) = raw_query(&d, &[0xB2]).filter(|r| r.len() >= 17) else { continue };
                let mut connected = false;
                for off in [2usize, 7, 12] {
                    if u16::from_be_bytes([r[off + 2], r[off + 3]]) == NAPE_IN_SLOT {
                        connected = r[off + 4] == 1;
                    }
                }
                if !connected {
                    continue;
                }
            }
            let Some(r) = raw_query(&d, &[0xA7, 0x31]).filter(|r| r[2] <= 100) else { continue };
            let mut s = Status::new("keychron:nape", "Keychron Nape Pro", Kind::Mouse);
            s.percent = r[2] as i32;
            s.charging = r[3] != 0;
            ctx.add(s, 0x3434, pid);
            return;
        }
    }
}

fn name_4k(product_id: u16) -> Option<&'static str> {
    Some(match product_id {
        0x0620 | 0x0622 => "Keychron M3 mini 4K",
        0x0621 => "Keychron M4 4K",
        0x0624 => "Keychron M6 4K",
        0x07A0 => "Keychron M3 4K",
        _ => return None,
    })
}

pub fn poll(ctx: &mut Ctx) {
    poll_nape(ctx);
    for (vid, pid) in ctx.products(0x3434) {
        for c in ctx.find(vid, pid, |c| c.usage_page() >= 0xFF00 && c.output_len() == 65 && c.input_len() == 65) {
            let Some(d) = ctx.open(&c) else { continue };
            let r1 = query(&d, 0x01, 0x01);
            let tag = r1.as_ref().map_or(0, |r| u16::from_le_bytes([r[6], r[7]]));
            if r1.is_some() && c.usage_page() == 0xFF0A && c.usage() == 0x01 && tag != 0x3434 && tag != 0x362D {
                // Mouse 4K no receptor 4K Link.
                let Some(r) = query(&d, 0x41, 0x01).filter(|r| (1..=100).contains(&r[6])) else { continue };
                let mut name = clean_name(c.product_string().unwrap_or(""));
                if let Some(st) = query(&d, 0x40, 0x00) {
                    if let Some(n) = name_4k(u16::from_le_bytes([st[4], st[5]])) {
                        name = n.into();
                    }
                }
                let mut s = Status::new(format!("keychron:{}", hex4(pid)), name, Kind::Mouse);
                s.percent = r[6] as i32;
                s.charging = r[5] != 0;
                ctx.add(s, vid, pid);
                break;
            }
            let mut found = None;
            if let Some(r1) = r1.as_ref().filter(|r| tag == 0x3434 && r[11] <= 100) {
                found = Some((r1[11], r1[10] != 0)); // protocolo classico (M4 etc.)
            } else if let Some(r) = query(&d, 0x41, 0x01).filter(|r| r[10] <= 3 && r[11] <= 100) {
                found = Some((r[11], r[10] == 1)); // mouse 8K no receptor
            } else if let Some(r1) = r1.as_ref().filter(|r| r[10] <= 3 && (1..=100).contains(&r[11]) && (1..=8).contains(&r[5])) {
                found = Some((r1[11], r1[10] == 1)); // mouse 8K no cabo
            }
            let Some((pct, chg)) = found else { continue };
            let mut s = Status::new(format!("keychron:{}", hex4(pid)), clean_name(c.product_string().unwrap_or("")), Kind::Mouse);
            s.percent = pct as i32;
            s.charging = chg;
            ctx.add(s, vid, pid);
            break;
        }
    }
}
