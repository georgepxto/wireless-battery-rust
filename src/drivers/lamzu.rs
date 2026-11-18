//! Mouses LAMZU nos VIDs proprios 373E e 37B0 (Maya, Tachi, Inca, Paro, Thorn, Atlantis, Orcus...).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), lamzu.cpp.
//!
//! Do driver web Aurora da LAMZU: SET_FEATURE report 0 [00][00][02][02][00][83]; ~100 ms depois
//! GET_FEATURE report 0: [A1][..][..][02][..][83][carregando][porcentagem] depois do byte do id
//! (aceita com ou sem esse byte). Carregando 1 = no cabo; 0% no dongle = mouse desligado.

use super::{hex4, sleep_ms, Ctx, Status};
use crate::device::Kind;

fn model_name(vid: u16, pid: u16) -> Option<&'static str> {
    const MODELS: &[(u16, [u16; 3], &str)] = &[
        (0x373E, [0x001C, 0x001D, 0x001E], "LAMZU Maya X"),
        (0x373E, [0x0016, 0, 0], "LAMZU Maya X V2"),
        (0x37B0, [0x001F, 0, 0], "LAMZU Maya X V2"),
        (0x37B0, [0x0005, 0x000B, 0x000C], "LAMZU Tachi"),
        (0x37B0, [0x001C, 0x001E, 0], "LAMZU Tachi Lite"),
        (0x37B0, [0x0009, 0x000F, 0x0010], "LAMZU Inca"),
        (0x37B0, [0x0011, 0x0013, 0x0015], "LAMZU Maya"),
        (0x37B0, [0x0007, 0x000D, 0x000E], "LAMZU Paro"),
        (0x37B0, [0x0017, 0x0019, 0x001B], "LAMZU Thorn"),
        (0x37B0, [0x0021, 0x0003, 0x0023], "LAMZU Thorn V2"),
        (0x37B0, [0x0030, 0x0032, 0x0040], "LAMZU Thorn V2"),
        (0x37B0, [0x0025, 0x0027, 0], "LAMZU Atlantis OG Champion"),
        (0x37B0, [0x0028, 0x002B, 0x002A], "LAMZU Atlantis Mini"),
        (0x37B0, [0x003E, 0x003F, 0], "LAMZU Atlantis Mini"),
        (0x37B0, [0x002C, 0x0052, 0], "LAMZU Maya M"),
        (0x37B0, [0x003A, 0x003B, 0x0044], "LAMZU Maya X"),
        (0x37B0, [0x003C, 0x0050, 0], "LAMZU Maya"),
        (0x37B0, [0x0042, 0x0056, 0], "LAMZU Orcus"),
        (0x37B0, [0x0046, 0x0054, 0], "LAMZU Mini"),
    ];
    MODELS.iter().find(|(v, pids, _)| *v == vid && pids.iter().any(|p| *p != 0 && *p == pid)).map(|m| m.2)
}

fn is_cable(vid: u16, pid: u16) -> bool {
    if vid == 0x373E {
        return pid == 0x001C;
    }
    matches!(
        pid,
        0x0005 | 0x0007 | 0x0009 | 0x0011 | 0x0017 | 0x001C | 0x001F | 0x0021 | 0x0025 | 0x0028 | 0x002C | 0x0030 | 0x0034 | 0x0038 | 0x003A | 0x003C
            | 0x003E | 0x0040 | 0x0042 | 0x0044 | 0x0046 | 0x0050 | 0x0052 | 0x0056
    )
}

/// (carregando, porcentagem) com ou sem o byte do report id.
pub fn parse(r: &[u8]) -> Option<(u8, u8)> {
    if r.len() < 10 {
        return None;
    }
    if r[1] == 0xA1 && r[4] == 0x02 && r[6] == 0x83 {
        Some((r[7], r[8]))
    } else if r[0] == 0xA1 && r[3] == 0x02 && r[5] == 0x83 {
        Some((r[6], r[7]))
    } else {
        None
    }
}

pub fn poll(ctx: &mut Ctx) {
    for vendor in [0x373Eu16, 0x37B0] {
        for (vid, pid) in ctx.products(vendor) {
            for c in ctx.find(vid, pid, |c| c.feature_len() == 65) {
                let Some(d) = ctx.open_feature_only(&c) else { continue };
                let mut req = [0u8; 65];
                req[3] = 0x02;
                req[4] = 0x02;
                req[6] = 0x83;
                if d.send_feature_report(&req).is_err() {
                    continue;
                }
                sleep_ms(100);
                let Some((chg, pct)) = d.get_feature(0).and_then(|r| parse(&r)) else { continue };
                if pct > 100 {
                    continue;
                }
                let cable = is_cable(vid, pid);
                let mut s = Status::new(format!("lamzu:{}:{}", hex4(vid), hex4(pid)), model_name(vid, pid).unwrap_or("LAMZU wireless mouse"), Kind::Mouse);
                s.connected = pct > 0 || cable;
                if pct > 0 {
                    s.percent = pct as i32;
                }
                s.charging = chg == 1;
                ctx.add(s, vid, pid);
                break;
            }
        }
    }
}
