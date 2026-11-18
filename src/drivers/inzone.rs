//! Headsets Sony INZONE H9 II (054C:0FA8) e H5 (0EBF) e o dongle dos INZONE Buds (0EC2): camada de
//! comandos Sony tunelada por HID (report 02, 64 bytes) na colecao FF04.
//! Bateria = evento 04, tipo 01 (GET) para o headset (endereco 0x41), com checksum e id de transacao.
//! Headsets: [carregando][porcentagem], FF = desligado. Buds: [estado][%] esquerdo, direito, estojo;
//! FF = fone no estojo; com os dois no estojo o dongle nao responde.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), sony_inzone.cpp; protocolo de
//! Sapd/HeadsetControl e kinako19 / takamachi66 inzone-buds-battery-tray.

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;
use std::sync::atomic::{AtomicU16, Ordering};

static TID: AtomicU16 = AtomicU16::new(1);

struct Event {
    id: u8,
    kind: u8,
    tid: u16,
    payload: Vec<u8>,
}

fn parse_event(b: &[u8]) -> Option<Event> {
    if b.len() < 15 || b[0] != 0x02 {
        return None;
    }
    let len = b[1] as usize;
    if !(12..=62).contains(&len) || b.len() < len + 2 {
        return None;
    }
    if b[2] != 0x04 || b[3] != 0xFF || b[5] != 0x00 || b[6] != 0x96 || b[7] != 0xC3 || b[8] >> 4 != 0x1 {
        return None;
    }
    let sum = b[5..=len].iter().fold(0u8, |a, x| a.wrapping_add(*x));
    if sum != b[len + 1] {
        return None;
    }
    Some(Event { id: b[9], kind: b[10], tid: u16::from_le_bytes([b[11], b[12]]), payload: b[13..=len].to_vec() })
}

enum Reply {
    Ok(Vec<u8>),
    Silent,
    Failed,
}

fn get_battery(d: &HidDevice, min_payload: usize) -> Reply {
    // tid 1 e das notificacoes do proprio dongle.
    let mut tid = TID.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if tid <= 1 {
        tid = 2;
        TID.store(2, Ordering::Relaxed);
    }
    let mut req = [0u8; 64];
    req[..13].copy_from_slice(&[0x02, 12, 0x01, 0x00, 0xFC, 8, 0x96, 0xC3, 0x41, 0x04, 0x01, tid as u8, (tid >> 8) as u8]);
    req[13] = req[6..=12].iter().fold(0u8, |a, x| a.wrapping_add(*x));
    d.flush();
    if d.write(&req).is_err() {
        return Reply::Failed;
    }
    let end = Deadline::in_ms(1500);
    while !end.expired() {
        let Some(r) = d.read(end.left()).filter(|r| r.len() >= 15) else { return Reply::Silent };
        let Some(e) = parse_event(&r) else { continue };
        if e.id != 0x04 || e.tid != tid || e.payload.len() < min_payload || (e.kind != 0x10 && e.kind != 0xA0) {
            continue;
        }
        return Reply::Ok(e.payload);
    }
    Reply::Silent
}

/// Menor carga entre os fones fora do estojo, ou -1 com os dois no estojo.
fn lowest_bud_out(p: &[u8]) -> i32 {
    [p[1], p[3]].iter().filter(|v| **v <= 100).map(|v| *v as i32).min().unwrap_or(-1)
}

pub fn poll(ctx: &mut Ctx) {
    for (pid, name, buds, min_payload) in [(0x0FA8u16, "Sony INZONE H9 II", false, 2usize), (0x0EBF, "Sony INZONE H5", false, 2), (0x0EC2, "Sony INZONE Buds", true, 6)] {
        if ctx.is_claimed(0x054C, pid) {
            continue;
        }
        let kind = if buds { Kind::Earbuds } else { Kind::Headset };
        let (mut added, mut no_reply) = (false, false);
        for c in ctx.find(0x054C, pid, |c| c.usage_page() == 0xFF04 && c.output_len() >= 64) {
            let Some(d) = ctx.open(&c) else { continue };
            let p = match get_battery(&d, min_payload) {
                Reply::Ok(p) => p,
                Reply::Silent => {
                    no_reply = true;
                    continue;
                }
                Reply::Failed => continue,
            };
            let mut s = Status::new(format!("inzone:{}", hex4(pid)), name, kind);
            if buds {
                let lvl = lowest_bud_out(&p);
                s.connected = lvl >= 0;
                s.percent = lvl;
            } else {
                s.connected = p[1] != 0xFF;
                s.percent = if p[1] <= 100 { p[1] as i32 } else { -1 };
                s.charging = p[0] != 0;
            }
            ctx.add(s, 0x054C, pid);
            added = true;
            break;
        }
        // Buds: o dongle aceitou mas ninguem respondeu = os dois no estojo.
        if buds && no_reply && !added {
            let mut s = Status::new(format!("inzone:{}", hex4(pid)), name, kind);
            s.connected = false;
            ctx.add(s, 0x054C, pid);
        }
    }
}
