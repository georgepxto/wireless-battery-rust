//! Logitech devices over HID++: Unifying / Lightspeed / Bolt receivers and directly attached
//! devices (e.g. headset dongles). Devices are discovered automatically.

mod curve;
pub mod hidpp;

use crate::device::{Kind, Observation, State};
use hidapi::HidApi;
use hidpp::{Endpoint, Error, Link, ERR_UNKNOWN_DEVICE};
use std::collections::HashMap;

/// Known receiver PIDs (Unifying, Nano, Lightspeed, Bolt). Anything named "Receiver" also counts.
const RECEIVER_PIDS: &[u16] = &[
    0xC517, 0xC518, 0xC51A, 0xC51B, 0xC521, 0xC525, 0xC526, 0xC52B, 0xC52E, 0xC52F, 0xC531, 0xC532,
    0xC534, 0xC537, 0xC539, 0xC53A, 0xC53D, 0xC53F, 0xC541, 0xC545, 0xC547, 0xC548, 0xC54D,
];

const DEVICE_FW_VERSION: u16 = 0x0003;
const DEVICE_NAME: u16 = 0x0005;
const BATTERY_STATUS: u16 = 0x1000;
const BATTERY_VOLTAGE: u16 = 0x1001;
const UNIFIED_BATTERY: u16 = 0x1004;
const ADC_MEASUREMENT: u16 = 0x1F20;

enum Raw {
    Percent { percent: u8, charging: bool },
    Millivolts { mv: u16, charging: bool },
    Off,
    NoBattery,
}

#[derive(Clone)]
struct Identity {
    name: String,
    kind: Kind,
    /// Device PIDs to look up a G HUB discharge curve.
    pids: Vec<u16>,
}

pub struct Logitech {
    curves: curve::Curves,
    identities: HashMap<String, Identity>,
}

impl Logitech {
    pub fn new() -> Self {
        Self { curves: curve::Curves::load(), identities: HashMap::new() }
    }

    pub fn scan(&mut self, api: &HidApi) -> Vec<Observation> {
        let mut out = Vec::new();
        for ep in hidpp::endpoints(api) {
            let Ok(link) = Link::open(api, &ep) else { continue };
            let receiver = RECEIVER_PIDS.contains(&ep.pid) || ep.product.to_ascii_lowercase().contains("receiver");
            if receiver {
                out.extend((1..=6).filter_map(|i| self.probe(&link, &ep, i, true)));
            } else {
                out.extend(self.probe(&link, &ep, 0xFF, false));
            }
        }
        out
    }

    fn probe(&mut self, link: &Link, ep: &Endpoint, index: u8, via_receiver: bool) -> Option<Observation> {
        let id = format!("logi:{:04x}:{}:{:02x}", ep.pid, ep.iface, index);
        let mut result = battery(link, index);
        if via_receiver && !self.identities.contains_key(&id) {
            // A sleeping device times out on the first request but starts waking up from it.
            // Give it a moment and retry so it gets discovered. Known devices aren't woken
            // (it costs them battery); they keep their last reading instead.
            for _ in 0..2 {
                if !matches!(result, Err(Error::Timeout)) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                result = battery(link, index);
            }
        }
        let raw = match result {
            Ok(Raw::NoBattery) => {
                if cfg!(debug_assertions) {
                    eprintln!("[{id}] no battery feature");
                }
                return None;
            }
            Ok(raw) => raw,
            Err(Error::Protocol(ERR_UNKNOWN_DEVICE)) if via_receiver => return None, // empty slot
            Err(e) => {
                debug(&id, &e);
                if via_receiver {
                    // Paired but not answering (asleep/off). Only meaningful if we know it.
                    let known = self.identities.get(&id)?;
                    return Some(Observation {
                        id,
                        name: Some(known.name.clone()),
                        kind: Some(known.kind),
                        state: State::Unreachable,
                    });
                }
                // Direct device that doesn't answer: report it only if it looks like a
                // wireless dongle (e.g. headset switched off), otherwise ignore it.
                let looks_wireless = ["wireless", "headset"].iter().any(|k| ep.product.to_ascii_lowercase().contains(k));
                if !self.identities.contains_key(&id) && !looks_wireless {
                    return None;
                }
                Raw::Off
            }
        };

        if !self.identities.contains_key(&id) && !matches!(raw, Raw::Off) {
            let identity = identify(link, index, ep, via_receiver);
            self.identities.insert(id.clone(), identity);
        }
        let identity = self.identities.get(&id).cloned().unwrap_or_else(|| Identity {
            name: ep.product.clone(),
            kind: Kind::from_name(&ep.product),
            pids: vec![ep.pid],
        });

        let state = match raw {
            Raw::Percent { percent, charging } => State::Battery { percent, charging },
            Raw::Millivolts { mv, charging } => {
                let percent = self.curves.percent(&identity.pids, mv);
                if cfg!(debug_assertions) {
                    eprintln!("[{id}] {mv} mV -> {percent}% (pids {:04x?})", identity.pids);
                }
                State::Battery { percent, charging }
            }
            Raw::Off | Raw::NoBattery => State::Off,
        };
        Some(Observation { id, name: Some(identity.name), kind: Some(identity.kind), state })
    }
}

fn debug(id: &str, e: &Error) {
    if cfg!(debug_assertions) {
        eprintln!("[{id}] {e}");
    }
}

fn battery(link: &Link, index: u8) -> Result<Raw, Error> {
    if let Some(f) = link.feature_index(index, UNIFIED_BATTERY)? {
        let r = link.request(index, f, 0x1, &[])?; // get_status
        return Ok(Raw::Percent { percent: r[0], charging: matches!(r[2], 1 | 2) });
    }
    if let Some(f) = link.feature_index(index, BATTERY_STATUS)? {
        let r = link.request(index, f, 0x0, &[])?;
        return Ok(Raw::Percent { percent: r[0], charging: matches!(r[2], 1 | 2 | 4) });
    }
    if let Some(f) = link.feature_index(index, BATTERY_VOLTAGE)? {
        let r = link.request(index, f, 0x0, &[])?;
        return Ok(Raw::Millivolts { mv: u16::from_be_bytes([r[0], r[1]]), charging: r[2] & 0x80 != 0 });
    }
    if let Some(f) = link.feature_index(index, ADC_MEASUREMENT)? {
        let r = link.request(index, f, 0x0, &[])?;
        if r[2] & 0x01 == 0 {
            return Ok(Raw::Off); // dongle present, device off
        }
        return Ok(Raw::Millivolts { mv: u16::from_be_bytes([r[0], r[1]]), charging: r[2] & 0x02 != 0 });
    }
    Ok(Raw::NoBattery)
}

fn identify(link: &Link, index: u8, ep: &Endpoint, via_receiver: bool) -> Identity {
    let mut name = None;
    let mut kind = None;
    if let Ok(Some(f)) = link.feature_index(index, DEVICE_NAME) {
        if let Ok(r) = link.request(index, f, 0x0, &[]) {
            let len = r[0] as usize;
            let mut bytes = Vec::with_capacity(len);
            while bytes.len() < len {
                let Ok(chunk) = link.request(index, f, 0x1, &[bytes.len() as u8]) else { break };
                let take = (len - bytes.len()).min(16);
                bytes.extend_from_slice(&chunk[..take]);
            }
            let s = String::from_utf8_lossy(&bytes).trim_matches(char::from(0)).trim().to_string();
            name = (!s.is_empty()).then_some(s);
        }
        if let Ok(r) = link.request(index, f, 0x2, &[]) {
            kind = match r[0] {
                0 | 2 => Some(Kind::Keyboard),
                3..=5 => Some(Kind::Mouse),
                8 => Some(Kind::Headset),
                10..=12 => Some(Kind::Gamepad),
                _ => None,
            };
        }
    }

    // Model ids hold the device's PIDs per transport; used to find its G HUB curve.
    let mut pids = if via_receiver { Vec::new() } else { vec![ep.pid] };
    if let Ok(Some(f)) = link.feature_index(index, DEVICE_FW_VERSION) {
        if let Ok(r) = link.request(index, f, 0x0, &[]) {
            pids.extend((0..3).map(|i| u16::from_be_bytes([r[7 + 2 * i], r[8 + 2 * i]])).filter(|p| *p != 0));
        }
    }

    let fallback = if via_receiver { "Logitech device".to_string() } else { ep.product.clone() };
    let name = name.unwrap_or(fallback);
    let kind = kind.unwrap_or_else(|| Kind::from_name(&name));
    Identity { name, kind, pids }
}
