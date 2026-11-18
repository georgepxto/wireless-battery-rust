//! Mouses MCHOSE: L7 / M7 / A7 / K7 (VID 5253) e G3 V2 (VID 3837).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), mchose.cpp e mchose_g3.cpp.
//!
//! VID 5253 (protocolo de alexfrih/mchose-linux, confirmado num L7 Ultra):
//! - Feature report 0x12 de 65 bytes na colecao FF01:0001, payload [06 00 00 ...] com TODOS
//!   os bytes invertidos (^ 0xFF), inclusive o preenchimento.
//! - Resposta lida de volta: [12][F9][payload invertido] = vid:u16, modelo:u16, firmware:u32,
//!   flags:u8 (09 no receptor, 00 no cabo), nivel:u8, carregando:u8.
//! - Mouse dormindo: o receptor ecoa o comando com payload zerado.
//!
//! G3 V2 (protocolo de Jstdi001/MCHOSE-Monitor, MIT): output [00][55 30 A5 0B 2E 01 01 01],
//! resposta [AA 30 .. .. .. .. .. .. nivel carregando].

use super::{clamp100, hex4, read_until, sleep_ms, Ctx, Status};
use crate::device::Kind;

const VID: u16 = 0x5253;
const REPORT: u8 = 0x12;
const CMD_STATUS: u8 = 0x06;

/// 1 = leitura, 0 = eco com payload vazio (dormindo), -1 = outra coisa.
pub fn parse_status(r: &[u8]) -> (i32, u8, u8, u8) {
    if r.len() < 13 || r[0] != REPORT || r[1] != CMD_STATUS ^ 0xFF {
        return (-1, 0, 0, 0);
    }
    let p: Vec<u8> = r[2..13].iter().map(|b| b ^ 0xFF).collect();
    if p.iter().all(|b| *b == 0) {
        return (0, 0, 0, 0);
    }
    if u16::from_le_bytes([p[0], p[1]]) != VID || p[9] > 100 {
        return (-1, 0, 0, 0);
    }
    (1, p[9], p[10], p[8])
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(VID) {
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFF01 && c.usage() == 0x0001 && c.feature_len() >= 65) {
            let Some(d) = ctx.open_feature_only(&c) else { continue };
            let mut req = vec![0xFFu8; c.feature_len() as usize];
            req[0] = REPORT;
            req[1] = CMD_STATUS ^ 0xFF;
            let (mut state, mut level, mut charging, mut flags) = (-1, 0u8, 0u8, 0u8);
            for attempt in 0..5 {
                if state == 1 {
                    break;
                }
                if attempt > 0 {
                    sleep_ms(100);
                }
                if d.send_feature_report(&req).is_err() {
                    break;
                }
                sleep_ms(120);
                let Some(r) = d.get_feature(REPORT) else { continue };
                let (st, l, ch, fl) = parse_status(&r);
                if st > state {
                    (state, level, charging, flags) = (st, l, ch, fl);
                }
            }
            if state < 0 {
                continue;
            }
            let name = c.product_string().unwrap_or("MCHOSE mouse").to_string();
            let mut s = Status::new(format!("mchose:{}", hex4(pid)), name, Kind::Mouse);
            if state == 1 {
                s.percent = clamp100(level as i32);
                s.charging = charging != 0;
                let _ = flags;
            } else {
                s.asleep = true;
            }
            ctx.add(s, vid, pid);
            break;
        }
    }
}

const G3_VID: u16 = 0x3837;
const G3_WIRED: u16 = 0x418C;
const G3_WIRELESS: u16 = 0x4245;

pub fn poll_g3(ctx: &mut Ctx) {
    // Cabo antes do 2.4 GHz quando os dois estao ligados.
    let mut found = false;
    for pid in [G3_WIRED, G3_WIRELESS] {
        if ctx.is_claimed(G3_VID, pid) {
            continue;
        }
        for c in ctx.find(G3_VID, pid, |c| c.usage_page() == 0xFF01 && c.usage() == 0x0010 && c.output_len() >= 65 && c.input_len() >= 65) {
            if found {
                ctx.claim(G3_VID, pid);
                break;
            }
            let Some(d) = ctx.open(&c) else { continue };
            d.flush();
            if d.write(&[0x00, 0x55, 0x30, 0xA5, 0x0B, 0x2E, 0x01, 0x01, 0x01, 0x00, 0x00]).is_err() {
                continue;
            }
            let reply = read_until(&d, 1000, |r| (r.len() >= 10 && r[0] == 0xAA && r[1] == 0x30).then(|| (r[8], r[9])));
            let Some((pct, chg)) = reply else { continue };
            if !(1..=100).contains(&pct) {
                continue;
            }
            let name = c.product_string().unwrap_or("MCHOSE G3 V2").to_string();
            let mut s = Status::new("mchose:g3v2", name, Kind::Mouse);
            s.percent = pct as i32;
            s.charging = chg == 1;
            ctx.add(s, G3_VID, pid);
            found = true;
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inverted_status() {
        // vid 5253, modelo 1020, fw 0, flags 09, nivel 77, carregando 0 -> invertido.
        let payload = [0x53, 0x52, 0x20, 0x10, 0, 0, 0, 0, 0x09, 77, 0];
        let mut r = vec![REPORT, CMD_STATUS ^ 0xFF];
        r.extend(payload.iter().map(|b| b ^ 0xFF));
        assert_eq!(parse_status(&r), (1, 77, 0, 9));
        let mut asleep = vec![REPORT, CMD_STATUS ^ 0xFF];
        asleep.extend([0xFFu8; 11]);
        assert_eq!(parse_status(&asleep).0, 0);
    }
}
