//! Cores das marcas no widget: o icone na cor do fabricante com um brilho suave e uma barrinha de
//! nivel embaixo da porcentagem (vermelha quando baixa).
//!
//! Tabela de cores e deteccao portadas de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0),
//! brands.cpp: cada marca tem um tom para a barra escura (a cor oficial, clareada so onde fica
//! escura demais) e outro para a barra clara (com contraste de pelo menos 3:1 sobre #F3F3F3).
//! A marca vem primeiro do prefixo da chave do leitor, depois do nome.

use crate::device::Kind;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brand {
    Razer,
    Logitech,
    SteelSeries,
    Corsair,
    HyperX,
    Rog,
    Zowie,
    PlayStation,
    Xbox,
    Samsung,
    Apple,
    Jabra,
    Jbl,
    CoolerMaster,
    Steam,
    Redragon,
}

impl Brand {
    /// 0xRRGGBB para a barra escura e para a clara.
    pub fn color(self, light: bool) -> u32 {
        let (dark, lt) = match self {
            Brand::Razer => (0x44D62C, 0x168C14),
            Brand::Logitech => (0x00B8FC, 0x0078B4),
            Brand::SteelSeries => (0xFF5200, 0xCC4100),
            Brand::Corsair => (0xF5E41A, 0x967A00),
            Brand::HyperX => (0xE8233F, 0xC8102E),
            Brand::Rog => (0xFF2E4D, 0xD0002A),
            Brand::Zowie => (0xF03C32, 0xC0201A),
            Brand::PlayStation => (0x1F8BFF, 0x0062C4),
            Brand::Xbox => (0x22A822, 0x107C10),
            Brand::Samsung => (0x5B7BFF, 0x1428A0),
            Brand::Apple => (0xC7C7CC, 0x6E6E73),
            Brand::Jabra => (0xFFC800, 0x8C6A00),
            Brand::Jbl => (0xFF3D00, 0xCC3400),
            Brand::CoolerMaster => (0x9D6BFF, 0x6A2BD9),
            Brand::Steam => (0x1A9FFF, 0x0B6FBF),
            Brand::Redragon => (0xE60012, 0xC0000F),
        };
        if light {
            lt
        } else {
            dark
        }
    }
}

const KEYS: &[(&str, Brand)] = &[
    ("razer:", Brand::Razer),
    ("blackshark:", Brand::Razer),
    ("barracuda:", Brand::Razer),
    ("logi:", Brand::Logitech),
    ("g435:", Brand::Logitech),
    ("steelseries:", Brand::SteelSeries),
    ("corsair:", Brand::Corsair),
    ("hyperx:", Brand::HyperX),
    ("rog:", Brand::Rog),
    ("zowie:", Brand::Zowie),
    ("ps:", Brand::PlayStation),
    ("playstation:", Brand::PlayStation),
    ("inzone:", Brand::PlayStation),
    ("xinput:", Brand::Xbox),
    ("jabra:", Brand::Jabra),
    ("jbl:", Brand::Jbl),
    ("coolermaster:", Brand::CoolerMaster),
    ("steam:", Brand::Steam),
    ("areson:", Brand::Redragon),
];

#[derive(Clone, Copy)]
enum How {
    Contains,
    Word,
    Prefix,
    Exact,
}

const NAMES: &[(&str, How, Brand)] = &[
    ("razer", How::Contains, Brand::Razer),
    ("logitech", How::Contains, Brand::Logitech),
    ("logi ", How::Prefix, Brand::Logitech),
    // Nomes Bluetooth costumam vir sem a marca: "MX Master 3S", "LIFT", "POP Mouse", "Pebble M350".
    ("mx ", How::Prefix, Brand::Logitech),
    ("lift", How::Prefix, Brand::Logitech),
    ("pop mouse", How::Prefix, Brand::Logitech),
    ("pop keys", How::Prefix, Brand::Logitech),
    ("pebble", How::Prefix, Brand::Logitech),
    ("ergo k", How::Prefix, Brand::Logitech),
    ("ergo m", How::Prefix, Brand::Logitech),
    ("steelseries", How::Contains, Brand::SteelSeries),
    ("arctis", How::Word, Brand::SteelSeries),
    ("aerox", How::Word, Brand::SteelSeries),
    ("rival ", How::Prefix, Brand::SteelSeries),
    ("corsair", How::Contains, Brand::Corsair),
    ("hyperx", How::Contains, Brand::HyperX),
    ("pulsefire", How::Word, Brand::HyperX),
    ("rog", How::Word, Brand::Rog),
    ("zowie", How::Contains, Brand::Zowie),
    ("dualsense", How::Contains, Brand::PlayStation),
    ("dualshock", How::Contains, Brand::PlayStation),
    ("playstation", How::Contains, Brand::PlayStation),
    ("inzone", How::Word, Brand::PlayStation),
    ("wireless controller", How::Exact, Brand::PlayStation), // nome Bluetooth do DualSense / DualShock 4
    ("xbox", How::Word, Brand::Xbox),
    ("galaxy ", How::Prefix, Brand::Samsung),
    ("samsung", How::Word, Brand::Samsung),
    ("airpods", How::Contains, Brand::Apple),
    ("magic mouse", How::Prefix, Brand::Apple),
    ("magic keyboard", How::Prefix, Brand::Apple),
    ("magic trackpad", How::Prefix, Brand::Apple),
    ("apple", How::Word, Brand::Apple),
    ("iphone", How::Prefix, Brand::Apple),
    ("jabra", How::Word, Brand::Jabra),
    ("jbl", How::Word, Brand::Jbl),
    ("cooler master", How::Contains, Brand::CoolerMaster),
    ("coolermaster", How::Contains, Brand::CoolerMaster),
    ("steam controller", How::Contains, Brand::Steam),
    ("redragon", How::Contains, Brand::Redragon),
];

/// `w` como palavra inteira: "rog" casa com "ROG Harpe", nao com "Progress".
fn has_word(n: &str, w: &str) -> bool {
    n.match_indices(w).any(|(at, _)| {
        let before = n[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = n[at + w.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        before && after
    })
}

/// Logitech com nome que e so o modelo: "K380", "M720 Triathlon", "G915", "G502 X".
fn logitech_model(n: &str) -> bool {
    let b = n.as_bytes();
    b.len() >= 4
        && matches!(b[0], b'g' | b'k' | b'm')
        && b[1..4].iter().all(u8::is_ascii_digit)
        && (b.len() == 4 || !b[4].is_ascii_alphanumeric())
}

/// Fones Sony (WH-1000XM5, WF-1000XM4) nao sao PlayStation.
fn sony_audio(n: &str) -> bool {
    n.starts_with("wh-") || n.starts_with("wf-") || has_word(n, "linkbuds")
}

pub fn brand_of(id: &str, name: &str, kind: Kind) -> Option<Brand> {
    if let Some((_, b)) = KEYS.iter().find(|(p, _)| id.starts_with(p)) {
        return Some(*b);
    }
    let n = name.to_lowercase();
    if sony_audio(&n) {
        return None;
    }
    if kind == Kind::Gamepad && n.contains("xbox") {
        return Some(Brand::Xbox);
    }
    for &(text, how, b) in NAMES {
        let hit = match how {
            How::Contains => n.contains(text),
            How::Word => has_word(&n, text),
            How::Prefix => n.starts_with(text),
            How::Exact => n == text,
        };
        if hit {
            return Some(b);
        }
    }
    logitech_model(&n).then_some(Brand::Logitech)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_brands() {
        assert_eq!(brand_of("logi:c54d:2:01", "PRO X 2", Kind::Mouse), Some(Brand::Logitech));
        assert_eq!(brand_of("bt:1", "MX Master 3S", Kind::Mouse), Some(Brand::Logitech));
        assert_eq!(brand_of("bt:2", "K380", Kind::Keyboard), Some(Brand::Logitech));
        assert_eq!(brand_of("bt:3", "ASUS ROG Delta", Kind::Headset), Some(Brand::Rog));
        assert_eq!(brand_of("bt:4", "Progress Bar", Kind::Other), None);
        assert_eq!(brand_of("bt:5", "WH-1000XM5", Kind::Headset), None);
        assert_eq!(brand_of("bt:6", "AirPods Pro", Kind::Earbuds), Some(Brand::Apple));
        assert_eq!(brand_of("bt:7", "Wireless Controller", Kind::Gamepad), Some(Brand::PlayStation));
        assert_eq!(brand_of("mchose:v9pro", "MCHOSE V9 Pro", Kind::Headset), None);
        assert_eq!(brand_of("aula:fa09", "AULA F75", Kind::Keyboard), None);
    }
}
