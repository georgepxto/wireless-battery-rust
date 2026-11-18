//! Mouses Cooler Master sem fio (VID 2516): MM712 (cabo 0169) e MM731 (cabo 0167), ambos no
//! dongle 01B7. Interface 3, pagina FF0A, sem report id.
//! Pedido [46][81][00] -> [47][81][modelo] ... [10] %, [11] carregando, [12] link (0 cabo, 1 dongle).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), coolermaster.cpp; protocolo de
//! syntactic-spice/coolermaster-usb-protocol e riVFerd/MM731-Controller.

use super::{hex4, read_until, Ctx, Status};
use crate::device::{Kind, State};

const DONGLE: u16 = 0x01B7;
const MM712_CABLE: u16 = 0x0169;
const MM731_CABLE: u16 = 0x0167;

fn model_name(pid: u16, model: u8) -> &'static str {
    if pid == MM712_CABLE || (pid == DONGLE && model == 0x50) {
        "Cooler Master MM712"
    } else if pid == MM731_CABLE || (pid == DONGLE && model == 0x70) {
        "Cooler Master MM731"
    } else {
        "Cooler Master wireless mouse"
    }
}

pub fn poll(ctx: &mut Ctx) {
    // Em ordem: os cabos vem antes do dongle.
    for (vid, pid) in ctx.products(0x2516) {
        if ![DONGLE, MM712_CABLE, MM731_CABLE].contains(&pid) {
            continue;
        }
        for c in ctx.find(vid, pid, |c| c.interface_number() == 3 && c.usage_page() == 0xFF0A) {
            let Some(d) = ctx.open(&c) else { continue };
            d.flush();
            if d.write(&[0x00, 0x46, 0x81, 0x00]).is_err() {
                continue;
            }
            let Some(r) = read_until(&d, 500, |r| (r.len() >= 13 && r[0] == 0x47 && r[1] == 0x81).then(|| r.to_vec())) else { continue };
            let name = model_name(pid, r[2]);
            let mut s = Status::new(format!("coolermaster:{}", hex4(pid)), name, Kind::Mouse);
            if (1..=100).contains(&r[10]) {
                s.percent = r[10] as i32;
            }
            s.charging = r[11] != 0;
            // Com cabo e dongle ligados o mouse aparece duas vezes: fica um so.
            let mut dup = false;
            for o in ctx.out.iter_mut().filter(|o| o.name.as_deref() == Some(name)) {
                dup = true;
                if let State::Battery { charging, .. } = &mut o.state {
                    *charging |= s.charging;
                }
            }
            if dup {
                ctx.claim(vid, pid);
            } else {
                ctx.add(s, vid, pid);
            }
            break;
        }
    }
}
