//! ATK / VXE NearLink no dongle "NK mouse NANO" 373B:1216 (protocolo BITMOUSE da ATK).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), atk_bitmouse.cpp.
//!
//! Output report 08 de 64 bytes: [08][checksum][72][02][00][01][cmd][01][00 ...],
//! checksum = soma(bytes 2..63); alvo 01 = o mouse atras do dongle.
//! cmd 07 = nivel, cmd 0F = carregando. Resposta: [08][72][estado][seq][00][cmd][01][valor].
//! Fontes: HeyOkay/HaloBattery PR #350, qwr79/mouse-battery.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x373B;
const NANO: u16 = 0x1216;

pub fn request(cmd: u8) -> [u8; 64] {
    let mut req = [0u8; 64];
    req[0] = 0x08;
    req[2] = 0x72;
    req[3] = 0x02;
    req[5] = 0x01;
    req[6] = cmd;
    req[7] = 0x01;
    req[1] = req[2..].iter().fold(0u8, |a, x| a.wrapping_add(*x));
    req
}

fn query(d: &HidDevice, cmd: u8) -> Option<u8> {
    d.flush();
    d.write(&request(cmd)).ok()?;
    let end = Deadline::in_ms(700);
    loop {
        if end.expired() {
            return None;
        }
        let r = d.read(end.left())?;
        if r.len() < 8 || r[0] != 0x08 || r[1] != 0x72 || r[5] != cmd {
            continue;
        }
        if r[2] == 0xFF {
            return None;
        }
        return Some(r[7]);
    }
}

pub fn poll(ctx: &mut Ctx) {
    if ctx.is_claimed(VID, NANO) {
        return;
    }
    for c in ctx.find(VID, NANO, |c| c.usage_page() == 0xFF05 && c.usage() == 0x0001 && c.output_len() >= 64 && c.input_len() >= 8) {
        let Some(d) = ctx.open(&c) else { continue };
        let Some(level) = query(&d, 0x07).filter(|l| (1..=100).contains(l)) else { continue };
        let mut s = Status::new(format!("atknk:{}", hex4(NANO)), "ATK / VXE NK mouse", Kind::Mouse);
        s.percent = level as i32;
        s.charging = query(&d, 0x0F) == Some(1);
        ctx.add(s, VID, NANO);
        break;
    }
}
