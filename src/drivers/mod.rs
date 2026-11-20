//! Leitores de fabricantes portados de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0).
//!
//! Cada leitor olha as colecoes HID enumeradas, conversa com os dispositivos que reconhece e
//! adiciona o que encontrou. Um leitor "reivindica" um dispositivo (VID/PID) depois de trata-lo,
//! para os leitores genericos no fim da fila nao listarem o mesmo aparelho duas vezes.
//! Todas as consultas sao so de leitura de bateria; nada e gravado na configuracao.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::device::{Kind, Observation, State};
use crate::hid::{DeviceInfo, HidApi, HidDevice, Mode};

/// Leitores na ordem do devices.cpp do krisgfx9 (os especificos antes dos genericos).
type Poll = fn(&mut Ctx);

const ORDER: &[(&str, Poll)] = &[
    ("sinowealth", misc_new::poll_sinowealth),
    ("glorious", glorious::poll),
    ("aula", aula::poll),
    ("dareu", dareu::poll),
    ("atk_keyboard", evision::poll_atk_keyboard),
    ("redragon_evision", misc_new::poll_redragon_evision),
    ("pulsar", pulsar::poll),
    ("atk", atk::poll),
    ("kysona", kysona::poll),
    ("cherry", evision::poll_cherry),
    ("keychron", keychron::poll),
    ("lamzu", lamzu::poll),
    ("endgame", endgame::poll),
    ("vaxee", vaxee::poll),
    ("wlmouse", wlmouse::poll),
    ("nordic", nordic::poll),
    ("akko", evision::poll_akko),
    ("rongyuan", rongyuan::poll),
    ("sonix", sonix::poll),
    ("inzone", inzone::poll),
    ("fractal", fractal::poll),
    ("jabra", jabra::poll),
    ("jbl", jbl::poll),
    ("rog", rog::poll),
    ("zowie", zowie::poll),
    ("rk", rk::poll),
    ("mchose_mouse", mchose_mouse::poll),
    ("mchose_a5", mchose_new::poll_a5),
    ("rapoo", rapoo::poll),
    ("areson", areson::poll),
    ("mchose_g3", mchose_mouse::poll_g3),
    ("mchose_modern", mchose_new::poll_modern),
    ("mchose_headset", mchose_new::poll_headset),
    ("mchose_keyboard", mchose_new::poll_keyboard),
    ("misc_mice", misc_mice::poll),
    ("fantech", fantech::poll),
    ("raigor", misc_new::poll_raigor),
    ("razer_headset", razer_headset::poll),
    ("hyperx", hyperx::poll),
    ("logitech_g435", logitech_g435::poll),
    ("razer", razer::poll),
    ("steelseries", steelseries::poll),
    ("audeze", audeze::poll),
    ("corsair", corsair::poll),
    ("coolermaster", coolermaster::poll),
    ("switch_pro", switch_pro::poll),
    ("steam_controller", steam_controller::poll),
    ("gamesir", gamesir::poll),
    ("flydigi", flydigi::poll),
    ("eightbitdo", eightbitdo::poll),
    ("beitong", beitong::poll),
    ("pressplay", misc_new::poll_pressplay),
    ("generic", generic::poll),
];

/// Roda todos os leitores; `claimed` sao os (VID, PID) que os leitores proprios ja trataram.
pub fn poll_all(api: &HidApi, claimed: &[(u16, u16)]) -> Vec<Observation> {
    let mut ctx = Ctx::new(api);
    for &(v, p) in claimed {
        ctx.claim(v, p);
    }
    for (name, poll) in ORDER {
        let before = ctx.out.len();
        let t = Instant::now();
        poll(&mut ctx);
        if crate::verbose() && (ctx.out.len() > before || t.elapsed().as_millis() > 300) {
            eprintln!("[{name}] {} ms: {:?}", t.elapsed().as_millis(), &ctx.out[before..]);
        }
    }
    ctx.out
}

pub mod areson;
pub mod beitong;
pub mod eightbitdo;
pub mod flydigi;
pub mod gamesir;
pub mod steam_controller;
pub mod switch_pro;
pub mod atk;
pub mod audeze;
pub mod coolermaster;
pub mod corsair;
pub mod aula;
pub mod dareu;
pub mod endgame;
pub mod evision;
pub mod fantech;
pub mod fractal;
pub mod generic;
pub mod glorious;
pub mod hyperx;
pub mod inzone;
pub mod jabra;
pub mod jbl;
pub mod keychron;
pub mod kysona;
pub mod lamzu;
pub mod logitech_g435;
pub mod mchose_mouse;
pub mod mchose_new;
pub mod misc_mice;
pub mod misc_new;
pub mod nordic;
pub mod pulsar;
pub mod rapoo;
pub mod razer;
pub mod razer_headset;
pub mod rk;
pub mod rog;
pub mod rongyuan;
pub mod sonix;
pub mod steelseries;
pub mod vaxee;
pub mod wlmouse;
pub mod zowie;

/// Estado de uma rodada: colecoes enumeradas, dispositivos ja tratados e o resultado.
pub struct Ctx<'a> {
    pub api: &'a HidApi,
    claimed: HashSet<(u16, u16)>,
    pub out: Vec<Observation>,
}

impl<'a> Ctx<'a> {
    pub fn new(api: &'a HidApi) -> Self {
        Self { api, claimed: HashSet::new(), out: Vec::new() }
    }

    pub fn is_claimed(&self, vid: u16, pid: u16) -> bool {
        self.claimed.contains(&(vid, pid))
    }

    pub fn claim(&mut self, vid: u16, pid: u16) {
        self.claimed.insert((vid, pid));
    }

    pub fn add(&mut self, s: Status, vid: u16, pid: u16) {
        self.out.push(s.into_observation());
        self.claim(vid, pid);
    }

    /// Pares (VID, PID) distintos de um fabricante ainda nao reivindicados.
    pub fn products(&self, vid: u16) -> Vec<(u16, u16)> {
        let mut v: Vec<(u16, u16)> = self
            .api
            .device_list()
            .filter(|c| c.vendor_id() == vid && !self.is_claimed(c.vendor_id(), c.product_id()))
            .map(|c| (c.vendor_id(), c.product_id()))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Colecoes de um produto que passam no filtro.
    pub fn find(&self, vid: u16, pid: u16, pred: impl Fn(&DeviceInfo) -> bool) -> Vec<DeviceInfo> {
        self.api
            .device_list()
            .filter(|c| c.vendor_id() == vid && c.product_id() == pid && pred(c))
            .cloned()
            .collect()
    }

    pub fn open(&self, c: &DeviceInfo) -> Option<HidDevice> {
        self.api.open(c, Mode::ReadWrite).ok()
    }

    pub fn open_read_only(&self, c: &DeviceInfo) -> Option<HidDevice> {
        self.api.open(c, Mode::ReadOnly).ok()
    }

    pub fn open_feature_only(&self, c: &DeviceInfo) -> Option<HidDevice> {
        self.api.open(c, Mode::FeatureOnly).ok()
    }

    /// Tipo pelo conjunto de colecoes de topo (mouse > controle > teclado).
    pub fn kind_from_collections(&self, vid: u16, pid: u16) -> Kind {
        let (mut mouse, mut pad, mut kbd) = (false, false, false);
        for c in self.api.device_list().filter(|c| c.vendor_id() == vid && c.product_id() == pid && c.usage_page() == 0x01) {
            mouse |= c.usage() == 0x02;
            pad |= matches!(c.usage(), 0x04 | 0x05);
            kbd |= c.usage() == 0x06;
        }
        if mouse {
            Kind::Mouse
        } else if pad {
            Kind::Gamepad
        } else if kbd {
            Kind::Keyboard
        } else {
            Kind::Other
        }
    }
}

/// Leitura de um dispositivo, no formato dos leitores originais.
#[derive(Clone, Debug)]
pub struct Status {
    pub key: String,
    pub name: String,
    pub kind: Kind,
    /// 0..=100, ou -1 quando o nivel nao e conhecido agora.
    pub percent: i32,
    pub charging: bool,
    /// Receptor respondeu, mas o dispositivo esta dormindo: mantem o ultimo nivel.
    pub asleep: bool,
    /// Falso: receptor presente com o dispositivo desligado (some da barra).
    pub connected: bool,
    /// Detalhe extra, ex.: "E 80%, D 75%, estojo 40%".
    pub detail: String,
}

impl Status {
    pub fn new(key: impl Into<String>, name: impl Into<String>, kind: Kind) -> Self {
        Self { key: key.into(), name: name.into(), kind, percent: -1, charging: false, asleep: false, connected: true, detail: String::new() }
    }

    pub fn into_observation(self) -> Observation {
        let state = if !self.connected {
            State::Off
        } else if self.asleep {
            State::Unreachable
        } else if self.percent < 0 {
            if self.charging {
                State::ChargingNoLevel
            } else {
                State::Unreachable
            }
        } else {
            State::Battery { percent: self.percent.clamp(0, 100) as u8, charging: self.charging }
        };
        let name = if self.detail.is_empty() { self.name } else { format!("{} ({})", self.name, self.detail) };
        Observation { id: self.key, name: Some(name), kind: Some(self.kind), state }
    }
}

pub fn hex4(v: u16) -> String {
    format!("{v:04X}")
}

pub fn clamp100(v: i32) -> i32 {
    v.clamp(0, 100)
}

/// Mapeia lo..hi para 0..100 linearmente, com limite.
pub fn map_to_100(v: i32, lo: i32, hi: i32) -> i32 {
    if hi <= lo {
        return -1;
    }
    clamp100((v - lo) * 100 / (hi - lo))
}

/// Interpolacao linear por partes em (mv[i], pct[i]); tabelas em ordem decrescente de tensao.
pub fn interpolate(mv: i32, mv_table: &[i32], pct_table: &[i32]) -> i32 {
    if mv >= mv_table[0] {
        return pct_table[0];
    }
    for i in 1..mv_table.len() {
        if mv >= mv_table[i] {
            let span = (mv_table[i - 1] - mv_table[i]).max(1);
            return pct_table[i] + (pct_table[i - 1] - pct_table[i]) * (mv - mv_table[i]) / span;
        }
    }
    pct_table[pct_table.len() - 1]
}

/// Soma de bytes modulo 256.
pub fn sum8(b: &[u8]) -> u8 {
    b.iter().fold(0u8, |a, x| a.wrapping_add(*x))
}

pub fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// Prazo absoluto para lacos de leitura.
pub struct Deadline(Instant);

impl Deadline {
    pub fn in_ms(ms: u64) -> Self {
        Self(Instant::now() + Duration::from_millis(ms))
    }
    /// Milissegundos restantes (0 quando acabou).
    pub fn left(&self) -> u32 {
        self.0.saturating_duration_since(Instant::now()).as_millis() as u32
    }
    pub fn expired(&self) -> bool {
        self.left() == 0
    }
}

/// Le input reports ate `accept` devolver algo ou o prazo acabar.
pub fn read_until<T>(d: &HidDevice, ms: u64, mut accept: impl FnMut(&[u8]) -> Option<T>) -> Option<T> {
    let end = Deadline::in_ms(ms);
    while !end.expired() {
        let r = d.read(end.left().max(1))?;
        if let Some(v) = accept(&r) {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_maps_to_state() {
        let mut s = Status::new("k", "Mouse", Kind::Mouse);
        assert_eq!(s.clone().into_observation().state, State::Unreachable);
        s.percent = 55;
        s.charging = true;
        assert_eq!(s.clone().into_observation().state, State::Battery { percent: 55, charging: true });
        s.connected = false;
        assert_eq!(s.clone().into_observation().state, State::Off);
        let mut c = Status::new("k", "Teclado", Kind::Keyboard);
        c.charging = true;
        assert_eq!(c.into_observation().state, State::ChargingNoLevel);
    }

    #[test]
    fn helpers() {
        assert_eq!(interpolate(3800, &[4200, 3800, 3500], &[100, 50, 0]), 50);
        assert_eq!(interpolate(4000, &[4200, 3800, 3500], &[100, 50, 0]), 75);
        assert_eq!(sum8(&[0x13, 0x4A]), 0x5D);
        assert_eq!(map_to_100(0x7F, 0x64, 0x9A), 50);
    }
}
