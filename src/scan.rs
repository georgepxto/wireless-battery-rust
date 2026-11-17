//! Uma rodada de leitura de todas as fontes de bateria.

use crate::device::{Observation, Reading, Tracker};
use crate::{aula, bluetooth, headsetcontrol, logitech, mchose, playstation, xbox};

/// Bluetooth e HeadsetControl (que abre um processo) so a cada N rodadas (N x 60 s = 3 min).
const SLOW_EVERY: u32 = 3;

const LOGITECH_VID: u16 = 0x046D;
const MCHOSE_VID: u16 = 0x291D;
/// MCHOSE V9 Pro pelo cabo USB (chip C-Media).
const MCHOSE_WIRED: (u16, u16) = (0x0D8C, 0x0343);

pub struct Scanner {
    api: Option<hidapi::HidApi>,
    logi: logitech::Logitech,
    tracker: Tracker,
    slow: Vec<Observation>,
    tick: u32,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            api: hidapi::HidApi::new().ok(),
            logi: logitech::Logitech::new(),
            tracker: Tracker::default(),
            slow: Vec::new(),
            tick: 0,
        }
    }

    pub fn scan(&mut self, force_slow: bool) -> Vec<Reading> {
        let mut obs = Vec::new();
        if let Some(api) = self.api.as_mut() {
            let _ = api.refresh_devices();
            obs.extend(self.logi.scan(api));
            obs.extend(mchose::scan(api));
            obs.extend(aula::scan(api));
            obs.extend(playstation::scan(api));
        }
        obs.extend(xbox::scan());
        if force_slow || self.tick.is_multiple_of(SLOW_EVERY) {
            self.slow = bluetooth::scan();
            if let Some(exe) = headsetcontrol::find() {
                // Nao lista de novo o que ja tem leitor proprio.
                let skip = |vid: u16, pid: u16| {
                    vid == MCHOSE_VID || (vid, pid) == MCHOSE_WIRED || (vid == LOGITECH_VID && obs.iter().any(|o| o.id.starts_with(&format!("logi:{pid:04x}:"))))
                };
                self.slow.extend(headsetcontrol::scan(&exe, skip));
            }
        }
        // Entre leituras lentas, repete o ultimo resultado para nao parecer desconectado.
        obs.extend(self.slow.iter().cloned());
        self.tick += 1;
        self.tracker.update(obs)
    }
}
