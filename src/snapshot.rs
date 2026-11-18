//! `--snapshot <pasta>` (so com `--features snapshot`): desenha o widget com o mesmo codigo
//! do app e salva PNGs com transparencia, em 2x (192 DPI), para as imagens do README.

use crate::device::{Kind, Reading, Status};
use crate::render;
use std::path::Path;

/// Barra de tarefas do Windows 11: 48 px logicos.
const DPI: u32 = 192;
const HEIGHT: i32 = 48 * DPI as i32 / 96;

pub fn run(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let all = crate::widget::demo_readings();
    let only = |kind: Kind| all.iter().filter(|r| r.kind == kind).cloned().collect::<Vec<_>>();
    let one = |name: &str, kind: Kind, status: Status| vec![Reading { id: name.into(), name: name.into(), kind, status }];

    let scenes: Vec<(&str, Vec<Reading>, bool)> = vec![
        ("all-dark", all.clone(), false),
        ("all-light", all.clone(), true),
        ("single-mouse-dark", only(Kind::Mouse), false),
        ("single-mouse-light", only(Kind::Mouse), true),
        ("state-normal", one("Mouse", Kind::Mouse, Status::Battery { percent: 87, charging: false }), false),
        ("state-low", one("Earbuds", Kind::Earbuds, Status::Battery { percent: 12, charging: false }), false),
        ("state-charging", one("Headset", Kind::Headset, Status::Battery { percent: 64, charging: true }), false),
        ("state-charging-no-level", one("Keyboard", Kind::Keyboard, Status::ChargingNoLevel), false),
        ("kind-headset", one("Headset", Kind::Headset, Status::Battery { percent: 80, charging: false }), false),
        ("kind-earbuds", one("Earbuds", Kind::Earbuds, Status::Battery { percent: 80, charging: false }), false),
        ("kind-mouse", one("Mouse", Kind::Mouse, Status::Battery { percent: 80, charging: false }), false),
        ("kind-keyboard", one("Keyboard", Kind::Keyboard, Status::Battery { percent: 80, charging: false }), false),
        ("kind-gamepad", one("Gamepad", Kind::Gamepad, Status::Battery { percent: 80, charging: false }), false),
        ("kind-other", one("Other", Kind::Other, Status::Battery { percent: 80, charging: false }), false),
    ];

    for (name, readings, light) in scenes {
        let items: Vec<_> = readings.iter().map(|r| (r.kind, r.status)).collect();
        let mut out = None;
        unsafe {
            render::render(&items, DPI, render::Layout::Row { height: HEIGHT }, light, |_, width, _, px| out = Some((width, px.to_vec())));
        }
        let Some((width, px)) = out else { continue };
        let path = dir.join(format!("{name}.png"));
        match std::fs::write(&path, png(width as u32, HEIGHT as u32, &px)) {
            Ok(()) => println!("{}", path.display()),
            Err(e) => println!("{}: {e}", path.display()),
        }
    }
}

/// PNG RGBA sem compressao (blocos "stored" do deflate). As imagens sao pequenas e sao
/// otimizadas antes de ir para o repositorio.
fn png(w: u32, h: u32, bgra_premul: &[u32]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((w * 4 + 1) * h) as usize);
    for row in bgra_premul.chunks(w as usize) {
        raw.push(0); // filtro: nenhum
        for p in row {
            let a = p >> 24;
            // Alfa 1 e o truque do widget para continuar clicavel: aqui vira transparente.
            let (a, un) = if a <= 1 { (0, 0) } else { (a, 1) };
            let c = |shift: u32| if un == 0 { 0 } else { (((p >> shift) & 0xFF) * 255 / a).min(255) as u8 };
            raw.extend_from_slice(&[c(16), c(8), c(0), a as u8]);
        }
    }

    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(b) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bits, RGBA

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    for (kind, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &z[..]), (b"IEND", &[][..])] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &x in data {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_is_valid() {
        // 2x1: um pixel vermelho opaco, um transparente.
        let data = super::png(2, 1, &[0xFFFF_0000, 0x0100_0000]);
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(super::crc32(b"IEND"), 0xAE42_6082);
    }
}
