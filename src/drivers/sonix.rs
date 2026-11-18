//! Teclados SONiX no dongle 2.4 GHz: AJAZZ AK820 MAX e AULA F108 Pro (05AC:024F), AJAZZ AK980 V2
//! (0C45:FEFE). Interface 3, colecao FF60:0061, reports de 33 bytes (id 0 + 32).
//! Pedido [20 01 00 ... soma]; resposta [20 01 00 nivel ... soma]. So no modo 2.4G.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), sonix.cpp;
//! fonte do protocolo: donglover69/sonix-keyboard-studio.

use super::{hex4, sum8, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const DONGLES: [(u16, u16); 2] = [(0x05AC, 0x024F), (0x0C45, 0xFEFE)];
const LEN: usize = 32;

fn query_level(d: &HidDevice) -> Option<u8> {
    let mut req = [0u8; 1 + LEN];
    req[1] = 0x20;
    req[2] = 0x01;
    req[LEN] = sum8(&req[1..LEN]);
    d.flush();
    d.write(&req).ok()?;
    let end = Deadline::in_ms(500);
    loop {
        if end.expired() {
            return None;
        }
        let r = d.read(end.left())?;
        if r.len() < LEN || r[0] != 0x20 || r[1] != 0x01 || r[LEN - 1] != sum8(&r[..LEN - 1]) {
            continue;
        }
        return Some(r[3]);
    }
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in DONGLES {
        if ctx.is_claimed(vid, pid) {
            continue;
        }
        for c in ctx.find(vid, pid, |c| {
            c.interface_number() == 3 && c.usage_page() == 0xFF60 && c.usage() == 0x0061 && c.output_len() as usize == 1 + LEN && c.input_len() as usize == 1 + LEN
        }) {
            let Some(d) = ctx.open(&c) else { continue };
            // O primeiro pedido pode ficar sem resposta enquanto o link 2.4 GHz acorda.
            let Some(level) = query_level(&d).or_else(|| query_level(&d)) else { continue };
            let name = c.product_string().unwrap_or("").trim_end_matches(" 2.4G").to_string();
            let mut s = Status::new(format!("sonix:{}:{}", hex4(vid), hex4(pid)), if name.is_empty() { "Wireless keyboard".into() } else { name }, Kind::Keyboard);
            s.connected = (1..=100).contains(&level);
            if s.connected {
                s.percent = level as i32;
            }
            ctx.add(s, vid, pid);
            break;
        }
    }
}
