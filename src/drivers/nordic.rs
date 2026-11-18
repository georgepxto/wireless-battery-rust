//! Ninjutso Sora V2 (1915:AE1C) e Sprime PM1 (1915:AC1C / AD8C, cabos AC11-AC13): mesma consulta.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), ninjutso.cpp e sprime.cpp.
//! Feature report 05, comando 15: [9] bateria, [10] carregando, [11] cheio (Sprime), [12] online.
//! Fontes: NinjaForce (createSoraV2) e o driver web da Sprime (sprime.pro).

use super::{hex4, sleep_ms, Ctx, Status};
use crate::device::Kind;

fn query(ctx: &mut Ctx, vid: u16, pid: u16, name: &str, cable: bool, sprime: bool) -> bool {
    for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFFA0 && c.usage() == 1 && c.feature_len() >= 32) {
        let Some(d) = ctx.open_feature_only(&c) else { continue };
        let mut req = [0u8; 32];
        req[0] = 0x05;
        req[1] = 0x15;
        req[4] = 0x01;
        let mut s = Status::new(format!("{}:{}:{}", if sprime { "sprime" } else { "ninjutso" }, hex4(vid), hex4(pid)), name, Kind::Mouse);
        let mut answered = false;
        for attempt in 0..4 {
            if attempt > 0 {
                sleep_ms(90);
            }
            if d.send_feature_report(&req).is_err() {
                break;
            }
            sleep_ms(15);
            let Some(r) = d.get_feature(0x05) else { continue };
            if r.len() < 13 || r[0] != 0x05 || r[1] != 0x15 || r[9] > 100 {
                continue;
            }
            answered = true;
            let full = sprime && r[11] != 0;
            s.connected = cable || r[12] != 0;
            s.percent = if !s.connected {
                -1
            } else if full || (!sprime && r[9] == 0) {
                100 // Ninjutso: 0 significa 100%
            } else {
                r[9] as i32
            };
            s.charging = s.connected && r[10] != 0 && !full;
            if s.connected {
                break;
            }
        }
        if answered {
            ctx.add(s, vid, pid);
            return true;
        }
    }
    false
}

pub fn poll(ctx: &mut Ctx) {
    let vid = 0x1915;
    if !ctx.is_claimed(vid, 0xAE1C) {
        query(ctx, vid, 0xAE1C, "Ninjutso Sora V2", false, false);
    }
    for (vid, pid) in ctx.products(vid) {
        let cable = (0xAC11..=0xAC13).contains(&pid);
        if cable || matches!(pid, 0xAC1C | 0xAD8C) {
            query(ctx, vid, pid, "Sprime PM1", cable, true);
        }
    }
}
