//! Flydigi Vader 5 Pro (37D7:2401) e Apex 5 (37D7:2501), colecao FFA0:0001. So consultas de
//! arquitetura e informacao: nada de tomar o controle nem gravar configuracao.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), flydigi.cpp; protocolo do SDL
//! (SDL_hidapi_flydigi.c) e HeyOkay/HaloBattery#191.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x37D7;

/// (pid, report id dos comandos (0 = sem numero), ids de modelo aceitos, nome)
const MODELS: &[(u16, u8, &[u8], &str)] = &[(0x2401, 0x00, &[130], "Flydigi Vader 5 Pro"), (0x2501, 0x03, &[128, 129, 133, 134, 135], "Flydigi Apex 5")];

fn reply(d: &HidDevice, command: u8) -> Option<Vec<u8>> {
    let end = Deadline::in_ms(800);
    while !end.expired() {
        let mut r = d.read(end.left())?;
        // Alguns firmwares mantem um report id numerado na entrada.
        if r.len() >= 4 && r[0] == 0x04 && r[1] == 0x5A && r[2] == 0xA5 {
            r.remove(0);
        }
        if r.len() >= 5 && r[0] == 0x5A && r[1] == 0xA5 && r[2] == command {
            return Some(r);
        }
    }
    None
}

pub fn poll(ctx: &mut Ctx) {
    for &(pid, report, ids, name) in MODELS {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFFA0 && c.usage() == 1 && c.input_len() >= 12 && c.output_len() >= 6) {
            let Some(d) = ctx.open(&c) else { continue };
            d.flush();
            if d.write(&[report, 0x5A, 0xA5, 0x07, 0, 0]).is_err() {
                continue;
            }
            let modern = reply(&d, 0x07).is_some_and(|r| r[3] == 1 && r[4] == 0);
            if d.write(&[report, 0x5A, 0xA5, 0x01, 2, 0]).is_err() {
                continue;
            }
            let Some(r) = reply(&d, 0x01).filter(|r| r.len() >= 12 && ids.contains(&r[5])) else { continue };
            let (level, state, link) = ((r[11] & 0x0F) as i32, r[11] >> 4, r[6]);
            if state > 2 || level > 5 || (modern && link > 1) || (!modern && link != 1 && link != 2) {
                continue;
            }
            let mut s = Status::new(format!("flydigi:{}", hex4(pid)), name, Kind::Gamepad);
            s.percent = if state == 2 { 100 } else { level * 20 };
            s.charging = state == 1;
            ctx.add(s, VID, pid);
            break;
        }
    }
}
