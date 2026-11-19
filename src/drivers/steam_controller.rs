//! Steam Controller (VID 28DE): reports de bateria de entrada, layout do hid-steam do Linux.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), steam_controller.cpp.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;

fn transport(pid: u16) -> bool {
    // 1142 receptor 2015, 1302 cabo, 1303 Bluetooth, 1304 puck 2026, 1305 receptor interno.
    matches!(pid, 0x1142 | 0x1302 | 0x1303 | 0x1304 | 0x1305)
}

pub fn poll(ctx: &mut Ctx) {
    let colls: Vec<_> = ctx.api.device_list().filter(|c| c.vendor_id() == 0x28DE && c.usage_page() == 0xFF00 && c.input_len() >= 15 && transport(c.product_id())).cloned().collect();
    for c in colls {
        let pid = c.product_id();
        let puck = pid == 0x1304;
        if puck && c.usage() != 0x01 {
            continue; // colecao dos contatos de carga
        }
        let Some(d) = ctx.open(&c) else { continue };
        let legacy = pid == 0x1142;
        let end = Deadline::in_ms(5000);
        let mut received = false;
        let mut got = None;
        while !end.expired() {
            let r = d.read(if puck && !received { 250 } else { 100 });
            let Some(r) = r else {
                if puck && !received {
                    break; // o puck mostra quatro vagas mesmo com menos controles
                }
                continue;
            };
            received = true;
            if r.len() < 15 {
                continue;
            }
            if legacy {
                if r[0] == 0x01 && r[1] == 0 && r[2] == 0x04 && r[3] == 11 && r[14] <= 100 {
                    got = Some((r[14] as i32, false));
                    break;
                }
            } else if r[0] == 0x43 && r[2] <= 100 {
                got = Some((r[2] as i32, r[1] == 2));
                break;
            }
        }
        let Some((pct, charging)) = got else { continue };
        let hash = c.path().to_bytes().iter().fold(2166136261u32, |h, b| (h ^ *b as u32).wrapping_mul(16777619));
        let name = match c.product_string() {
            Some(n) if !n.is_empty() && !legacy && !puck && pid != 0x1305 => n.to_string(),
            _ => "Steam Controller".to_string(),
        };
        let mut s = Status::new(format!("steam:{}:{hash}", hex4(pid)), name, Kind::Gamepad);
        s.percent = pct;
        s.charging = charging;
        ctx.add(s, 0x28DE, pid);
    }
}
