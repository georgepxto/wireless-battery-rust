//! Headset Fractal Design Scape (VID 36BC): dongle 2.4 GHz (0001) ou cabo (2001).
//! Output report 0x02: [02][tipo][comando]; GET_DEVICE_STATE = 0x21 para o dongle (0x11) ou o
//! headset (0xF1). Dongle: byte 4 = 1 com o headset ligado. Headset: byte 15 = bateria, byte 23 = cabo.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), fractal.cpp;
//! fonte: configurador web Adjust Pro da Fractal.

use super::{read_until, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

fn get_state(d: &HidDevice, kind: u8) -> Option<Vec<u8>> {
    d.flush();
    d.write(&[0x02, kind, 0x21]).ok()?;
    read_until(d, 1000, |r| (r.len() >= 24 && r[0] == 0x02 && r[1] == kind && r[2] == 0x21).then(|| r.to_vec()))
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x36BC) {
        if pid != 0x0001 && pid != 0x2001 {
            continue;
        }
        let cable = pid == 0x2001;
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFF00 && c.usage() == 0x0001 && c.output_len() >= 4 && c.input_len() >= 24) {
            let Some(d) = ctx.open(&c) else { continue };
            let mut s = Status::new("fractal:scape", "Fractal Scape", Kind::Headset);
            if !cable {
                let Some(r) = get_state(&d, 0x11) else { continue };
                if r[4] != 1 {
                    s.connected = false; // headset desligado, longe ou na base
                    ctx.add(s, vid, pid);
                    break;
                }
            }
            let Some(r) = get_state(&d, 0xF1).filter(|r| r[15] <= 100) else { continue };
            s.percent = r[15] as i32;
            s.charging = r[23] == 1;
            ctx.add(s, vid, pid);
            break;
        }
    }
}
