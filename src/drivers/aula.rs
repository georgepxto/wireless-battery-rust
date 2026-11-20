//! Teclados AULA F75 / F87 Pro / F99 e MCHOSE (K99 V2, G75 Pro, X75...) nos receptores 2.4 GHz
//! "BY": 3554:FA09 (Compx, das duas marcas) e 41E4:2001 (da MCHOSE).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), src/drivers/aula.cpp.
//!
//! - Colecao vendor de 20 bytes (usage page FF02 ou FF04, usage 02).
//! - Pedido: output report 0x13, comando 0x4A, soma dos bytes no final.
//! - Resposta: outro frame 0x13 cujo comando, sem o bit 7, e 0x4A. Byte 5 = nivel (1..100),
//!   byte 6 = estado (nibble alto 1 = carregando; a AULA manda 0x01 na bateria e 0x10 no cabo,
//!   e no cabo o nivel fica fixo em 100).
//! - O receptor tambem manda outros frames 0x13 (ex.: status 0x0A); o id sozinho nao basta.
//! - Firmware MCHOSE deixa o ultimo byte em 0 em vez da soma; aceito so em frame de um pacote.
//! - Teclado cochilando ignora o primeiro pedido: reenvia a cada 1,3 s dentro de 4 s.
//! - Receptores com nome generico recebem antes o pedido de modelo (comando 0x05, so leitura),
//!   como o M HUB faz, para identificar o modelo MCHOSE.
//!
//! Fontes do protocolo: PyFlat/Device-Battery-Info (PR 6), deepan-alve/womier-l65-linux,
//! ushineko/sanshoku#36 e o driver web M HUB da MCHOSE.

use super::{hex4, sum8, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;
use std::time::Instant;

const RECEIVERS: [(u16, u16); 2] = [(0x3554, 0xFA09), (0x41E4, 0x2001)];
const REPORT: u8 = 0x13;
const CMD_BATTERY: u8 = 0x4A;
const CMD_MODEL: u8 = 0x05;
const LEN: usize = 20;
const RESEND_MS: u64 = 1300;

/// Tabela keyboardPwds do M HUB: codigo de modelo de seis bytes devolvido pelo comando 0x05.
const MODELS: &[([u8; 6], &str)] = &[
    ([0x03, 0, 0, 0, 0x02, 0x1C], "MCHOSE G87"),
    ([0x03, 0, 0, 0, 0x00, 0x77], "MCHOSE K99"),
    ([0x03, 0, 0, 0, 0x01, 0xDF], "MCHOSE K99 V2"),
    ([0x03, 0, 0, 0, 0x01, 0x32], "MCHOSE G98"),
    ([0x03, 0, 0, 0, 0x00, 0xD6], "MCHOSE X75"),
    ([0x0A, 0, 0, 0, 0x00, 0x03], "MCHOSE G98 Pro"),
    ([0x03, 0, 0, 0, 0x01, 0xC6], "MCHOSE X75 V2"),
    ([0x03, 0, 0, 0, 0x01, 0x15], "MCHOSE G75 Pro"),
    ([0x0A, 0, 0, 0, 0x00, 0x18], "MCHOSE G98 V2"),
    ([0x0A, 0, 0, 0, 0x00, 0x2B], "MCHOSE UT98"),
    ([0x0A, 0, 0, 0, 0x00, 0x01], "MCHOSE Z75"),
    ([0x0A, 0, 0, 0, 0x00, 0x09], "MCHOSE Z75S"),
    ([0x03, 0, 0, 0, 0x01, 0xC7], "MCHOSE KX75"),
    ([0x03, 0, 0, 0, 0x01, 0x74], "MCHOSE GX87"),
    ([0x03, 0, 0, 0, 0x01, 0xED], "MCHOSE GX87 V2"),
    ([0x03, 0, 0, 0, 0x01, 0x35], "MCHOSE K87"),
    ([0x03, 0, 0, 0, 0x03, 0x35], "MCHOSE G75 V2"),
    ([0x0A, 0, 0, 0, 0x00, 0x34], "MCHOSE G87 V2"),
];

pub fn request(cmd: u8, packets: u8) -> [u8; LEN] {
    let mut r = [0u8; LEN];
    r[0] = REPORT;
    r[1] = cmd;
    r[2] = packets;
    r[LEN - 1] = sum8(&r[..LEN - 1]);
    r
}

/// A resposta ao comando `cmd`, ou None para frames que nao sao ela.
pub fn is_reply(r: &[u8], cmd: u8) -> bool {
    if r.len() < LEN || r[0] != REPORT || r[1] & 0x7F != cmd {
        return false;
    }
    if r[4..19].iter().all(|b| *b == 0) {
        return false; // eco do pedido
    }
    let checksum = r[19] == sum8(&r[..19]);
    let mchose = r[19] == 0 && r[2] == 1 && r[3] == 0;
    checksum || mchose
}

/// Envia 13 <cmd> e espera a resposta, reenviando a cada 1,3 s dentro de `total_ms`.
fn exchange(d: &HidDevice, cmd: u8, total_ms: u64) -> Option<Vec<u8>> {
    let start = Instant::now();
    let end = Deadline::in_ms(total_ms);
    let mut sent = 0u64;
    d.flush();
    loop {
        if end.expired() {
            return None;
        }
        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed >= sent * RESEND_MS {
            // Byte 2 alterna 00 / 01 nos reenvios: o M HUB manda 01, as referencias da AULA 00.
            d.write(&request(cmd, (sent & 1) as u8)).ok()?;
            sent += 1;
        }
        let next = (sent * RESEND_MS).saturating_sub(start.elapsed().as_millis() as u64) as u32;
        let wait = next.min(end.left()).max(1);
        let before = Instant::now();
        match d.read(wait) {
            Some(r) if is_reply(&r, cmd) => return Some(r),
            Some(_) => {}
            // Falha imediata e erro de E/S (receptor desplugado), nao tempo esgotado.
            None if (before.elapsed().as_millis() as u32) + 20 < wait => return None,
            None => {}
        }
    }
}

/// O M HUB le o codigo do modelo nos bytes 5..10; numa captura veio um byte antes.
fn identify_model(d: &HidDevice) -> Option<&'static str> {
    let r = exchange(d, CMD_MODEL, 1300)?;
    [5usize, 4].iter().find_map(|&off| MODELS.iter().find(|(code, _)| r.get(off..off + 6) == Some(&code[..])).map(|(_, n)| *n))
}

/// AULA F75 no modo USB (com fio): larga o dongle e aparece como 258A:010C (BY Tech, "Gaming Keyboard").
/// No cabo ele esta sempre carregando e nao informa o nivel; com o dongle ligado fica a mesma entrada.
const WIRED: (u16, u16) = (0x258A, 0x010C);

pub fn poll_wired(ctx: &mut Ctx) {
    if ctx.is_claimed(WIRED.0, WIRED.1) {
        return;
    }
    // Fabricante "BY Tech", produto "Gaming Keyboard".
    let present = !ctx.find(WIRED.0, WIRED.1, |c| c.manufacturer_string().unwrap_or("").contains("BY Tech")).is_empty();
    if !present {
        return;
    }
    let dongle = !ctx.find(0x3554, 0xFA09, |_| true).is_empty();
    let key = if dongle { "aula:3554:FA09".to_string() } else { format!("aula:{}:{}", hex4(WIRED.0), hex4(WIRED.1)) };
    let mut s = Status::new(key, "AULA F75", Kind::Keyboard);
    s.charging = true;
    ctx.add(s, WIRED.0, WIRED.1);
    if dongle {
        ctx.claim(0x3554, 0xFA09); // o dongle nao alcanca o teclado no modo USB
    }
}

pub fn poll(ctx: &mut Ctx) {
    for (vid, pid) in RECEIVERS {
        if ctx.is_claimed(vid, pid) {
            continue;
        }
        let colls = ctx.find(vid, pid, |c| {
            matches!(c.usage_page(), 0xFF02 | 0xFF04) && c.usage() == 0x0002 && c.output_len() >= 20 && c.input_len() >= 20
        });
        if colls.is_empty() {
            continue;
        }
        let mut handled = false;
        for c in &colls {
            let Some(d) = ctx.open(c) else { continue };
            let product = c.product_string().unwrap_or("");
            let generic = product.contains("2.4G Wireless Receiver") || product.contains("MCHOSE 2.4G Wireless");
            let model = if generic { identify_model(&d) } else { None };
            let mchose = vid == 0x41E4 || model.is_some();
            let Some(r) = exchange(&d, CMD_BATTERY, 4000) else { continue };
            if crate::verbose() {
                eprintln!("[aula {}:{}] resposta: {:02X?}", hex4(vid), hex4(pid), &r[..LEN.min(r.len())]);
            }
            let level = r[5] as i32;
            if !(1..=100).contains(&level) {
                continue;
            }
            let name = model.unwrap_or(if mchose { "MCHOSE wireless keyboard" } else { "AULA F75" });
            let mut s = Status::new(format!("aula:{}:{}", hex4(vid), hex4(pid)), name, Kind::Keyboard);
            s.charging = r[6] >> 4 == 1;
            // AULA no cabo: o nivel fica fixo em 100, entao so "carregando".
            s.percent = if s.charging && !mchose { -1 } else { level };
            ctx.add(s, vid, pid);
            handled = true;
            break;
        }
        if !handled {
            // Receptor plugado, teclado desligado ou dormindo: mantem o ultimo nivel.
            let mut s = Status::new(format!("aula:{}:{}", hex4(vid), hex4(pid)), if vid == 0x41E4 { "MCHOSE wireless keyboard" } else { "AULA F75" }, Kind::Keyboard);
            s.asleep = true;
            ctx.add(s, vid, pid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn request_checksum() {
        let r = request(CMD_BATTERY, 0);
        assert_eq!((r[0], r[1], r[19]), (0x13, 0x4A, 0x5D));
    }

    #[test]
    fn accepts_captured_replies() {
        assert!(is_reply(&hex("134A0100026101000000000000000000000000C2"), CMD_BATTERY));
        assert!(is_reply(&hex("134A0100026410000000000000000000000000D4"), CMD_BATTERY));
        assert!(!is_reply(&hex("134A0100026101000000000000000000000000C3"), CMD_BATTERY));
        // Eco do pedido.
        assert!(!is_reply(&request(CMD_BATTERY, 0), CMD_BATTERY));
        // MCHOSE: ultimo byte 0, um pacote.
        assert!(is_reply(&hex("134A0100005A1000000000000000000000000000"), CMD_BATTERY));
    }
}
