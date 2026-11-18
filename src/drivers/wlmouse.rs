//! WLMouse (VID 36A7): Beast X 8K, Beast X Mini / Mini Pro, Beast X Pro, Beast MAX, Sword X 8K.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), wlmouse.cpp.
//! Colecao FFFF, feature report 0 de 65 bytes: [00][00][00][02][02][00][83]; resposta
//! [00][estado][00][02][02][00][83][carregando][bateria%]; A1/A2 = respondeu, A0 = dormindo.
//! Fontes: djnawara/mouse-gauge, Waturus/HaloBattery.

use super::{hex4, sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x36A7;

/// Cabos antes dos receptores: um cabo que responde reivindica o receptor dele.
const MODELS: &[(u16, bool, &str, u16)] = &[
    (0xA879, false, "WLMouse Sword X 8K", 0xA878),
    (0xA884, false, "WLMouse Beast X 8K", 0xA883),
    (0xA860, false, "WLMouse mouse", 0),
    (0xA866, false, "WLMouse mouse", 0),
    (0xA867, false, "WLMouse mouse", 0),
    (0xA856, true, "WLMouse Beast X V2", 0),
    (0xA868, true, "WLMouse Beast X Mini Pro", 0),
    (0xA870, true, "WLMouse Beast X Pro", 0),
    (0xA878, true, "WLMouse Sword X 8K", 0),
    (0xA880, true, "WLMouse Beast MAX", 0),
    (0xA883, true, "WLMouse Beast X 8K", 0),
    (0xA885, true, "WLMouse Beast X Mini", 0),
];

enum Reply {
    Battery(u8, bool),
    Asleep,
    None,
}

fn query(d: &HidDevice) -> Reply {
    let mut req = [0u8; 65];
    req[3] = 0x02;
    req[4] = 0x02;
    req[6] = 0x83;
    for _ in 0..2 {
        if d.send_feature_report(&req).is_err() {
            return Reply::None;
        }
        for _ in 0..6 {
            sleep_ms(60);
            let Some(r) = d.get_feature(0).filter(|r| r.len() >= 9) else { return Reply::None };
            if r[3] != 0x02 || r[4] != 0x02 || r[6] != 0x83 {
                continue;
            }
            if r[1] == 0xA0 {
                return Reply::Asleep;
            }
            if r[1] != 0xA1 && r[1] != 0xA2 {
                continue; // ainda o eco do pedido
            }
            return if r[8] > 100 { Reply::None } else { Reply::Battery(r[8], r[7] == 0x01) };
        }
    }
    Reply::None
}

pub fn poll(ctx: &mut Ctx) {
    for &(pid, _receiver, name, receiver_pid) in MODELS {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let mut colls = ctx.find(VID, pid, |c| c.usage_page() == 0xFFFF && c.feature_len() >= 65);
        colls.sort_by_key(|c| c.usage() != 0);
        for c in &colls {
            let Some(d) = ctx.open_feature_only(c) else { continue };
            let mut s = Status::new(format!("wlmouse:{}", hex4(pid)), name, Kind::Mouse);
            match query(&d) {
                Reply::None => continue,
                Reply::Asleep => s.asleep = true,
                Reply::Battery(p, chg) => {
                    s.percent = p as i32;
                    s.charging = chg;
                }
            }
            ctx.add(s, VID, pid);
            if receiver_pid != 0 {
                ctx.claim(VID, receiver_pid);
            }
            break;
        }
    }
}
