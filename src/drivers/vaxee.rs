//! Receptor 4K da VAXEE e cabo do ZYGEN NP-01S V2.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), vaxee.cpp.
//! Feature report 0E: [0E][A5][cmd][01 = leitura][01]; resposta [0E][A5][cmd][..][1][valor].
//! cmd 0B = bateria (0..20, x5 = %), cmd 10 = carregando. Fonte: stuffz/mouse-battery-monitor.

use super::{sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x3057;
const RECEIVER: u16 = 0x1002;
const CABLE: u16 = 0x1011;

fn query(d: &HidDevice, command: u8) -> Option<u8> {
    let mut req = [0u8; 64];
    req[0] = 0x0E;
    req[1] = 0xA5;
    req[2] = command;
    req[3] = 0x01; // LEITURA, nunca escrita (02)
    req[4] = 0x01;
    d.send_feature_report(&req).ok()?;
    sleep_ms(100);
    let r = d.get_feature(0x0E)?;
    (r.len() >= 6 && r[0] == 0x0E && r[1] == 0xA5 && r[2] == command && r[4] == 1).then(|| r[5])
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(VID) {
        if pid != RECEIVER && pid != CABLE {
            continue;
        }
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFF05 && c.usage() == 1 && c.feature_len() == 64) {
            let Some(d) = ctx.open_feature_only(&c) else { continue };
            let Some(battery) = (0..2).find_map(|_| query(&d, 0x0B).filter(|b| *b <= 20)) else { continue };
            let name = if pid == CABLE { "ZYGEN NP-01S V2 Wireless" } else { "VAXEE wireless mouse" };
            let mut s = Status::new(format!("vaxee:{}", c.path().to_string_lossy()), name, Kind::Mouse);
            s.percent = battery as i32 * 5;
            s.charging = query(&d, 0x10).is_some_and(|v| v != 0);
            ctx.add(s, vid, pid);
            break;
        }
    }
}
