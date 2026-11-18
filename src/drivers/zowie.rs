//! Mouses BenQ ZOWIE (VID 04A5). So os comandos de bateria, de leitura (outros comandos do EC-CW
//! reiniciam o receptor e desfazem o pareamento).
//! - EC-CW (receptor 8006, cabo 8005): output de 64 bytes [A5 03]; resposta [mV lo][mV hi][%].
//!   (zowie-battery, yleshchuk)
//! - DW (receptor 800A) e U2 (8008): output 08 [B4 15] na FF03, resposta input 09 na FF04,
//!   mV = ([1] * 256 + [2]) * 10, [4] == C8. (IgnacioBarb/zowie-battery-tray, MIT)
//!
//! Na base de carga so chega a tensao: carregando e deduzido pela tendencia da tensao.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), zowie.cpp.

use super::{hex4, interpolate, Ctx, Deadline, Status};
use crate::device::Kind;
use std::collections::HashMap;
use std::sync::Mutex;

/// Tensao -> % ancorada nos limites do LED do proprio mouse (10 / 25 / 50 %).
fn zowie_percent(mv: i32) -> i32 {
    interpolate(mv, &[4200, 4000, 3890, 3760, 3680, 3500], &[100, 75, 50, 25, 10, 0])
}

#[derive(Default)]
struct Trend {
    has: bool,
    charging: bool,
    peak: i32,
    trough: i32,
}

impl Trend {
    fn update(&mut self, mv: i32) -> bool {
        if !self.has {
            self.has = true;
            self.charging = mv >= 4250; // celula Li-ion em repouso nunca fica acima de ~4,2 V
            self.peak = mv;
            self.trough = mv;
        } else if self.charging {
            self.peak = self.peak.max(mv);
            if mv <= self.peak - 20 {
                self.charging = false;
                self.trough = mv;
            }
        } else {
            self.trough = self.trough.min(mv);
            if mv >= self.trough + 50 {
                self.charging = true;
                self.peak = mv;
            }
        }
        self.charging
    }
}

static TREND: Mutex<Option<HashMap<u16, Trend>>> = Mutex::new(None);

fn poll_cw(ctx: &mut Ctx, pid: u16) {
    for c in ctx.find(0x04A5, pid, |c| (c.usage_page() >= 0xFF00 || c.usage_page() == 0x00FF) && c.output_len() == 65) {
        let Some(d) = ctx.open(&c) else { continue };
        d.flush();
        if d.write(&[0x00, 0xA5, 0x03]).is_err() {
            continue;
        }
        let Some(r) = d.read(1000).filter(|r| r.len() >= 3) else { continue };
        let mv = u16::from_le_bytes([r[0], r[1]]) as i32;
        if mv != 0 && !(3000..=4500).contains(&mv) {
            continue;
        }
        let mut s = Status::new(format!("zowie:{}", hex4(pid)), "ZOWIE EC-CW", Kind::Mouse);
        s.charging = pid == 0x8005;
        if mv == 0 {
            s.connected = false;
        } else {
            s.percent = if (1..=100).contains(&r[2]) { r[2] as i32 } else { zowie_percent(mv) };
        }
        ctx.add(s, 0x04A5, pid);
        return;
    }
}

fn poll_dw(ctx: &mut Ctx, pid: u16) {
    let cmd = ctx.find(0x04A5, pid, |c| c.usage_page() == 0xFF03 && c.output_len() == 16);
    let rep = ctx.find(0x04A5, pid, |c| c.usage_page() == 0xFF04 && c.input_len() == 16);
    let (Some(cmd_c), Some(rep_c)) = (cmd.first(), rep.first()) else { return };
    // Abre o canal de resposta antes.
    let Some(input) = ctx.open(rep_c) else { return };
    let Some(output) = ctx.open(cmd_c) else { return };
    input.flush();
    if output.write(&[0x08, 0xB4, 0x15]).is_err() {
        return;
    }
    let end = Deadline::in_ms(1500);
    let mut reply = None;
    while !end.expired() {
        let Some(r) = input.read(end.left()).filter(|r| r.len() >= 5) else { break };
        if r[0] == 0x09 {
            reply = Some(r);
            break;
        }
    }
    let Some(r) = reply.filter(|r| r[1] != 0xE6 && r[4] == 0xC8) else { return };
    let mv = (r[1] as i32 * 256 + r[2] as i32) * 10;
    let cable = pid != 0x800A && pid != 0x8008;
    let product = cmd_c.product_string().unwrap_or("");
    let name = if pid == 0x8008 {
        "ZOWIE U2".to_string()
    } else if product.is_empty() || product.contains("Receiver") {
        "ZOWIE DW mouse".to_string()
    } else {
        product.to_string()
    };
    let mut s = Status::new(format!("zowie:{}", hex4(pid)), name, Kind::Mouse);
    if mv == 0 {
        s.connected = false;
    } else {
        s.percent = zowie_percent(mv);
        let mut t = TREND.lock().unwrap();
        s.charging = cable || t.get_or_insert_with(HashMap::new).entry(pid).or_default().update(mv);
    }
    ctx.add(s, 0x04A5, pid);
}

pub fn poll(ctx: &mut Ctx) {
    for (_, pid) in ctx.products(0x04A5) {
        if pid == 0x8005 || pid == 0x8006 {
            poll_cw(ctx, pid);
        } else {
            poll_dw(ctx, pid); // so age em quem tem as colecoes de comando/resposta do DW
        }
    }
}
