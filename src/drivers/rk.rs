//! Royal Kludge (RK) MG6 Pro, VID 2E3C (protocolo "QiWang" do driver web da RK).
//! Output report 0 de 64 bytes: [01][01][02][01 = leitura]; resposta com os mesmos tres bytes,
//! [3] bit 4 = o mouse respondeu, [4..5] mV, [6..7] %, [10] USB plugado.
//! Dongle 00A6 / 00E6 (FF00:0001), cabo 00A5 / 00E5 (FFF2:0001).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), royal_kludge.cpp.

use super::{hex4, sleep_ms, Ctx, Deadline, Status};
use crate::device::Kind;

fn is_cable(pid: u16) -> bool {
    pid == 0x00A5 || pid == 0x00E5
}

fn poll_mg6(ctx: &mut Ctx, vid: u16, pid: u16) {
    let page = if is_cable(pid) { 0xFFF2 } else { 0xFF00 };
    for c in ctx.find(vid, pid, |c| c.usage_page() == page && c.usage() == 1 && c.output_len() == 65 && c.input_len() == 65) {
        let Some(d) = ctx.open(&c) else { continue };
        let (mut answered, mut online, mut percent, mut plugged) = (false, false, -1, false);
        for attempt in 0..3 {
            if online {
                break;
            }
            if attempt > 0 {
                sleep_ms(100);
            }
            d.flush();
            if d.write(&[0x00, 0x01, 0x01, 0x02, 0x01]).is_err() {
                break;
            }
            let end = Deadline::in_ms(600);
            while !end.expired() {
                let Some(r) = d.read(end.left()).filter(|r| r.len() >= 11) else { break };
                if r[0] & 0x1F != 0x01 || r[1] & 0x0F != 0x01 || r[2] != 0x02 {
                    continue;
                }
                answered = true;
                if (r[3] >> 4) & 1 == 0 {
                    break; // o mouse nao respondeu
                }
                let p = u16::from_le_bytes([r[6], r[7]]) as i32;
                if p > 100 {
                    break;
                }
                online = true;
                percent = p;
                plugged = r[10] != 0;
                break;
            }
        }
        if !answered {
            continue;
        }
        let mut s = Status::new(format!("rk:{}:{}", hex4(vid), hex4(pid)), "RK MG6 Pro", Kind::Mouse);
        if online {
            s.percent = percent;
            s.charging = plugged || is_cable(pid);
        } else {
            s.asleep = true;
        }
        ctx.add(s, vid, pid);
        return;
    }
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x2E3C) {
        if matches!(pid, 0x00A5 | 0x00A6 | 0x00E5 | 0x00E6) {
            poll_mg6(ctx, vid, pid);
        }
    }
}
