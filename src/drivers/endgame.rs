//! Endgame Gear 4K (OP1w 4k v1/v2, XM2w 4k v1/v2) no dongle 4K 3367:1970.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), endgame.cpp.
//!
//! Colecao FF01:0002, feature report A1 (64 bytes): [A1][cmd][alvo][tam][00 ...].
//! SET_FEATURE, ~150 ms, GET_FEATURE A1. Byte 1 = estado: 01 ok, 03 ocupado, 08/11 dormindo.
//! B4 = bateria (byte 16), 0E = info do mouse (VID 3367 + PID), 0F = sondagem, so apos falha.
//! Fontes: johanneszab/vole, Janlhin/Endgame_Gear_Web_Configurator.

use super::{hex4, sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;
use std::collections::HashMap;
use std::sync::Mutex;

const VID: u16 = 0x3367;
const DONGLE: u16 = 0x1970;

/// O mouse so se identifica acordado: guarda o ultimo nome por dongle.
static LAST_NAME: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);

fn mouse_name(pid: u16) -> Option<&'static str> {
    Some(match pid {
        0x1972 => "Endgame Gear OP1w 4k",
        0x1984 => "Endgame Gear OP1w 4k v2",
        0x1968 => "Endgame Gear XM2w 4k",
        0x1982 => "Endgame Gear XM2w 4k v2",
        _ => return None,
    })
}

/// Envia um comando e devolve (estado, resposta); estado -1 = erro de E/S.
fn command(d: &HidDevice, cmd: u8, target: u8) -> (i32, Vec<u8>) {
    let mut req = [0u8; 64];
    req[0] = 0xA1;
    req[1] = cmd;
    req[2] = target;
    if d.send_feature_report(&req).is_err() {
        return (-1, Vec::new());
    }
    for attempt in 0..3u64 {
        sleep_ms(150 + attempt * 100);
        let Some(r) = d.get_feature(0xA1).filter(|r| r.len() >= 20 && r[0] == 0xA1) else { return (-1, Vec::new()) };
        if r[1] != 0x03 {
            return (r[1] as i32, r);
        }
    }
    (0x03, Vec::new())
}

pub fn poll(ctx: &mut Ctx) {
    if ctx.is_claimed(VID, DONGLE) {
        return;
    }
    for c in ctx.find(VID, DONGLE, |c| c.usage_page() == 0xFF01 && c.usage() == 0x0002 && c.feature_len() >= 64) {
        let Some(d) = ctx.open_feature_only(&c) else { continue };
        let (mut st, mut r) = command(&d, 0xB4, 0x00);
        if !matches!(st, 0x01 | 0x08 | 0x11) {
            // Sem resposta: sonda o dongle e o mouse uma vez e tenta de novo.
            command(&d, 0x0F, 0x01);
            command(&d, 0x0F, 0x0F);
            (st, r) = command(&d, 0xB4, 0x00);
        }
        let awake = st == 0x01 && r.get(16).is_some_and(|p| *p <= 100);
        let asleep = st == 0x08 || st == 0x11;
        if !awake && !asleep {
            continue;
        }
        let path = c.path().to_string_lossy().into_owned();
        let mut names = LAST_NAME.lock().unwrap();
        let names = names.get_or_insert_with(HashMap::new);
        let mut s = Status::new(format!("endgame:{}", hex4(DONGLE)), "", Kind::Mouse);
        s.asleep = asleep;
        if awake {
            s.percent = r[16] as i32;
            let (ist, info) = command(&d, 0x0E, 0x00);
            if ist == 0x01 && info[16] == 0x67 && info[17] == 0x33 {
                if let Some(n) = mouse_name(u16::from_le_bytes([info[18], info[19]])) {
                    names.insert(path.clone(), n);
                }
            }
        }
        s.name = names.get(&path).copied().unwrap_or("Endgame Gear 4k wireless mouse").into();
        ctx.add(s, VID, DONGLE);
        break;
    }
}
