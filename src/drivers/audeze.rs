//! Headsets Audeze Maxwell (VID 3329) no dongle USB ou no cabo USB-C. Colecao FF13:0001: output 06
//! de 62 bytes, resposta input 07 lida com GetInputReport; a resposta e um buffer das ultimas
//! respostas, onde se procura o marcador D6 0C 00 00 <%>. Sem headset ligado o dongle se chama
//! "... Dongle" e repete um valor velho, entao nao e consultado. Logo depois de ligar o headset
//! informa 0% por um tempo.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), audeze.cpp; protocolo do
//! HeyOkay/HaloBattery (MIT) e do HeadsetControl.

use super::{sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::DeviceInfo;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

const VID: u16 = 0x3329;
/// (pid, cabo, nome); cabos primeiro: a conexao direta vence.
const MODELS: &[(u16, bool, &str)] = &[
    (0x4B1A, true, "Audeze Maxwell"),
    (0x4B1E, true, "Audeze Maxwell"),
    (0x4B19, false, "Audeze Maxwell"),
    (0x4B18, false, "Audeze Maxwell"),
    (0x4B29, false, "Audeze Maxwell 2"),
    (0x4B28, false, "Audeze Maxwell 2"),
];

fn find_level(b: &[u8]) -> Option<i32> {
    b.windows(5).find(|w| w[0] == 0xD6 && w[1] == 0x0C && w[2] == 0 && w[3] == 0 && w[4] <= 100).map(|w| w[4] as i32)
}

fn read_level(ctx: &Ctx, c: &DeviceInfo) -> Option<i32> {
    let d = ctx.open(c)?;
    let mut req = vec![0u8; 62];
    req[..9].copy_from_slice(&[0x06, 0x07, 0x80, 0x05, 0x5A, 0x03, 0x00, 0xD6, 0x0C]);
    let mut level = None;
    for _ in 0..2 {
        d.write(&req).ok()?;
        for i in 0..4 {
            sleep_ms(60);
            if let Some(l) = d.get_input_report(0x07).and_then(|r| find_level(&r)) {
                level = Some(l);
            }
            if i >= 2 && level.is_some() {
                return level;
            }
        }
        if level.is_some() {
            return level;
        }
    }
    level
}

static FIRST_SEEN: Mutex<Option<HashMap<u16, Instant>>> = Mutex::new(None);

pub fn poll(ctx: &mut Ctx) {
    let mut guard = FIRST_SEEN.lock().unwrap();
    let first_seen = guard.get_or_insert_with(HashMap::new);
    for &(pid, cable, name) in MODELS {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        let colls = ctx.find(VID, pid, |c| c.usage_page() == 0xFF13 && c.usage() == 1 && c.input_len() > 0);
        let Some(c) = colls.first() else {
            first_seen.remove(&pid);
            continue;
        };
        if !cable && c.product_string().unwrap_or("").to_lowercase().ends_with("dongle") {
            ctx.claim(VID, pid); // sem headset ligado: nada a ler (e o valor guardado e velho)
            first_seen.remove(&pid);
            continue;
        }
        let level = read_level(ctx, c);
        let since = *first_seen.entry(pid).or_insert_with(Instant::now);
        let mut s = Status::new("audeze:maxwell", name, Kind::Headset); // dongle e cabo sao um headset so
        match level {
            Some(l) if !(l == 0 && since.elapsed().as_secs() < 90) => {
                s.percent = l;
                s.charging = cable;
            }
            _ => s.asleep = true,
        }
        ctx.add(s, VID, pid);
        for &(o, _, _) in MODELS {
            ctx.claim(VID, o);
        }
        return;
    }
}
