//! Source-independent device model, and the tracker that merges scans over time.

use std::collections::HashSet;

/// Also the display order on the taskbar.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    Headset,
    Earbuds,
    Mouse,
    Keyboard,
    Gamepad,
    #[allow(dead_code)] // usado pelo leitor do Phone Link (em breve)
    Phone,
    Other,
}

impl Kind {
    pub fn from_name(name: &str) -> Kind {
        let n = name.to_ascii_lowercase();
        if ["headset", "headphone", "airpods max"].iter().any(|k| n.contains(k)) {
            Kind::Headset
        } else if ["earbud", "buds", "airpods", "earphone", "in-ear", "liberty", "space a40", "pixel buds", "wf-"].iter().any(|k| n.contains(k)) {
            Kind::Earbuds
        } else if ["mouse", "mice", "trackball", "superlight"].iter().any(|k| n.contains(k)) {
            Kind::Mouse
        } else if n.contains("keyboard") || n.contains("keys") || n.ends_with(" kb") {
            Kind::Keyboard
        } else if ["controller", "gamepad", "xbox", "dualsense", "dualshock", "joy-con"].iter().any(|k| n.contains(k)) {
            Kind::Gamepad
        } else if ["arctis", "cloud", "void", "virtuoso", "blackshark", "barracuda", "quantum", "inzone"]
            .iter()
            .any(|k| n.contains(k))
        {
            Kind::Headset
        } else {
            Kind::Other
        }
    }
}

/// What a source saw for one device during one scan.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Battery { percent: u8, charging: bool },
    /// Charging, but the device doesn't report a level while the cable is in.
    ChargingNoLevel,
    /// Known to exist but didn't answer (asleep, or off; often indistinguishable).
    Unreachable,
    /// Definitely not connected (e.g. headset off while its dongle is plugged).
    Off,
}

#[derive(Clone, Debug)]
pub struct Observation {
    /// Stable per device across scans, e.g. "logi:c539:2:01" or "ble:f4ee25fbba46".
    pub id: String,
    /// None when the source can't identify the device right now (only updates known devices).
    pub name: Option<String>,
    pub kind: Option<Kind>,
    pub state: State,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Battery { percent: u8, charging: bool },
    /// No cabo, mas o aparelho nao informa a porcentagem (ex.: AULA F75).
    ChargingNoLevel,
    NotConnected,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Battery { percent, charging } => {
                write!(f, "{percent}%")?;
                if *charging {
                    write!(f, " (carregando)")?;
                }
                Ok(())
            }
            Status::ChargingNoLevel => write!(f, "carregando"),
            Status::NotConnected => write!(f, "desconectado"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Reading {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub status: Status,
}

/// Remembers every device seen this session, so a device that disappears shows as
/// "not connected" and a sleeping one keeps its last battery reading.
#[derive(Default)]
pub struct Tracker {
    known: Vec<Reading>,
}

impl Tracker {
    pub fn update(&mut self, observations: Vec<Observation>) -> Vec<Reading> {
        let mut seen = HashSet::new();
        for o in observations {
            seen.insert(o.id.clone());
            let pos = self.known.iter().position(|r| r.id == o.id);
            let status = match (o.state, pos.map(|p| self.known[p].status)) {
                (State::Battery { percent, charging }, _) => Status::Battery { percent, charging },
                (State::ChargingNoLevel, _) => Status::ChargingNoLevel,
                // Sem resposta (dormindo, em geral): a bateria nao muda nesse meio tempo,
                // entao mantem a ultima leitura como esta.
                (State::Unreachable, Some(prev @ (Status::Battery { .. } | Status::ChargingNoLevel))) => prev,
                (State::Unreachable | State::Off, _) => Status::NotConnected,
            };
            match pos {
                Some(p) => {
                    let r = &mut self.known[p];
                    r.status = status;
                    if let Some(name) = o.name {
                        r.name = name;
                    }
                    if let Some(kind) = o.kind {
                        r.kind = kind;
                    }
                }
                None => {
                    let Some(name) = o.name else { continue };
                    let kind = o.kind.unwrap_or_else(|| Kind::from_name(&name));
                    self.known.push(Reading { id: o.id, name, kind, status });
                }
            }
        }
        for r in &mut self.known {
            if !seen.contains(&r.id) {
                r.status = Status::NotConnected;
            }
        }
        self.known.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
        self.known.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(state: State) -> Observation {
        Observation { id: "x".into(), name: Some("Mouse".into()), kind: Some(Kind::Mouse), state }
    }

    #[test]
    fn kind_from_name() {
        assert_eq!(Kind::from_name("Renan's AirPods"), Kind::Earbuds);
        assert_eq!(Kind::from_name("AirPods Max"), Kind::Headset);
        assert_eq!(Kind::from_name("Galaxy Buds2 Pro"), Kind::Earbuds);
        assert_eq!(Kind::from_name("PRO X Wireless Gaming Headset"), Kind::Headset);
        assert_eq!(Kind::from_name("MX Master 3S Mouse"), Kind::Mouse);
    }

    #[test]
    fn sleeping_device_keeps_last_reading() {
        let mut t = Tracker::default();
        t.update(vec![obs(State::Battery { percent: 80, charging: false })]);
        let r = t.update(vec![obs(State::Unreachable)]);
        assert_eq!(r[0].status, Status::Battery { percent: 80, charging: false });
    }

    #[test]
    fn vanished_device_is_not_connected() {
        let mut t = Tracker::default();
        t.update(vec![obs(State::Battery { percent: 80, charging: false })]);
        assert_eq!(t.update(vec![])[0].status, Status::NotConnected);
    }

    #[test]
    fn unknown_unreachable_device_is_ignored() {
        let mut t = Tracker::default();
        let mut o = obs(State::Unreachable);
        o.name = None;
        assert!(t.update(vec![o]).is_empty());
    }
}
