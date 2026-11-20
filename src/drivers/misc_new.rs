//! Leitores pequenos, cada um com o seu protocolo de leitura:
//! - Receptor SinoWealth 258A:002F (Redragon M602-KS / M690 Pro): feature 05 [05][cmd]; 80 = link,
//!   90 = bateria ([2] = 11, [3] = %). (OpenMouse-Project/mouse-protocol)
//! - Press Play ICARUS V2 Nearlink (7768:6434): output 06 05 na FFB2:0001 -> [2] = 26, [13] = %.
//! - Fantech Raigor III / Raigor II WG10 (dongle 1EA7:0066 / 0064): output B5 05 na FFB5:0001; o dongle
//!   manda a pagina de status em pedacos, o pedaco 94 traz o %; B5 67 ... = aviso ao acordar.
//!   (tuttizpro218/t20-battery)
//! - Mouses Redragon no receptor Evision 320F:226F (K1NG M999): o pedido de status Evision com sub 02.
//!
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0): sinowealth_receiver.cpp,
//! pressplay_icarus.cpp, raigor.cpp e redragon_evision.cpp.

use super::{evision, hex4, sleep_ms, Ctx, Deadline, Status};
use crate::device::Kind;

pub fn poll_sinowealth(ctx: &mut Ctx) {
    let (vid, pid) = (0x258A, 0x002F);
    if ctx.is_claimed(vid, pid) {
        return;
    }
    for c in ctx.find(vid, pid, |c| c.interface_number() == 1 && c.usage_page() == 0xFF00 && c.usage() == 1 && c.feature_len() == 8 && c.input_len() == 0 && c.output_len() == 0) {
        let Some(d) = ctx.open_feature_only(&c) else { continue };
        let q = |cmd: u8| {
            d.send_feature_report(&[5, cmd, 0, 0, 0, 0, 0, 0]).ok()?;
            sleep_ms(100);
            d.get_feature(5).filter(|r| r.len() == 8 && r[0] == 5 && r[1] == cmd)
        };
        let Some(b) = q(0x80).filter(|l| l[2] == 1).and_then(|_| q(0x90)).filter(|b| b[2] == 0x11 && b[3] <= 100) else { continue };
        let mut s = Status::new("sinowealth:258A:002F", "Sinowealth wireless mouse", Kind::Mouse);
        s.percent = b[3] as i32;
        ctx.add(s, vid, pid);
        break;
    }
}

pub fn poll_pressplay(ctx: &mut Ctx) {
    let (vid, pid) = (0x7768, 0x6434);
    if ctx.is_claimed(vid, pid) {
        return;
    }
    for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFFB2 && c.usage() == 1 && c.input_len() == 64 && c.output_len() == 64) {
        let Some(d) = ctx.open(&c) else { continue };
        d.flush();
        let mut req = vec![0u8; c.output_len() as usize];
        req[0] = 0x06;
        req[1] = 0x05;
        let Some(r) = d.set_output_and_read(&req, 500).filter(|r| r.len() > 13 && r[0] == 0x06 && r[1] == 0x05 && r[2] == 0x26 && r[13] <= 100) else { continue };
        let mut s = Status::new("pressplay:icarus-v2", "Press Play ICARUS V2 Nearlink", Kind::Mouse);
        s.percent = r[13] as i32;
        ctx.add(s, vid, pid);
        break;
    }
}

pub fn poll_raigor(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x1EA7) {
        if pid != 0x0066 && pid != 0x0064 {
            continue;
        }
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFFB5 && c.usage() == 1 && c.input_len() == 8 && c.output_len() == 8) {
            let Some(d) = ctx.open(&c) else { continue };
            d.flush();
            if d.write(&[0xB5, 0x05, 0, 0, 0, 0, 0, 0]).is_err() {
                continue;
            }
            let end = Deadline::in_ms(1200);
            let mut pct = None;
            while !end.expired() {
                let Some(r) = d.read(end.left().min(200)) else { continue };
                if r.len() != 8 || r[0] != 0xB5 {
                    continue;
                }
                if r[1] == 0x05 && r[2] == 0x94 && r[3] == 0x04 && r[6] <= 100 {
                    pct = Some(r[6]);
                    break;
                }
                if r[1] == 0x67 && r[7] <= 100 {
                    pct = Some(r[7]);
                    break;
                }
            }
            let Some(pct) = pct else { continue };
            let name = c.product_string().filter(|n| !n.is_empty()).unwrap_or("2.4 GHz mouse");
            let kind = match ctx.kind_from_collections(vid, pid) {
                Kind::Other => Kind::Mouse,
                k => k,
            };
            let mut s = Status::new(format!("raigor:{}:{}", hex4(vid), hex4(pid)), name, kind);
            s.percent = pct as i32;
            ctx.add(s, vid, pid);
            break;
        }
    }
}

pub fn poll_redragon_evision(ctx: &mut Ctx) {
    let (vid, pid) = (0x320F, 0x226F);
    if ctx.is_claimed(vid, pid) {
        return;
    }
    let Some(c) = ctx.find(vid, pid, |c| c.usage_page() == 0xFF1C && c.usage() == 0x0092).into_iter().next() else { return };
    let mut s = Status::new(format!("redragon_evision:{}", hex4(pid)), "Redragon wireless mouse", Kind::Mouse);
    match ctx.open(&c).and_then(|d| evision::read_evision(&d, 0x02, 1000)) {
        Some((p, chg)) => {
            s.percent = p;
            s.charging = chg;
        }
        None => s.asleep = true,
    }
    ctx.add(s, vid, pid);
}
