//! Headsets Jabra atras de um dongle Jabra Link 380 / 390: protocolo GNP na colecao FF00 (report 05).
//! Consulta [05][dst][00][seq][40|len][classe][op]; resposta [05][00][src][seq][C0|len][classe][op][payload],
//! classe FE = NAK. Endereco 04 = o headset. Bateria = classe 12 / op 02, payload [flags][nivel],
//! flag bit 0 = carregando. Nome = classe 02 / op 00, payload [n][n bytes UTF-8].
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), jabra.cpp; protocolo de
//! Watchdog0x/jabridge.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

const VID: u16 = 0x0B0E;
const HEADSET: u8 = 0x04;
static SEQ: AtomicU8 = AtomicU8::new(0x20);
/// Nome do headset por dongle: pedido uma vez enquanto conectado.
static NAMES: Mutex<Option<HashMap<u16, String>>> = Mutex::new(None);

enum Reply {
    Ok(Vec<u8>),
    Nak,
    None,
}

fn query(d: &HidDevice, dst: u8, cls: u8, op: u8, ms: u64) -> Reply {
    let s = SEQ.fetch_add(1, Ordering::Relaxed);
    d.flush();
    if d.write(&[0x05, dst, 0x00, s, 0x46, cls, op]).is_err() {
        return Reply::None;
    }
    let end = Deadline::in_ms(ms);
    while !end.expired() {
        let Some(r) = d.read(end.left()) else { return Reply::None };
        if r.len() < 6 || r[0] != 0x05 || r[1] != 0x00 || r[2] != dst || r[3] != s || r[4] & 0xC0 != 0xC0 {
            continue;
        }
        if r[5] == 0xFE {
            return Reply::Nak;
        }
        let len = (r[4] & 0x3F) as usize;
        if r.len() < 7 || r[5] != cls || r[6] != op || len < 6 || len + 1 > r.len() {
            continue;
        }
        return Reply::Ok(r[7..=len].to_vec());
    }
    Reply::None
}

fn headset_name(d: &HidDevice) -> Option<String> {
    let Reply::Ok(p) = query(d, HEADSET, 0x02, 0x00, 400) else { return None };
    let n = *p.first()? as usize;
    if n == 0 || n + 1 > p.len() {
        return None;
    }
    Some(String::from_utf8_lossy(&p[1..=n]).trim().to_string()).filter(|s| !s.is_empty())
}

pub fn poll(ctx: &mut Ctx) {
    for pid in [0x24C7u16, 0x24C8, 0x24C9, 0x24CA, 0x24E9, 0x2E50, 0x2E51, 0x2E56, 0x2E57] {
        if ctx.is_claimed(VID, pid) {
            continue;
        }
        for c in ctx.find(VID, pid, |c| c.usage_page() == 0xFF00 && c.output_len() >= 64 && c.input_len() >= 64) {
            let Some(d) = ctx.open(&c) else { continue };
            let rep = query(&d, HEADSET, 0x12, 0x02, 700);
            let mut names = NAMES.lock().unwrap();
            let names = names.get_or_insert_with(HashMap::new);
            let mut s = Status::new(format!("jabra:{}", hex4(pid)), "Jabra headset", Kind::Headset);
            match rep {
                Reply::None => continue,
                Reply::Ok(p) if p.len() < 2 => continue,
                Reply::Ok(p) => {
                    s.percent = if p[1] <= 100 { p[1] as i32 } else { -1 };
                    s.charging = p[0] & 0x01 != 0;
                    names.entry(pid).or_insert_with(|| headset_name(&d).unwrap_or_else(|| "Jabra headset".into()));
                    s.name = names[&pid].clone();
                }
                Reply::Nak => {
                    s.connected = false; // dongle presente, headset desligado
                    if let Some(n) = names.remove(&pid) {
                        s.name = n;
                    }
                }
            }
            ctx.add(s, VID, pid);
            break;
        }
    }
}
