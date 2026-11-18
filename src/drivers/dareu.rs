//! Receptor DAREU EM901X (260D:1074).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), dareu.cpp.
//!
//! Feature 08 enviado na colecao FF02:0002; o input 09 chega na colecao irma FF01:0000.
//! So o comando 04 (bateria). Resposta: [09][04][estado=0]...[6=porcentagem][7=carregando].
//! Fonte: driver oficial EM901X AIM-WL; wushuai0802/myfinder-device-battery.

use super::{hex4, kysona, Ctx, Deadline, Status};
use crate::device::Kind;

pub fn poll(ctx: &mut Ctx) {
    let (vid, pid) = (0x260D, 0x1074);
    if ctx.is_claimed(vid, pid) {
        return;
    }
    let inputs = ctx.find(vid, pid, |c| c.interface_number() == 1 && c.usage_page() == 0xFF01 && c.usage() == 0 && c.input_len() == 17);
    let features = ctx.find(vid, pid, |c| c.interface_number() == 1 && c.usage_page() == 0xFF02 && c.usage() == 2 && c.feature_len() == 17);
    // Com mais de um receptor, nao pareia colecoes ao acaso.
    if inputs.len() != 1 || features.len() != 1 {
        return;
    }
    let (Some(input), Some(feature)) = (ctx.open(&inputs[0]), ctx.open_feature_only(&features[0])) else { return };
    input.flush();
    if feature.send_feature_report(&kysona::request(0x04)).is_err() {
        return;
    }
    let end = Deadline::in_ms(1000);
    for _ in 0..32 {
        if end.expired() {
            return;
        }
        let Some(r) = input.read(end.left()) else { return };
        if r.len() != 17 || r[0] != 0x09 || r[1] != 0x04 || r[2] != 0 || r[6] > 100 {
            continue;
        }
        let mut s = Status::new(format!("dareu:{}:{}", hex4(vid), hex4(pid)), "DAREU EM901X", Kind::Mouse);
        s.percent = r[6] as i32;
        s.charging = r[7] != 0;
        ctx.add(s, vid, pid);
        return;
    }
}
