//! MCHOSE V9 Pro pelo dongle 2.4G ou pelo cabo USB.
//!
//! Protocolo do projeto jmjp/mchose-hub-native (MIT), obtido por engenharia
//! reversa do M HUB oficial, e confirmado no headset real:
//! - Dongle: VID 0x291D (PID 0x385D). Cabo USB: o headset aparece com o chip de audio,
//!   VID 0x0D8C (C-Media), PID 0x0343, nome "MCHOSE V9 PRO HEADSET". Os dois tem as
//!   colecoes 0xFF90 (report id 85) e 0xFF82.
//! - Pedido sem framing na colecao 0xFF90: `[85, 0x65, 0x01, 0 x 61]`.
//! - Resposta: `[85, 0x65, porcentagem, estado]`.
//!   Estado: 0 = ainda indefinido (vem primeiro, o real chega logo depois),
//!   2 = descarregando, 3 = carregando, 4 = cheio, 26 = dormindo.
//! - Com o cabo ligado, o headset deixa de falar pelo dongle: o dongle fica sem resposta.
//!   Os dois viram um unico dispositivo, com a melhor leitura entre eles.

use crate::device::{Kind, Observation, State};
use crate::hid::{DeviceInfo, HidApi, HidDevice};
use std::time::{Duration, Instant};

const DONGLE_VID: u16 = 0x291D;
const CMEDIA_VID: u16 = 0x0D8C;
const USAGE_COMMAND: u16 = 0xFF82;
const USAGE_NOTIFY: u16 = 0xFF90;
const REPORT_ID: u8 = 85;
const MARKER: u8 = 0x65;
const TIMEOUT: Duration = Duration::from_millis(1200);

const DISCHARGING: u8 = 2;
const CHARGING: u8 = 3;
const FULL: u8 = 4;
const SLEEPING: u8 = 26;

/// Dongle MCHOSE ou o headset pelo cabo. O VID da C-Media e usado por muitos aparelhos
/// de audio, entao so conta com o nome MCHOSE.
fn is_mchose(d: &DeviceInfo) -> bool {
    d.vendor_id() == DONGLE_VID
        || (d.vendor_id() == CMEDIA_VID && d.product_string().unwrap_or("").to_uppercase().contains("MCHOSE"))
}

pub fn scan(api: &HidApi) -> Vec<Observation> {
    let mut ids: Vec<(u16, u16)> = api
        .device_list()
        .filter(|d| is_mchose(d) && d.usage_page() == USAGE_NOTIFY)
        .map(|d| (d.vendor_id(), d.product_id()))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Vec::new();
    }

    // Cabo primeiro: com ele ligado, e o unico caminho que responde.
    ids.sort_by_key(|(vid, _)| *vid != CMEDIA_VID);
    let mut best = State::Off;
    for (vid, pid) in ids {
        let wired = vid == CMEDIA_VID;
        let state = read(api, vid, pid, wired);
        if crate::verbose() || cfg!(debug_assertions) {
            println!("[mchose {vid:04x}:{pid:04x}] {state:?}");
        }
        if rank(&state) > rank(&best) {
            best = state;
        }
        if matches!(best, State::Battery { .. }) {
            break;
        }
    }
    vec![Observation {
        id: "mchose:v9pro".to_string(),
        name: Some("MCHOSE V9 Pro".to_string()),
        kind: Some(Kind::Headset),
        state: best,
    }]
}

fn rank(s: &State) -> u8 {
    match s {
        State::Battery { .. } | State::ChargingNoLevel => 2,
        State::Unreachable => 1,
        State::Off => 0,
    }
}

fn read(api: &HidApi, vid: u16, pid: u16, wired: bool) -> State {
    let Some(notify) = open(api, vid, pid, USAGE_NOTIFY) else { return State::Off };
    let command = open(api, vid, pid, USAGE_COMMAND);
    match query(&notify, command.as_ref()) {
        Some((percent, status)) if percent <= 100 => match status {
            DISCHARGING => State::Battery { percent, charging: false },
            CHARGING => State::Battery { percent, charging: true },
            FULL => State::Battery { percent, charging: false },
            SLEEPING => State::Unreachable,
            // Estado ainda indefinido: no cabo, o headset esta carregando.
            _ => State::Battery { percent, charging: wired },
        },
        // Sem resposta valida: dongle conectado com o headset desligado ou no cabo.
        _ => State::Off,
    }
}

fn open(api: &HidApi, vid: u16, pid: u16, usage_page: u16) -> Option<HidDevice> {
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == vid && d.product_id() == pid && d.usage_page() == usage_page)?;
    api.open_path(info.path()).ok()
}

/// Envia o pedido de bateria e espera a resposta. A primeira resposta costuma vir com o
/// estado 0 (indefinido); continua lendo ate chegar um estado conhecido ou acabar o tempo.
fn query(notify: &HidDevice, command: Option<&HidDevice>) -> Option<(u8, u8)> {
    let mut tx = [0u8; 64];
    tx[0] = REPORT_ID;
    tx[1] = MARKER;
    tx[2] = 0x01;
    let written = notify.write(&tx);
    if crate::verbose() {
        println!("[mchose] escrita na 0xFF90: {written:?}");
    }
    written.ok()?;

    let deadline = Instant::now() + TIMEOUT;
    let mut rx = [0u8; 64];
    let mut last = None;
    while Instant::now() < deadline {
        for (name, dev) in std::iter::once(("FF90", notify)).chain(command.map(|c| ("FF82", c))) {
            let Ok(n) = dev.read_timeout(&mut rx, 10) else { continue };
            if n > 0 && crate::verbose() {
                println!("[mchose] resposta {name}: {:02X?}", &rx[..n.min(16)]);
            }
            if let Some(r) = parse(&rx[..n]) {
                if matches!(r.1, DISCHARGING | CHARGING | FULL | SLEEPING) {
                    return Some(r);
                }
                last = Some(r);
            }
        }
    }
    if crate::verbose() && last.is_none() {
        println!("[mchose] sem resposta de bateria em {} ms", TIMEOUT.as_millis());
    }
    last
}

/// Aceita a resposta com ou sem o byte de report id na frente.
fn parse(data: &[u8]) -> Option<(u8, u8)> {
    match data {
        [REPORT_ID, MARKER, percent, status, ..] => Some((*percent, *status)),
        [MARKER, percent, status, ..] => Some((*percent, *status)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_reply_with_and_without_report_id() {
        // Capturado do headset real pelo cabo: 70%, carregando.
        assert_eq!(parse(&[0x55, 0x65, 0x46, 0x03, 0, 0]), Some((70, 3)));
        assert_eq!(parse(&[0x55, 0x65, 0x46, 0x00, 0, 0]), Some((70, 0)));
        assert_eq!(parse(&[0x65, 50, 2]), Some((50, 2)));
        assert_eq!(parse(&[85, 0x11, 2]), None);
    }
}
