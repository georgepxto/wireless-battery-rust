//! Controles compativeis com o Switch Pro (057E:2009), inclusive o GameSir Tarantula Pro no
//! receptor em modo Switch. Byte 2 do report completo 0x30: bits 7..5 = nivel 0..4, bit 4 =
//! carregando (SDL_hidapi_switch.c). O Steam Input ou outro programa precisa ligar os reports
//! completos; aqui so se escuta.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), switch_pro.cpp.

use super::{Ctx, Deadline, Status};
use crate::device::Kind;

fn path_hash(p: &[u8]) -> u32 {
    p.iter().fold(2166136261u32, |h, b| (h ^ *b as u32).wrapping_mul(16777619))
}

pub fn poll(ctx: &mut Ctx) {
    let (vid, pid) = (0x057E, 0x2009);
    for c in ctx.find(vid, pid, |c| c.usage_page() == 0x01 && matches!(c.usage(), 0x04 | 0x05) && c.input_len() >= 13) {
        let Some(d) = ctx.open_read_only(&c) else { continue };
        let end = Deadline::in_ms(350);
        while !end.expired() {
            let Some(r) = d.read(end.left()) else { break };
            if r.len() < 13 || r[0] != 0x30 {
                continue;
            }
            let level = (r[2] & 0xE0) >> 5;
            if level > 4 {
                continue;
            }
            let name = c.product_string().filter(|n| !n.is_empty()).unwrap_or("Nintendo Switch Pro Controller");
            let mut s = Status::new(format!("switchpro:{}", path_hash(c.path().to_bytes())), name, Kind::Gamepad);
            s.percent = 25 * level as i32;
            s.charging = r[2] & 0x10 != 0 && level < 4;
            ctx.add(s, vid, pid);
            break;
        }
    }
}
