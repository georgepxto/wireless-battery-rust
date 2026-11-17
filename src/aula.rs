//! AULA F75 pelo receptor 2.4G (3554:FA09, "Compx 2.4G Receiver").
//!
//! Protocolo publicado sob MIT (ushineko/sanshoku#36, PyFlat/Device-Battery-Info#6):
//! - Colecao vendor: usage page 0xFF02, usage 0x0002. Pedido como output report,
//!   resposta como input report, ambos com 20 bytes e report id 0x13.
//! - Pedido: `13 4A 00 .. 00 5D` (ultimo byte = soma dos anteriores).
//! - Resposta: [1] & 0x7F = 0x4A; [5] = nivel 1-100; [6] = 0x01 na bateria,
//!   0x10 no cabo (o nivel fica fixo em 100, entao nao ha porcentagem);
//!   [19] = soma dos bytes 0-18.
//!
//! No modo Bluetooth o teclado aparece como "AULA-F75 5.0 KB" e e lido pelo
//! modulo bluetooth (GATT Battery Service).

use crate::device::{Kind, Observation, State};
use hidapi::{HidApi, HidDevice};
use std::time::{Duration, Instant};

const VID: u16 = 0x3554;
const PID_RECEIVER: u16 = 0xFA09;
const USAGE_PAGE: u16 = 0xFF02;
const USAGE: u16 = 0x0002;
const REPORT_ID: u8 = 0x13;
const CMD_BATTERY: u8 = 0x4A;
const LEN: usize = 20;
const TIMEOUT: Duration = Duration::from_millis(800);

pub fn scan(api: &HidApi) -> Vec<Observation> {
    let Some(info) = api.device_list().find(|d| {
        d.vendor_id() == VID && d.product_id() == PID_RECEIVER && d.usage_page() == USAGE_PAGE && d.usage() == USAGE
    }) else {
        return Vec::new();
    };
    let Ok(dev) = api.open_path(info.path()) else { return Vec::new() };

    let state = match query(&dev) {
        Some((level, 0x01)) if (1..=100).contains(&level) => State::Battery { percent: level, charging: false },
        Some((_, 0x10)) => State::ChargingNoLevel,
        // Receptor plugado, teclado desligado ou dormindo.
        _ => State::Unreachable,
    };
    if cfg!(debug_assertions) || crate::verbose() {
        println!("[aula] {state:?}");
    }
    vec![Observation {
        id: "aula:fa09".to_string(),
        name: Some("AULA F75".to_string()),
        kind: Some(Kind::Keyboard),
        state,
    }]
}

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))
}

fn request() -> [u8; LEN] {
    let mut tx = [0u8; LEN];
    tx[0] = REPORT_ID;
    tx[1] = CMD_BATTERY;
    tx[LEN - 1] = checksum(&tx[..LEN - 1]);
    tx
}

fn query(dev: &HidDevice) -> Option<(u8, u8)> {
    dev.write(&request()).ok()?;
    let deadline = Instant::now() + TIMEOUT;
    let mut rx = [0u8; 64];
    while Instant::now() < deadline {
        let n = dev.read_timeout(&mut rx, 20).ok()?;
        if n > 0 && crate::verbose() {
            println!("[aula] resposta: {:02X?}", &rx[..n]);
        }
        if let Some(r) = parse(&rx[..n]) {
            return Some(r);
        }
    }
    None
}

/// O receptor tambem manda outros frames 0x13 (ex.: status 0x0A); so aceita o de bateria.
fn parse(data: &[u8]) -> Option<(u8, u8)> {
    if data.len() < LEN || data[0] != REPORT_ID || data[1] & 0x7F != CMD_BATTERY {
        return None;
    }
    if checksum(&data[..LEN - 1]) != data[LEN - 1] {
        return None;
    }
    Some((data[5], data[6]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn request_checksum() {
        let r = request();
        assert_eq!((r[0], r[1], r[19]), (0x13, 0x4A, 0x5D));
    }

    #[test]
    fn parses_captured_frames() {
        assert_eq!(parse(&hex("134A0100026101000000000000000000000000C2")), Some((97, 0x01)));
        assert_eq!(parse(&hex("134A0100026410000000000000000000000000D4")), Some((100, 0x10)));
        assert_eq!(parse(&hex("134A0100026101000000000000000000000000C3")), None);
    }
}
