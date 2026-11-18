//! Mouses Redragon em receptores Areson/CompX 2.4 GHz (25A7:FA07 / FA08), ex.: M913, M915RGB-WL.
//! "GetPower", so leitura: SET_FEATURE na FF02:0002 da interface 1, 17 bytes [08][04][00 ...][checksum];
//! resposta na FF01:0000 da interface 1: [09][04][00][..][..][..][nivel 0..10][carregando]...,
//! o frame todo soma 0x55. Fontes: Qehbr/m913-ctl, Doomguy-BFG9000/redragon-m913-battery-tray.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), areson.cpp.

use super::{clamp100, hex4, kysona, sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

fn query_power(cmd: &HidDevice, reply: &HidDevice) -> Option<(i32, bool)> {
    let req = kysona::request(0x04);
    for attempt in 0..2 {
        if attempt > 0 {
            sleep_ms(200);
        }
        reply.flush();
        cmd.send_feature_report(&req).ok()?;
        let Some(r) = reply.read(1800).filter(|r| r.len() >= 17) else { continue };
        let total = r.iter().fold(0u8, |a, x| a.wrapping_add(*x));
        if total != 0x55 || r[0] != 0x09 || r[1] != 0x04 || r[2] != 0x00 || r[6] > 10 {
            continue;
        }
        return Some((clamp100(r[6] as i32 * 10), r[7] != 0));
    }
    None
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x25A7) {
        if pid != 0xFA07 && pid != 0xFA08 {
            continue; // FA7B/FA7C usam outra escala
        }
        let cmds = ctx.find(vid, pid, |c| c.interface_number() == 1 && c.usage_page() == 0xFF02 && c.usage() == 0x0002 && c.feature_len() >= 17);
        let inputs = ctx.find(vid, pid, |c| c.interface_number() == 1 && c.usage_page() == 0xFF01 && c.usage() == 0x0000 && c.input_len() >= 17);
        'outer: for c in &cmds {
            for i in &inputs {
                let (Some(cmd), Some(reply)) = (ctx.open_feature_only(c), ctx.open(i)) else { continue };
                if let Some((pct, chg)) = query_power(&cmd, &reply) {
                    let mut s = Status::new(format!("areson:{}:{}", hex4(vid), hex4(pid)), "Redragon wireless mouse", Kind::Mouse);
                    s.percent = pct;
                    s.charging = chg;
                    ctx.add(s, vid, pid);
                    break 'outer;
                }
            }
        }
    }
}
