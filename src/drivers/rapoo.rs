//! Mouses Rapoo VT: a bateria chega sozinha, sem comandos.
//! - Nordic 54L (Rapoo Hub): report 07 na FF00:0002, estado no byte 7, % no 8, PID do mouse em
//!   13..14, conexao no nibble baixo do byte 1 (0 RF / 1 BT / 2 USB).
//! - Antigos VT3 Pro / VT1 Pro Max: report de 7 bytes BB B0, estado no 5, % no 6 (7F = carregando
//!   sem nivel). (PyFlat/Device-Battery-Info)
//!
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), rapoo.cpp e rapoo_protocol.h.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;

const MODELS: &[(u16, u16, &str)] = &[
    (0x4610, 0x1410, "Rapoo VT0"),
    (0x4620, 0x1420, "Rapoo VT0 Max"),
    (0x4615, 0x1415, "Rapoo VT1"),
    (0x4644, 0x1444, "Rapoo VT1 Max"),
    (0x4622, 0x1422, "Rapoo VT2"),
    (0x4623, 0x1423, "Rapoo VT2 Max"),
    (0x4612, 0x1412, "Rapoo VT3"),
    (0x4617, 0x1417, "Rapoo VT3 Max (Gen 2)"),
    (0x4B06, 0x1B06, "Rapoo VT3 Max V2"),
    (0x4611, 0x1411, "Rapoo VT3S"),
    (0x4621, 0x1421, "Rapoo VT3S Max"),
    (0x4613, 0x1413, "Rapoo VT7"),
    (0x4616, 0x1416, "Rapoo VT7 Max"),
    (0x4614, 0x1414, "Rapoo VT9"),
    (0x4618, 0x1418, "Rapoo VT9 Max"),
    (0x4415, 0x1215, "Rapoo VT3 Pro"),
    (0x4433, 0x1233, "Rapoo VT1 Pro Max"),
];

fn is_legacy(pid: u16) -> bool {
    matches!(pid, 0x1215 | 0x4415 | 0x1233 | 0x4433)
}

#[derive(Debug, PartialEq, Default)]
pub struct Battery {
    pub percent: i32,
    pub charging: bool,
    /// 0 desconhecida, 1 receptor, 2 Bluetooth, 3 cabo.
    pub connection: u8,
    pub mouse_pid: u16,
}

pub fn decode(data: &[u8], legacy: bool) -> Option<Battery> {
    let mut b = Battery { percent: -1, ..Default::default() };
    let (state, percent);
    if !legacy && data.first() == Some(&0x07) {
        if data.len() < 9 || data[1] & 0x0F > 2 {
            return None;
        }
        b.connection = (data[1] & 0x0F) + 1;
        state = data[7];
        percent = data[8];
        if data.len() >= 15 {
            b.mouse_pid = u16::from_le_bytes([data[13], data[14]]);
        }
    } else if legacy && data.first() == Some(&0xBB) {
        if data.len() < 7 || data[1] != 0xB0 {
            return None;
        }
        state = data[5];
        percent = data[6];
    } else {
        return None;
    }
    if state != 1 && state != 2 {
        return None;
    }
    if percent > 100 && !(legacy && state == 2 && percent == 0x7F) {
        return None;
    }
    b.charging = state == 2;
    if percent <= 100 {
        b.percent = percent as i32;
    }
    Some(b)
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x24AE) {
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFF00 && c.usage() == 0x0002 && (c.input_len() == 19 || (c.input_len() == 7 && is_legacy(pid)))) {
            let Some(d) = ctx.open(&c) else { continue };
            let legacy = c.input_len() == 7;
            // O report passivo pode levar 3-4 s; sem flush, um report na fila ja serve.
            let end = Deadline::in_ms(5000);
            let mut battery = None;
            while !end.expired() {
                let Some(r) = d.read(end.left()) else { break };
                if let Some(b) = decode(&r, legacy) {
                    battery = Some(b);
                    break;
                }
            }
            let Some(b) = battery else { continue };
            let look = if b.mouse_pid != 0 { b.mouse_pid } else { pid };
            let model = MODELS.iter().find(|m| look == m.0 || look == m.1);
            let identity = model.map_or(look, |m| m.0);
            let mut name = model.map(|m| m.2.to_string()).unwrap_or_else(|| c.product_string().unwrap_or("").to_string());
            if name.is_empty() || name == "Rapoo Gaming Device" {
                name = "Rapoo wireless mouse".into();
            }
            let key = format!("rapoo:{}", hex4(identity));
            let mut s = Status::new(key.clone(), name, Kind::Mouse);
            s.percent = b.percent;
            s.charging = b.charging;
            // Carregando, o mouse aparece pelo cabo e pelo receptor: fica um so.
            if let Some(o) = ctx.out.iter_mut().find(|o| o.id == key) {
                if b.connection == 3 || b.charging {
                    *o = s.into_observation();
                }
                ctx.claim(vid, pid);
            } else {
                ctx.add(s, vid, pid);
            }
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_captured_frames() {
        // VT1 Pro Max (issue #136): receptor 100%, cabo carregando 95%.
        assert_eq!(decode(&[0xBB, 0xB0, 0x71, 0x84, 0x03, 0x01, 0x64], true).map(|b| (b.percent, b.charging)), Some((100, false)));
        assert_eq!(decode(&[0xBB, 0xB0, 0x71, 0x84, 0x03, 0x02, 0x5F], true).map(|b| (b.percent, b.charging)), Some((95, true)));
        assert_eq!(decode(&[0xBB, 0xB0, 0x71, 0x84, 0x03, 0x02, 0x7F], true).map(|b| (b.percent, b.charging)), Some((-1, true)));
        let nordic = [0x07, 0x02, 0, 0, 0, 0, 0, 0x01, 77, 0, 0, 0, 0, 0x17, 0x46];
        let b = decode(&nordic, false).unwrap();
        assert_eq!((b.percent, b.connection, b.mouse_pid), (77, 3, 0x4617));
    }
}
