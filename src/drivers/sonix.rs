//! Teclados SONiX no dongle 2.4 GHz: AJAZZ AK820 MAX e AULA F108 Pro (05AC:024F), AJAZZ AK980 V2
//! (0C45:FEFE), AJAZZ AK820 Pro (0C45:FDFD) e os receptores 342D / 388D / 36B0 / 362D / 19F5 /
//! 0DB0 de outras marcas (EPOMAKER P75, RK R75, Redragon K742B, Lofree Flow 2, NuPhy Air75 V2,
//! NUT65, WEIKAV MAX102, Lemokey Link...). Colecao FF60:0061, reports de 33 bytes (id 0 + 32);
//! os tres primeiros ficam na interface 3, os demais em qualquer interface.
//! Pedido [20 01 00 ... soma]; resposta [20 01 00 nivel ...], reconhecida pelo eco do comando
//! (sem conferir a soma, como as ferramentas do AK820 Pro). So no modo 2.4G.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), sonix.cpp;
//! fonte do protocolo: donglover69/sonix-keyboard-studio.

use super::{hex4, sum8, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

/// (vid, pid, interface; -1 = qualquer). Os receptores de outras marcas vieram de pedidos no repo
/// do krisgfx9 e ainda nao foram testados em hardware.
const DONGLES: &[(u16, u16, i32)] = &[
    (0x05AC, 0x024F, 3),
    (0x0C45, 0xFEFE, 3),
    (0x0C45, 0xFDFD, 3),
    (0x342D, 0xE479, -1), // EPOMAKER P75
    (0x342D, 0xE485, -1), // RK Royal Kludge R75
    (0x342D, 0xE4BC, -1), // Redragon Arlokks K742B
    (0x342D, 0xE4C2, -1), // M1 V5 VIA
    (0x342D, 0xE407, -1), // Lucky65 V2
    (0x388D, 0x0003, -1), // Lofree Flow 2
    (0x0DB0, 0x0CA0, -1), // MSI FORGE TKL W FRIEREN EDITION
    (0x36B0, 0x30BD, -1), // Essential75
    (0x342D, 0xE51A, -1), // NUT65
    (0x342D, 0xE487, -1), // RK 2.4G Dongle
    (0x342D, 0xE537, -1), // WEIKAV MAX102
    (0x388D, 0x0002, -1), // Lofree Flow 2
    (0x19F5, 0x3245, -1), // NuPhy Air75 V2
    (0x362D, 0xD028, -1), // SKN Qinglong
    (0x362D, 0xD030, -1), // Lemokey Link
    (0x362D, 0xD101, -1), // Wireless Link
    (0x342D, 0xE49F, -1), // Spade65-QMK-2.4GHz
];
const LEN: usize = 32;

fn query_level(d: &HidDevice) -> Option<u8> {
    let mut req = [0u8; 1 + LEN];
    req[1] = 0x20;
    req[2] = 0x01;
    req[LEN] = sum8(&req[1..LEN]);
    d.flush();
    d.write(&req).ok()?;
    let end = Deadline::in_ms(500);
    loop {
        if end.expired() {
            return None;
        }
        let r = d.read(end.left())?;
        if r.len() < 4 || r[0] != 0x20 || r[1] != 0x01 || r[2] != 0x00 {
            continue;
        }
        return Some(r[3]);
    }
}

pub fn poll(ctx: &mut Ctx) {
    for &(vid, pid, iface) in DONGLES {
        if ctx.is_claimed(vid, pid) {
            continue;
        }
        for c in ctx.find(vid, pid, |c| {
            (iface < 0 || c.interface_number() == iface) && c.usage_page() == 0xFF60 && c.usage() == 0x0061 && c.output_len() as usize == 1 + LEN && c.input_len() as usize == 1 + LEN
        }) {
            let Some(d) = ctx.open(&c) else { continue };
            // O primeiro pedido pode ficar sem resposta enquanto o link 2.4 GHz acorda.
            let Some(level) = query_level(&d).or_else(|| query_level(&d)) else { continue };
            let name = c.product_string().unwrap_or("").trim_end_matches(" 2.4G").to_string();
            let mut s = Status::new(format!("sonix:{}:{}", hex4(vid), hex4(pid)), if name.is_empty() { "Wireless keyboard".into() } else { name }, Kind::Keyboard);
            s.connected = (1..=100).contains(&level);
            if s.connected {
                s.percent = level as i32;
            }
            ctx.add(s, vid, pid);
            break;
        }
    }
}
