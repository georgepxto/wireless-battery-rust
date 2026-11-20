//! ATK / VXE NearLink no dongle "NK mouse NANO" 373B:1216 (protocolo BITMOUSE da ATK).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), atk_bitmouse.cpp.
//!
//! Output report 08 de 64 bytes: [08][checksum][72][02][00][01][cmd][01][00 ...],
//! checksum = soma(bytes 2..63); alvo 01 = o mouse atras do dongle.
//! cmd 07 = nivel, cmd 0F = carregando. Resposta: [08][72][estado][seq][00][cmd][01][valor].
//! Fontes: HeyOkay/HaloBattery PR #350, qwr79/mouse-battery.
//!
//! ATK Zero: a mesma moldura na FF05:0001, alvo 00 no cabo (1154) ou 01 nos receptores 8K (124F / 1155),
//! checksum fixo 7D; so o estado 00 vale. cmd 07 = nivel, cmd 08 = carregando (02) no cabo.
//! (Fan4Metal/mouse_tray, nordic54.py)
//!
//! ATK "8K Nano Dongle" 373B:1178 (receptor Holtek): feature report 00 [82][00 x7] (GetConfigData);
//! resposta como input [82]...[8] bateria (bit 7 = carregando), [9] link (0 = sem mouse). (ATK HUB)

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x373B;
const NANO: u16 = 0x1216;

#[derive(Clone, Copy, PartialEq)]
enum Link {
    Nk,
    ZeroCable,
    ZeroReceiver,
}

fn request_for(cmd: u8, link: Link) -> [u8; 64] {
    let mut req = [0u8; 64];
    req[0] = 0x08;
    req[2] = 0x72;
    req[3] = 0x02;
    req[5] = if link == Link::ZeroCable { 0x00 } else { 0x01 };
    req[6] = cmd;
    req[7] = 0x01;
    req[1] = if link == Link::Nk { req[2..].iter().fold(0u8, |a, x| a.wrapping_add(*x)) } else { 0x7D };
    req
}

fn query(d: &HidDevice, cmd: u8) -> Option<u8> {
    query_link(d, cmd, Link::Nk)
}

fn query_link(d: &HidDevice, cmd: u8, link: Link) -> Option<u8> {
    let zero = link != Link::Nk;
    d.flush();
    d.write(&request_for(cmd, link)).ok()?;
    let end = Deadline::in_ms(if zero { 500 } else { 700 });
    loop {
        if end.expired() {
            return None;
        }
        let r = d.read(end.left())?;
        if r.len() < 8 || r[0] != 0x08 || r[1] != 0x72 || r[5] != cmd || (zero && r[6] != 0x01) {
            continue;
        }
        if if zero { r[2] != 0 } else { r[2] == 0xFF } {
            return None;
        }
        return Some(r[7]);
    }
}

fn poll_zero(ctx: &mut Ctx) {
    let mut reported = false;
    // Cabo primeiro: leitura fresca enquanto carrega; a mesma chave nos dois caminhos.
    for pid in [0x1154u16, 0x124F, 0x1155] {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFF05 && c.usage() == 1 && c.output_len() == 64 && c.input_len() == 64) {
            ctx.claim(VID, pid); // tira as molduras do Kysona desta colecao, mesmo dormindo
            if reported {
                break;
            }
            let Some(d) = ctx.open(&c) else { continue };
            let wired = pid == 0x1154;
            let link = if wired { Link::ZeroCable } else { Link::ZeroReceiver };
            let Some(level) = query_link(&d, 0x07, link).filter(|l| *l <= 100) else { continue };
            let mut s = Status::new("atkzero:1154", "ATK Zero", Kind::Mouse);
            s.percent = level as i32;
            s.charging = wired && query_link(&d, 0x08, link) == Some(0x02);
            ctx.add(s, VID, pid);
            reported = true;
            break;
        }
    }
}

fn poll_nano8k(ctx: &mut Ctx) {
    let pid = 0x1178;
    if ctx.is_claimed(VID, pid) {
        return;
    }
    for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFF10 && c.usage() == 1 && c.feature_len() >= 9 && c.input_len() >= 11) {
        let Some(d) = ctx.open(&c) else { continue };
        d.flush();
        if d.send_feature_report(&[0x00, 0x82]).is_err() {
            continue;
        }
        let end = Deadline::in_ms(700);
        let mut reply = None;
        while !end.expired() {
            let Some(r) = d.read(end.left()).filter(|r| r.len() >= 10) else { break };
            if r[0] == 0x82 {
                reply = Some(r);
                break;
            }
        }
        let Some(r) = reply else { continue };
        let level = (r[8] & 0x7F) as i32;
        if r[9] == 0 || !(1..=100).contains(&level) {
            ctx.claim(VID, pid); // sem mouse ligado: o Kysona nao deve sondar este dongle
            return;
        }
        let mut s = Status::new(format!("atknano8k:{}", hex4(pid)), "ATK 8K Nano mouse", Kind::Mouse);
        s.percent = level;
        s.charging = r[8] & 0x80 != 0;
        ctx.add(s, VID, pid);
        return;
    }
}

pub fn poll(ctx: &mut Ctx) {
    poll_zero(ctx);
    poll_nano8k(ctx);
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
