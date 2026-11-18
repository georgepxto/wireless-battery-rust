//! Leitor generico: qualquer dispositivo USB com o campo padrao de bateria do HID
//! (ver `crate::hid::battery_strength`). Roda por ultimo, so no que nenhum leitor especifico tratou.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), generic.cpp.

use super::{hex4, Ctx, Status};
use crate::device::Kind;

/// Fabricantes que este leitor nao deve listar: Microsoft (controles Xbox, lidos pelo
/// Windows.Gaming.Input) e Machenike (o dongle informa uma bateria falsa, sempre cheia).
const SKIP_VIDS: &[u16] = &[0x045E, 0x2345];

pub fn poll(ctx: &mut Ctx) {
    let colls: Vec<_> = ctx
        .api
        .device_list()
        .filter(|c| !ctx.is_claimed(c.vendor_id(), c.product_id()) && c.has_value_caps() && !SKIP_VIDS.contains(&c.vendor_id()))
        .filter(|c| c.usage_page() != 0x84 && c.usage_page() != 0x85) // nobreaks / fontes
        // So USB: os HID Bluetooth ja aparecem pelo leitor Bluetooth com o nivel do Windows.
        .filter(|c| c.path().to_string_lossy().to_ascii_lowercase().contains("#vid_"))
        .cloned()
        .collect();
    for c in colls {
        let (vid, pid) = (c.vendor_id(), c.product_id());
        if ctx.is_claimed(vid, pid) {
            continue;
        }
        let Some(pct) = crate::hid::battery_strength(&c) else { continue };
        let name = c.product_string().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Wireless device").to_string();
        let kind = match ctx.kind_from_collections(vid, pid) {
            Kind::Other => Kind::from_name(&name),
            k => k,
        };
        let mut s = Status::new(format!("hid:{}:{}", hex4(vid), hex4(pid)), name, kind);
        s.percent = pct;
        ctx.add(s, vid, pid);
    }
}
