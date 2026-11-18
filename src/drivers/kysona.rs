//! Kysona, Pulsar, ATK / VXE, G-Wolves (alguns), Hitscan Hyperlight, Pulsar XS-1, HECATE e mouses
//! Redragon com chip Compx: o mesmo protocolo.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), kysona.cpp.
//!
//! Report 08 de 17 bytes: [08][cmd][0 x14][checksum], checksum = 0x55 - soma(bytes 0..15).
//!   cmd 03 = online   -> resposta [6] = 1 quando o mouse esta ligado ao dongle
//!   cmd 04 = bateria  -> resposta [6] = porcentagem, [7] = carregando, [8..9] = milivolts
//! Referencias: driver hid-kysona do Linux (Lode Willems), S1berla/vxe-r1-pro-max-battery,
//! orcunturkol/BatteryTray, EliottV17/redragon-battery-omarchy.

use super::glorious::{gwolves_is_cable, gwolves_name};
use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

fn is_redragon(vid: u16, pid: u16) -> bool {
    vid == 0x3554 && matches!(pid, 0xF55D | 0xF5D5 | 0xF55E)
}

fn is_vxe_r1(vid: u16, pid: u16) -> bool {
    vid == 0x3554 && matches!(pid, 0xF58A | 0xF58C | 0xF58E | 0xF58F | 0xF505 | 0xF506)
}

fn is_cable(vid: u16, pid: u16) -> bool {
    (vid == 0x33E4 && gwolves_is_cable(pid))
        || (vid == 0x35BB && matches!(pid, 0xD3FA | 0xD3FC | 0xD3FD))
        || (vid == 0x3554 && matches!(pid, 0xF55E | 0xF58C | 0xF58F | 0xF506))
}

pub fn checksum(b: &[u8]) -> u8 {
    0x55u8.wrapping_sub(b[..16].iter().fold(0u8, |a, x| a.wrapping_add(*x)))
}

pub fn request(cmd: u8) -> [u8; 17] {
    let mut r = [0u8; 17];
    r[0] = 0x08;
    r[1] = cmd;
    r[16] = checksum(&r);
    r
}

/// Envia `cmd` e espera a resposta com checksum.
pub fn query(d: &HidDevice, cmd: u8) -> Option<Vec<u8>> {
    d.flush();
    d.write(&request(cmd)).ok()?;
    let end = Deadline::in_ms(500);
    loop {
        if end.expired() {
            return None;
        }
        let r = d.read(end.left())?;
        if r.len() < 17 {
            return None;
        }
        if r[0] == 0x08 && r[1] == cmd && r[16] == checksum(&r) {
            return Some(r);
        }
    }
}

fn clean_name(product: &str, vid: u16, pid: u16) -> String {
    if vid == 0x33E4 {
        return gwolves_name(pid).into();
    }
    if is_redragon(vid, pid) {
        return "Redragon wireless mouse".into();
    }
    if is_vxe_r1(vid, pid) {
        return "VXE Dragonfly R1".into();
    }
    let mut n = product.to_string();
    for suffix in [" Dongle", " Receiver", " 2.4G", " Wireless Receiver"] {
        if n.len() > suffix.len() && n.ends_with(suffix) {
            n.truncate(n.len() - suffix.len());
        }
    }
    if vid == 0x3770 {
        return "Hitscan Hyperlight".into();
    }
    if vid == 0x35BB {
        return "HECATE mouse".into();
    }
    if n.is_empty() {
        n = if vid == 0x373B { "ATK wireless mouse" } else { "Wireless mouse" }.into();
    }
    n
}

pub fn poll(ctx: &mut Ctx) {
    let mut seen: Vec<String> = Vec::new();
    for vendor in [0x3554u16, 0x373B, 0x3770, 0x33E4, 0x3710, 0x35BB] {
        for (vid, pid) in ctx.products(vendor) {
            // Headsets ATK N9 falam outro protocolo (porta serial): nunca mandar frames de mouse.
            if vid == 0x373B && matches!(pid, 0x11CA | 0x11CB | 0x1203 | 0x1204) {
                continue;
            }
            let mut colls = ctx.find(vid, pid, |c| {
                (c.output_len() == 17 && c.input_len() == 17) || (c.usage_page() >= 0xFF00 && c.output_len() >= 17 && c.input_len() >= 17)
            });
            // O canal de bateria e FF02:0002 onde existe: tenta antes das outras colecoes vendor.
            colls.sort_by_key(|c| !(c.usage_page() == 0xFF02 && c.usage() == 0x0002));
            for c in &colls {
                let Some(d) = ctx.open(c) else { continue };
                let mut s = Status::new(String::new(), String::new(), Kind::Mouse);
                let mut answered = false;
                if let Some(r) = query(&d, 0x03) {
                    answered = true;
                    s.connected = r[6] != 0;
                }
                if !answered || s.connected {
                    if let Some(r) = query(&d, 0x04) {
                        answered = true;
                        s.connected = true;
                        if r[6] <= 100 {
                            s.percent = r[6] as i32;
                        }
                        s.charging = r[7] != 0;
                    } else if answered && s.connected {
                        s.asleep = true; // receptor diz online, mas o mouse nao respondeu
                    }
                }
                if !answered {
                    continue;
                }
                if !s.connected {
                    ctx.claim(vid, pid);
                    break;
                }
                s.key = format!("kysona:{}:{}", hex4(vid), hex4(pid));
                s.name = clean_name(c.product_string().unwrap_or(""), vid, pid);
                s.kind = match ctx.kind_from_collections(vid, pid) {
                    Kind::Other => Kind::Mouse,
                    k => k,
                };
                let _ = is_cable(vid, pid);
                // Um mouse carregando pode aparecer duas vezes (cabo + dongle): fica um.
                if seen.contains(&s.name) {
                    ctx.claim(vid, pid);
                } else {
                    seen.push(s.name.clone());
                    ctx.add(s, vid, pid);
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_reference() {
        // 0x55 - (0x08 + 0x04) = 0x49
        assert_eq!(request(0x04)[16], 0x49);
        assert_eq!(request(0x03)[16], 0x4A);
    }
}
