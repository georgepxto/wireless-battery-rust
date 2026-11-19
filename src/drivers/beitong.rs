//! BEITONG (Betop) KP20 (20BC:5128) e KP40D (20BC:515C) no receptor 2.4 GHz, modo XInput.
//! Colecao 00FF:0003, report 02; cada byte depois do report id vai embaralhado com uma cifra fixa
//! de nibbles: enc(x) = S[x >> 4] << 4 | ((x & 0x0F) ^ 0x08). Pedido 02 15 -> 02 15 <% 0..100> 00 <carga>.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), beitong.cpp.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;

const S: [u8; 16] = [0x0, 0xA, 0x6, 0xF, 0x1, 0xC, 0x2, 0x9, 0xE, 0x3, 0x7, 0xD, 0x4, 0xB, 0x5, 0x8];

pub fn enc(x: u8) -> u8 {
    (S[(x >> 4) as usize] << 4) | ((x & 0x0F) ^ 0x08)
}

pub fn dec(x: u8) -> u8 {
    let hi = S.iter().position(|v| *v == x >> 4).unwrap_or(0) as u8;
    (hi << 4) | ((x & 0x0F) ^ 0x08)
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x20BC) {
        let name = match pid {
            0x5128 => "BEITONG KP20",
            0x515C => "BEITONG KP40D",
            _ => continue,
        };
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0x00FF && c.usage() == 0x0003 && c.output_len() >= 3 && c.input_len() >= 5) {
            let Some(d) = ctx.open(&c) else { continue };
            d.flush();
            if d.write(&[0x02, enc(0x15)]).is_err() {
                continue;
            }
            let end = Deadline::in_ms(800);
            let mut got = None;
            while !end.expired() {
                let Some(r) = d.read(end.left().min(200)) else { continue };
                if r.len() >= 5 && r[0] == 0x02 && dec(r[1]) == 0x15 {
                    got = Some(r);
                    break;
                }
            }
            let Some(r) = got else { continue };
            let level = dec(r[2]);
            if level > 100 {
                continue;
            }
            let mut charge = dec(r[4]);
            if pid == 0x515C {
                charge &= 0x0F;
            }
            let mut s = Status::new(format!("beitong:{}", hex4(pid)), name, Kind::Gamepad);
            s.percent = level as i32;
            s.charging = charge == 1;
            ctx.add(s, vid, pid);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cipher() {
        assert_eq!(enc(0x15), 0xAD);
        for x in 0..=255u8 {
            assert_eq!(dec(enc(x)), x);
        }
    }
}
