//! MCHOSE V9 Pro (e headsets da mesma familia) pelo dongle 2.4G / cabo USB.
//!
//! Protocolo do projeto jmjp/mchose-hub-native (MIT), obtido por engenharia
//! reversa do M HUB oficial:
//! - Colecoes HID do VID 0x291D: 0xFF90 (notificacoes, report id 85) e 0xFF82
//!   (comandos). O pedido vai pela 0xFF90, que e a que declara o report 85.
//! - Pedido sem framing: `[85, 0x65, 0x01, 0 x 61]`.
//! - Resposta: `[0x65, porcentagem, estado]` depois do report id.
//!   Estado: 2 = descarregando, 3 = carregando, 4 = cheio, 26 = dormindo.

use crate::device::{Kind, Observation, State};
use hidapi::{HidApi, HidDevice};
use std::time::{Duration, Instant};

const VID: u16 = 0x291D;
const USAGE_COMMAND: u16 = 0xFF82;
const USAGE_NOTIFY: u16 = 0xFF90;
const REPORT_ID: u8 = 85;
const MARKER: u8 = 0x65;
const TIMEOUT: Duration = Duration::from_millis(800);

pub fn scan(api: &HidApi) -> Vec<Observation> {
    let mut pids: Vec<u16> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.usage_page() == USAGE_NOTIFY)
        .map(|d| d.product_id())
        .collect();
    pids.sort_unstable();
    pids.dedup();

    let mut out = Vec::new();
    for pid in pids {
        let Some(notify) = open(api, pid, USAGE_NOTIFY) else { continue };
        let command = open(api, pid, USAGE_COMMAND);
        let state = match query(&notify, command.as_ref()) {
            Some((percent, 2 | 4)) if percent <= 100 => State::Battery { percent, charging: false },
            Some((percent, 3)) if percent <= 100 => State::Battery { percent, charging: true },
            Some((_, 26)) => State::Unreachable,
            // Sem resposta valida: dongle conectado com o headset desligado.
            _ => State::Off,
        };
        if cfg!(debug_assertions) {
            eprintln!("[mchose {pid:04x}] {state:?}");
        }
        out.push(Observation {
            id: format!("mchose:{pid:04x}"),
            name: Some("MCHOSE V9 Pro".to_string()),
            kind: Some(Kind::Headset),
            state,
        });
    }
    out
}

fn open(api: &HidApi, pid: u16, usage_page: u16) -> Option<HidDevice> {
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == VID && d.product_id() == pid && d.usage_page() == usage_page)?;
    api.open_path(info.path()).ok()
}

/// Envia o pedido de bateria e espera a resposta em qualquer das duas colecoes.
fn query(notify: &HidDevice, command: Option<&HidDevice>) -> Option<(u8, u8)> {
    let mut tx = [0u8; 64];
    tx[0] = REPORT_ID;
    tx[1] = MARKER;
    tx[2] = 0x01;
    if notify.write(&tx).is_err() {
        command?.write(&tx).ok()?;
    }

    let deadline = Instant::now() + TIMEOUT;
    let mut rx = [0u8; 64];
    while Instant::now() < deadline {
        for dev in std::iter::once(notify).chain(command) {
            let Ok(n) = dev.read_timeout(&mut rx, 10) else { continue };
            if let Some(r) = parse(&rx[..n]) {
                return Some(r);
            }
        }
    }
    None
}

/// Aceita a resposta com ou sem o byte de report id na frente.
fn parse(data: &[u8]) -> Option<(u8, u8)> {
    match data {
        [_, MARKER, percent, status, ..] => Some((*percent, *status)),
        [MARKER, percent, status, ..] => Some((*percent, *status)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_reply_with_and_without_report_id() {
        assert_eq!(parse(&[85, 0x65, 87, 3]), Some((87, 3)));
        assert_eq!(parse(&[0x65, 50, 2]), Some((50, 2)));
        assert_eq!(parse(&[85, 0x11, 2]), None);
    }
}
