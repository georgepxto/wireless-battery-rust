//! Glorious, Hitscan, G-Wolves e WLMouse YING: protocolo do mesmo fornecedor.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), glorious.cpp.
//!
//! Pedido: feature report de 64 bytes (id 0), bytes [3]=02 [4]=02 [6]=83.
//! Resposta: [1]=estado, [4]=02, [6]=83, [7]=carregando (1), [8]=porcentagem.
//! Estado A1 = acordado, A4 = dormindo, A0 = acordando.
//! Fontes: ikajdan/glorious-indicator, louis4craft/glorious-ctl, configuradores web da Hitscan,
//! G-Wolves (mouse.xyz) e WLMouse.

use super::{clamp100, hex4, sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::DeviceInfo;

/// Nomes de modelo G-Wolves (lista oficial em mouse.xyz/Config/env-models.json).
pub fn gwolves_name(pid: u16) -> &'static str {
    match pid {
        0x3808 | 0x3817 => "G-Wolves HTM Plus",
        0x6808 | 0x6817 => "G-Wolves HSK Pro 2.0",
        0x5608 | 0x5617 | 0x5618 | 0x5619 => "G-Wolves HTXU",
        0x3608 | 0x3617 | 0x3619 => "G-Wolves Fenrir Pro",
        0x3908 | 0x3917 => "G-Wolves VUK",
        0x7904 | 0x7913 => "G-Wolves HT-S2",
        0x3708 | 0x3717 => "G-Wolves Fenrir Max",
        0x5308 | 0x5317 => "G-Wolves HTS Ultra",
        0x7704 | 0x7713 => "G-Wolves HTR",
        0x3508 | 0x3517 => "G-Wolves Fenrir",
        0x7908 | 0x7917 => "G-Wolves HT-S2 Pro",
        0x5808 | 0x5817 => "G-Wolves HSK Pro",
        0x2708 | 0x2717 | 0x2719 => "G-Wolves HTX Mini",
        0x5408 | 0x5417 | 0x5418 | 0x5419 => "G-Wolves HTS Plus",
        0x5708 | 0x5717 => "G-Wolves HTX",
        0x5908 | 0x5917 => "G-Wolves HSK Plus",
        0x7204 | 0x7203 => "G-Wolves HSK Lite",
        0x7708 | 0x7717 => "G-Wolves HTR Pro",
        0x5804 | 0x5803 => "G-Wolves HSK Pro ACE",
        0x5404 | 0x5403 => "G-Wolves HTS Plus ACE",
        0x5704 | 0x5703 => "G-Wolves HTX ACE",
        0x5904 | 0x5903 => "G-Wolves HSK Plus ACE",
        0x4718 | 0x4719 => "G-Wolves Lycan",
        0x4219 => "G-Wolves WARG",
        0x5219 => "G-Wolves HTS Plus Pro",
        0x3519 => "G-Wolves Fenrir Asym",
        0x2619 => "G-Wolves HTX Mini Asym",
        0x1619 => "G-Wolves Fenrir Max Asym",
        0x2119 => "G-Wolves VUK Mini",
        _ => "G-Wolves wireless mouse",
    }
}

pub fn gwolves_is_cable(pid: u16) -> bool {
    const WIRED: &[u16] = &[
        0x1619, 0x2119, 0x2619, 0x2708, 0x2719, 0x3508, 0x3519, 0x3608, 0x3619, 0x3708, 0x3808, 0x3908, 0x4219, 0x4718, 0x4719, 0x5219,
        0x5308, 0x5404, 0x5408, 0x5418, 0x5419, 0x5608, 0x5618, 0x5619, 0x5704, 0x5708, 0x5804, 0x5808, 0x5904, 0x5908, 0x6808, 0x7204,
        0x7704, 0x7708, 0x7904, 0x7908,
    ];
    WIRED.contains(&pid)
}

fn glorious_name(pid: u16) -> Option<&'static str> {
    Some(match pid {
        0x2022 | 0x2011 => "Glorious Model O Wireless",
        0x2023 | 0x2012 => "Glorious Model D Wireless",
        0x2024 | 0x2013 => "Glorious Model O- Wireless",
        0x2025 | 0x2014 => "Glorious Model D- Wireless",
        0x2033 => "Glorious Model O 2 Wireless",
        0x2034 | 0x2036 | 0x201A => "Glorious Model D 2 PRO",
        // Outra marca no mesmo chip Sinowealth (ex.: teclados Redragon).
        _ => return None,
    })
}

fn glorious_cable(pid: u16) -> bool {
    matches!(pid, 0x2011 | 0x2012 | 0x2013 | 0x2014 | 0x201A)
}

fn hitscan_name(pid: u16) -> Option<&'static str> {
    Some(match pid {
        0x0136 => "Hitscan Hyperlight X",
        0x0194 => "Hitscan Hyperlight N",
        _ => "Hitscan Hyperlight",
    })
}

struct Vendor {
    vid: u16,
    prefix: &'static str,
    reply_delay: u64,
    name: fn(u16) -> Option<&'static str>,
    cable: fn(u16) -> bool,
    matches: fn(&DeviceInfo) -> bool,
}

const VENDORS: &[Vendor] = &[
    Vendor { vid: 0x258A, prefix: "glorious:", reply_delay: 50, name: glorious_name, cable: glorious_cable, matches: |_| true },
    Vendor { vid: 0x3770, prefix: "hitscan:", reply_delay: 200, name: hitscan_name, cable: |p| p == 0x0136 || p == 0x0194, matches: |_| true },
    Vendor { vid: 0x33E4, prefix: "gwolves:", reply_delay: 100, name: |p| Some(gwolves_name(p)), cable: gwolves_is_cable, matches: |_| true },
    Vendor {
        vid: 0x36A7,
        prefix: "wlmouse:",
        reply_delay: 100,
        name: |_| Some("WLMouse YING"),
        cable: |_| false,
        matches: |c| c.product_id() == 0xA874 && c.usage_page() == 0xFFFF && c.usage() == 0,
    },
];

/// 258A e o VID da Sinowealth, usado por muitas marcas: para produto desconhecido, usa o nome
/// do aparelho e descobre se e teclado ou mouse.
fn sinowealth_kind(ctx: &Ctx, vid: u16, pid: u16, name: &str) -> Kind {
    let k = Kind::from_name(name);
    if k != Kind::Other {
        return k;
    }
    let keyboards = ctx.api.device_list().filter(|c| c.vendor_id() == vid && c.product_id() == pid && c.usage_page() == 0x01 && c.usage() == 0x06).count();
    if keyboards >= 2 {
        return Kind::Keyboard; // teclados expoem colecoes boot + NKRO
    }
    ctx.kind_from_collections(vid, pid)
}

pub fn poll(ctx: &mut Ctx) {
    for v in VENDORS {
        for (vid, pid) in ctx.products(v.vid) {
            for c in ctx.find(vid, pid, |c| c.feature_len() == 65 && (v.matches)(c)) {
                let Some(d) = ctx.open_feature_only(&c) else { continue };
                let mut reply = None;
                for attempt in 0..2 {
                    let mut w = [0u8; 65];
                    w[3] = 0x02;
                    w[4] = 0x02;
                    w[6] = 0x83;
                    if d.send_feature_report(&w).is_err() {
                        break;
                    }
                    sleep_ms(v.reply_delay);
                    let Some(r) = d.get_feature(0) else { break };
                    if r.len() < 9 || r[6] != 0x83 || r[1] & 0xF0 != 0xA0 {
                        break; // nao e este protocolo
                    }
                    if r[1] == 0xA0 && attempt == 0 {
                        sleep_ms(400); // acordando: pergunta de novo
                        continue;
                    }
                    reply = Some(r);
                    break;
                }
                let Some(r) = reply else { continue };
                let (name, kind) = match (v.name)(pid) {
                    Some(n) => (n.to_string(), Kind::Mouse),
                    None => {
                        let n = c.product_string().unwrap_or("Wireless device").to_string();
                        let k = sinowealth_kind(ctx, vid, pid, &n);
                        (n, k)
                    }
                };
                let mut s = Status::new(format!("{}{}", v.prefix, hex4(pid)), name, kind);
                s.charging = (v.cable)(pid) || r[7] == 0x01;
                if r[1] == 0xA1 && r[8] <= 100 {
                    s.percent = clamp100(r[8].max(1) as i32);
                } else {
                    s.asleep = true;
                }
                ctx.add(s, vid, pid);
                break;
            }
        }
    }
}
