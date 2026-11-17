//! Uma rodada de leitura de todas as fontes de bateria.

use crate::device::{Observation, Reading, Tracker};
use crate::{aula, bluetooth, logitech, mchose, xbox};

/// O Bluetooth acorda o radio do aparelho: so e lido a cada N rodadas (N x 60 s = 3 min).
const BT_EVERY: u32 = 3;

pub struct Scanner {
    api: Option<hidapi::HidApi>,
    logi: logitech::Logitech,
    tracker: Tracker,
    bt: Vec<Observation>,
    tick: u32,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            api: hidapi::HidApi::new().ok(),
            logi: logitech::Logitech::new(),
            tracker: Tracker::default(),
            bt: Vec::new(),
            tick: 0,
        }
    }

    pub fn scan(&mut self, force_bt: bool) -> Vec<Reading> {
        let mut obs = Vec::new();
        if let Some(api) = self.api.as_mut() {
            let _ = api.refresh_devices();
            obs.extend(self.logi.scan(api));
            obs.extend(mchose::scan(api));
            obs.extend(aula::scan(api));
        }
        obs.extend(xbox::scan());
        if force_bt || self.tick.is_multiple_of(BT_EVERY) {
            self.bt = bluetooth::scan();
        }
        // Entre leituras de Bluetooth, repete o ultimo resultado para nao parecer desconectado.
        obs.extend(self.bt.iter().cloned());
        self.tick += 1;
        self.tracker.update(obs)
    }
}
