//! Fantech Helios Go XD5 (VID 25A7; cabo FA7B, dongle FA7C).
//! Feature report 08 de 17 bytes [08][cmd][0 x14][checksum] (soma total 0x55); cmd 04 = bateria,
//! resposta [6] = %, [7] = carregando. O feature e a resposta ficam em colecoes diferentes da
//! interface 1: escreve numa e escuta todas as de entrada.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), fantech.cpp; protocolo de
//! jarjut/xd5-go-web e libratbag#1893.

use super::{hex4, kysona, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::Mode;

#[derive(Clone, Copy)]
struct Reply {
    pct: i32,
    charging: bool,
    checksum_ok: bool,
}

/// O byte de comando pode vir no indice 0 ou 1, conforme o HID mantenha o report id.
fn parse(r: &[u8]) -> Option<Reply> {
    let chk = r.len() >= 17 && r[..17].iter().fold(0u8, |a, x| a.wrapping_add(*x)) == 0x55;
    let plausible = |p: usize, c: usize| -> Option<Reply> {
        let (pv, cv) = (*r.get(p)? as i32, *r.get(c)?);
        ((1..=100).contains(&pv) && cv <= 1).then_some(Reply { pct: pv, charging: cv != 0, checksum_ok: chk })
    };
    if r.len() >= 7 && r[0] == 0x04 {
        if let Some(x) = plausible(5, 6).or_else(|| plausible(4, 5)) {
            return Some(x);
        }
    }
    if r.len() >= 8 && r[1] == 0x04 {
        return plausible(6, 7);
    }
    None
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x25A7) {
        if pid != 0xFA7B && pid != 0xFA7C {
            continue;
        }
        let all = ctx.find(vid, pid, |_| true);
        let Some(feat) = all.iter().find(|c| c.feature_len() >= 17) else { continue };
        let Some(cmd) = ctx.open(feat).or_else(|| ctx.open_feature_only(feat)) else { continue };
        let mut readers = Vec::new();
        for c in all.iter().filter(|c| c.path() != feat.path() && c.input_len() >= 6) {
            if let Ok(d) = ctx.api.open(c, Mode::ReadWrite) {
                readers.push(d);
            }
        }
        cmd.flush();
        if cmd.send_feature_report(&kysona::request(0x04)).is_err() {
            continue;
        }
        let mut found: Option<Reply> = None;
        let mut remember = |c: Reply| {
            if found.is_none_or(|f| c.checksum_ok && !f.checksum_ok) {
                found = Some(c);
            }
        };
        let end = Deadline::in_ms(500);
        'wait: while !end.expired() {
            let own = (feat.input_len() >= 6).then_some(&cmd);
            for d in own.into_iter().chain(readers.iter()) {
                if end.expired() {
                    break 'wait;
                }
                if let Some(c) = d.read(end.left().min(40)).filter(|r| r.len() >= 6).and_then(|r| parse(&r)) {
                    remember(c);
                    if c.checksum_ok {
                        break 'wait;
                    }
                }
            }
        }
        // Alguns firmwares respondem com o feature report em vez do input.
        if let Some(c) = cmd.get_feature(0x08).and_then(|f| parse(&f)) {
            remember(c);
        }
        let Some(r) = found else { continue };
        let mut s = Status::new(format!("fantech:{}", hex4(pid)), "Fantech Helios Go XD5", Kind::Mouse);
        s.percent = r.pct;
        s.charging = r.charging;
        ctx.add(s, vid, pid);
        break;
    }
}
