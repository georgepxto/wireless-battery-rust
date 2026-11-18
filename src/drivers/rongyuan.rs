//! Receptores Rongyuan 2.4 GHz (Attack Shark, Ajazz, alguns MSI e outros): um feature report
//! devolve o teclado e o mouse. Pedido [00 F7 00 ...] na interface 2, colecao FFFF:0002;
//! resposta [00][ok][teclado %][mouse %][teclado offline][mouse offline].
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), rongyuan.cpp; lista de receptores
//! de qu1ncky/BatteryTray (MIT), tirada do driver web oficial da ATTACK SHARK.

use super::{hex4, sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const RECEIVERS: &[u32] = &[
    0x0DB0197E, 0x0DB08E9A, 0x145F0376, 0x145F038D, 0x14A5C045, 0x25AA2008, 0x25AA4008, 0x25AA4018, 0x31514011, 0x31514014, 0x31514017,
    0x31514021, 0x31514027, 0x3151402D, 0x31514038, 0x31515006, 0x31515007, 0x31515008, 0x31515020, 0x31515026, 0x3151502B, 0x31515031,
    0x31515037, 0x31515038, 0x3151503D, 0x3151503F, 0x31515040, 0x31515042, 0x31515045, 0x3151504B, 0x3151504E, 0x31515050, 0x31515052,
    0x31515055, 0x3151505A, 0x342DE3E0, 0x342DE3E1, 0x342DE3E2, 0x342DE3E6, 0x342DE3E7, 0x342DE3E8, 0x342DE3F5, 0x342DE3F6, 0x342DE3F7,
    0x342DE3FB, 0x342DE3FC, 0x342DE3FD, 0x347AA302, 0x347AA305, 0x374AA205, 0x374AA217, 0x374AA220, 0x374AA223, 0x374AA226, 0x374AA229,
    0x374AA234, 0x374AA237, 0x374AA251, 0x374AA263, 0x374AA268, 0x374AA311, 0x374AA314, 0x374AA319, 0x374AA322, 0x374AA329, 0x374AA355,
    0x379A1804, 0x379A1807, 0x38A90002, 0x38EE0000, 0x38EE0002, 0x38EE0039, 0x391D1A04, 0x391D1A08, 0x391D1A0E, 0x391D1A13, 0x391D1A25,
    0x391D1A2B, 0x391D1A34, 0x391D2017, 0x391D2A05,
];

fn query(d: &HidDevice) -> Option<Vec<u8>> {
    for attempt in 0..3 {
        if attempt > 0 {
            sleep_ms(100);
        }
        let mut cmd = vec![0u8; d.feature_len()];
        cmd[1] = 0xF7;
        d.send_feature_report(&cmd).ok()?;
        sleep_ms(20);
        if let Some(r) = d.get_feature(0) {
            if r.len() >= 7 && r[2..7].iter().any(|b| *b != 0) {
                return Some(r);
            }
        }
    }
    None
}

pub fn poll(ctx: &mut Ctx) {
    let mut products: Vec<(u16, u16)> = ctx
        .api
        .device_list()
        .map(|c| (c.vendor_id(), c.product_id()))
        .filter(|(v, p)| RECEIVERS.contains(&(((*v as u32) << 16) | *p as u32)))
        .collect();
    products.sort_unstable();
    products.dedup();
    for (vid, pid) in products {
        if ctx.is_claimed(vid, pid) {
            continue;
        }
        for c in ctx.find(vid, pid, |c| c.interface_number() == 2 && c.usage_page() == 0xFFFF && c.usage() == 0x0002 && c.feature_len() >= 7) {
            let Some(d) = ctx.open_feature_only(&c) else { continue };
            let Some(r) = query(&d) else { continue };
            let product = c.product_string().unwrap_or("");
            let generic = product.is_empty() || product.contains("Receiver") || product.contains("2.4G");
            for keyboard in [true, false] {
                if r[if keyboard { 4 } else { 5 }] != 0 {
                    continue; // nada conectado nesse slot
                }
                let level = r[if keyboard { 2 } else { 3 }];
                let name = if vid == 0x3151 && pid == 0x402D && !keyboard {
                    "Attack Shark R3".to_string()
                } else if generic {
                    if keyboard { "Wireless keyboard" } else { "Wireless mouse" }.to_string()
                } else {
                    product.to_string()
                };
                let key = format!("rongyuan:{}:{}:{}", hex4(vid), hex4(pid), if keyboard { "kb" } else { "ms" });
                let mut s = Status::new(key, name, if keyboard { Kind::Keyboard } else { Kind::Mouse });
                if (1..=100).contains(&level) {
                    s.percent = level as i32;
                }
                ctx.out.push(s.into_observation());
            }
            ctx.claim(vid, pid);
            break;
        }
    }
}
