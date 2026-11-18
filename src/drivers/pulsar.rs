//! Pulsar X2 V3 / X2 V3 eS e outros Pulsar com chip Sonix (VID 3710).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), pulsar.cpp.
//!
//! Feature report 0, pacote de 64 bytes: [00][categoria][registrador][sub][00 ...][soma lo][soma hi].
//! Leitura de bateria = 08 81 01; SET_FEATURE e GET_FEATURE a cada ~50 ms ate a resposta ecoar o
//! comando: [estado][08][81][01][..][..][porcentagem]. Estado 01 = respondeu, 02 = pendente,
//! 05 = mouse fora do dongle (ou pendente). A colecao vendor fica nas interfaces 2 e 3; a 2 recusa
//! SET_FEATURE, entao as maiores vem primeiro. No cabo o mouse esta carregando.
//! Fontes: darthsoup/PulsarBattery, bluesceada/pulsar-mouse-linux.

use super::{hex4, sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x3710;
const PRODUCTS: &[(u16, bool, Option<&str>)] =
    &[(0x3402, true, Some("Pulsar X2 V3")), (0x3406, true, Some("Pulsar X2 V3 eS")), (0x5402, false, None), (0x5403, false, None)];

enum Reply {
    Battery(i32),
    Offline,
    None,
}

pub fn request() -> [u8; 65] {
    let mut req = [0u8; 65]; // [0] = report id 0, depois o pacote de 64 bytes
    req[2] = 0x08;
    req[3] = 0x81;
    req[4] = 0x01;
    let sum: u32 = req[1..63].iter().map(|b| *b as u32).sum();
    req[63] = (sum & 0xFF) as u8;
    req[64] = (sum >> 8) as u8;
    req
}

fn read_battery(d: &HidDevice) -> Reply {
    let req = request();
    let mut saw_offline = false;
    for _ in 0..3 {
        if d.send_feature_report(&req).is_err() {
            return Reply::None;
        }
        for _ in 0..6 {
            sleep_ms(50);
            let Some(r) = d.get_feature(0) else { return Reply::None };
            if r.len() < 65 {
                return Reply::None;
            }
            let w = &r[1..];
            if w[1] != 0x08 || w[2] != 0x81 || w[3] != 0x01 {
                continue;
            }
            if w[0] == 0x01 {
                return if w[6] > 100 { Reply::None } else { Reply::Battery(w[6] as i32) };
            }
            saw_offline = w[0] == 0x05;
        }
    }
    if saw_offline {
        Reply::Offline
    } else {
        Reply::None
    }
}

pub fn poll(ctx: &mut Ctx) {
    for &(pid, cable, name) in PRODUCTS {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let mut colls = ctx.find(VID, pid, |c| c.feature_len() >= 65);
        colls.sort_by_key(|c| std::cmp::Reverse(c.interface_number()));
        for c in &colls {
            let Some(d) = ctx.open_feature_only(c) else { continue };
            let rep = read_battery(&d);
            let pct = match rep {
                Reply::None => continue,
                Reply::Battery(p) => p,
                Reply::Offline => -1,
            };
            let mut s = Status::new(format!("pulsar:{}", hex4(pid)), "", Kind::Mouse);
            if cable {
                s.name = name.unwrap_or("Pulsar mouse").into();
                s.charging = true;
            } else {
                let n = c.product_string().unwrap_or("");
                let generic = n.is_empty() || n.contains("Dongle") || n.contains("Receiver");
                s.name = if generic { "Pulsar wireless mouse".into() } else { n.into() };
                s.connected = pct > 0;
            }
            if pct > 0 || (cable && pct >= 0) {
                s.percent = pct;
            }
            ctx.add(s, VID, pid);
            break;
        }
    }
}
