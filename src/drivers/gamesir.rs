//! Controles GameSir no receptor 2.4 GHz ou no cabo (VID 3537). O XInput ve esses controles como
//! "com fio", entao a bateria vem da colecao do fabricante.
//! - Nova Lite / Nova 2 / Nova 2 Lite (receptor 1098, Nova Lite 2 1040, cabo 100F): pagina FF7A,
//!   pedido 01 01 -> [0]=01 ... [18] = 0..100, FF = carregando (sem nivel no cabo). Frames "F4 00"
//!   de estado sao ignorados. (WWWyyy452/GamesirBatteryTray, AJAY0406/Gamesir-Nova-2-Lite-Battery-Detection)
//! - Cyclone 2 (100B, modo XInput): colecao FFF0:0040, pedido 0F 03 -> input 12: [35] cabo/carregando,
//!   [36] %. (vdemonchy/cyclone2-linux)
//!
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), gamesir.cpp.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;

const VID: u16 = 0x3537;

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(VID) {
        if matches!(pid, 0x1098 | 0x100F | 0x1040) {
            nova(ctx, vid, pid);
        } else if pid == 0x100B {
            cyclone2(ctx, vid, pid);
        }
    }
}

fn nova(ctx: &mut Ctx, vid: u16, pid: u16) {
    for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFF7A && c.output_len() >= 3) {
        let Some(d) = ctx.open(&c) else { continue };
        d.flush();
        if d.write(&[0x00, 0x01, 0x01]).is_err() {
            continue;
        }
        let end = Deadline::in_ms(800);
        let mut level = None;
        while !end.expired() {
            let Some(r) = d.read(end.left().min(200)) else { continue };
            if r.len() >= 19 && r[0] == 0x01 {
                level = Some(r[18]);
                break;
            }
        }
        let Some(level) = level.filter(|l| *l <= 100 || *l == 0xFF) else { continue };
        let mut s = Status::new(format!("gamesir:{}", hex4(pid)), "GameSir Nova", Kind::Gamepad);
        if level == 0xFF {
            s.charging = true; // fica sem porcentagem: o ultimo valor conhecido e mantido
        } else {
            s.percent = level as i32;
        }
        ctx.add(s, vid, pid);
        break;
    }
}

fn cyclone2(ctx: &mut Ctx, vid: u16, pid: u16) {
    for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFFF0 && c.usage() == 0x0040 && c.output_len() >= 3 && c.input_len() >= 37) {
        let Some(d) = ctx.open(&c) else { continue };
        d.flush();
        if d.write(&[0x0F, 0x03]).is_err() {
            continue;
        }
        let end = Deadline::in_ms(1500);
        let mut got = None;
        while !end.expired() {
            let Some(r) = d.read(end.left().min(200)) else { continue };
            if r.len() >= 37 && r[0] == 0x12 && r[36] <= 100 {
                got = Some((r[36], r[35] == 1));
                break;
            }
        }
        let Some((pct, charging)) = got else { continue };
        let mut s = Status::new(format!("gamesir:{}", hex4(pid)), "GameSir Cyclone 2", Kind::Gamepad);
        s.percent = pct as i32;
        s.charging = charging;
        ctx.add(s, vid, pid);
        break;
    }
}
