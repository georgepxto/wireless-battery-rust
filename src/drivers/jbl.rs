//! Headsets JBL Quantum 520 / 650 / 850 / 950 (e X), VID 0ECB, protocolo "HIDV3" na colecao FF12.
//! Pedido (feature 88): [88][00 DD][01 00][01][00][02 00][feature lo][feature hi]; resposta (input 86)
//! [86][00 DD][01 00][01][00][len][status][feature][tam][valor]. Bateria = feature 000D, um byte:
//! bit 7 carregando, bits 0..6 porcentagem.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), jbl.cpp (de JBL QuantumENGINE).

use super::{hex4, read_until, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x0ECB;
const MODELS: &[(u16, &str)] = &[
    (0x2125, "JBL Quantum 650 Wireless"),
    (0x2126, "JBL Quantum 650 Wireless"),
    (0x213B, "JBL Quantum 650X Wireless"),
    (0x213D, "JBL Quantum 650X Wireless"),
    (0x2127, "JBL Quantum 950 Wireless"),
    (0x2128, "JBL Quantum 950 Wireless"),
    (0x213E, "JBL Quantum 950X Wireless"),
    (0x2140, "JBL Quantum 950X Wireless"),
    (0x218D, "JBL Quantum 520 Wireless"),
    (0x218E, "JBL Quantum 520 Wireless"),
    (0x21A7, "JBL Quantum 520X Wireless"),
    (0x21A9, "JBL Quantum 520X Wireless"),
    (0x219A, "JBL Quantum 850 Wireless"),
    (0x219B, "JBL Quantum 850 Wireless"),
];

/// Com o headset desligado o dongle nao responde nada.
fn get_battery(d: &HidDevice) -> Option<u8> {
    d.flush();
    d.send_feature_report(&[0x88, 0x00, 0xDD, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x0D, 0x00]).ok()?;
    let info = read_until(d, 800, |r| {
        if r.len() < 16 || r[0] != 0x86 || r[1] != 0x00 || r[2] != 0xDD || r[3] != 0x01 || r[4] != 0x00 || r[5] != 0x01 {
            return None;
        }
        if r[10] & 0x80 != 0 {
            return Some(None); // erro
        }
        if r[11] != 0x0D || r[12] != 0x00 || (r[13] == 0 && r[14] == 0) {
            return None;
        }
        Some(Some(r[15]))
    })??;
    (info & 0x7F <= 100).then_some(info)
}

pub fn poll(ctx: &mut Ctx) {
    for &(pid, name) in MODELS {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFF12 && c.feature_len() >= 11 && c.input_len() >= 16) {
            let Some(d) = ctx.open(&c) else { continue };
            let Some(info) = get_battery(&d) else { continue };
            let mut s = Status::new(format!("jbl:{}", hex4(pid)), name, Kind::Headset);
            s.percent = (info & 0x7F) as i32;
            s.charging = info & 0x80 != 0;
            ctx.add(s, VID, pid);
            break;
        }
    }
}
