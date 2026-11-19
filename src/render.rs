//! Desenha o widget num bitmap com alfa por pixel e aplica com UpdateLayeredWindow,
//! para o texto se misturar com a barra de tarefas translucida.
//! Adaptado de renanleonel/windows-taskbar-battery (MIT).

use crate::device::{Kind, Status};
use crate::wide;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

pub const LOW_BATTERY: u8 = 20;
const BOLT: &str = "\u{E945}";
/// Newer glyph (clearer at small sizes); older Segoe Fluent Icons versions only have the pair.
const EARBUD_SINGLE: &str = "\u{F27C}";
const EARBUD_PAIR: &str = "\u{F4C0}";

/// Segoe Fluent Icons glyph per device kind.
fn icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Headset => "\u{E7F6}",
        Kind::Earbuds => EARBUD_SINGLE, // swapped for EARBUD_PAIR if the font lacks it
        Kind::Mouse => "\u{E962}",
        Kind::Keyboard => "\u{E765}",
        Kind::Gamepad => "\u{E7FC}",
        Kind::Phone => "\u{E8EA}", // cellphone
        Kind::Other => "\u{E83F}", // battery
    }
}

struct Seg {
    text: Vec<u16>,
    icon: bool,
    color: u32, // 0xRRGGBB
    opacity: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    gap_after: i32,
    /// Dispositivo ao qual o segmento pertence.
    item: usize,
    /// Icone de marca: brilho suave atras do glifo.
    halo: bool,
    /// Porcentagem de marca: barrinha de nivel (%, cor) embaixo do texto.
    bar: Option<(i32, u32)>,
}

/// Um dispositivo no widget: tipo, estado e a cor da marca (0xRRGGBB), se houver.
pub type Item = (Kind, Status, Option<u32>);

/// Como o widget e disposto na barra de tarefas (renanleonel, 933d3f5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// Barra horizontal desta altura: dispositivos em linha.
    Row { height: i32 },
    /// Barra vertical (esquerda/direita) desta largura: dispositivos empilhados, icone sobre o texto.
    Column { width: i32 },
}

/// Renders the widget and returns its (width, height) in physical pixels.
pub unsafe fn draw(hwnd: HWND, items: &[Item], dpi: u32, layout: Layout, light: bool) -> (i32, i32) {
    render(items, dpi, layout, light, |dc, width, height, _| {
        let size = SIZE { cx: width, cy: height };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(hwnd, null_mut(), null(), &size, dc, &origin, 0, &blend, ULW_ALPHA);
    })
}

/// Desenha o widget e entrega o DC, o tamanho e os pixels BGRA pre-multiplicados ao `sink`.
/// Usado pelo widget (UpdateLayeredWindow) e pelo `--snapshot` (PNG para o README).
pub unsafe fn render(items: &[Item], dpi: u32, layout: Layout, light: bool, sink: impl FnOnce(HDC, i32, i32, &[u32])) -> (i32, i32) {
    let s = |v: i32| v * dpi as i32 / 96;
    let fg = if light { 0x000000 } else { 0xFFFFFF };
    let red = if light { 0xC42B1C } else { 0xFF6B6B };
    let green = if light { 0x0F7B0F } else { 0x6CCB5F };

    let mut segs = Vec::new();
    let seg = |item: usize, text: &str, icon: bool, color: u32, opacity: u32, gap_after: i32| Seg {
        text: wide(text),
        icon,
        color,
        opacity,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        gap_after,
        item,
        halo: false,
        bar: None,
    };
    for (i, &(kind, status, accent)) in items.iter().enumerate() {
        let kind = &kind;
        let gap = if i == items.len() - 1 { 0 } else { s(12) };
        let icon_seg = |color: u32, opacity: u32, gap: i32| {
            let mut g = seg(i, icon(*kind), true, accent.unwrap_or(color), opacity, gap);
            g.halo = accent.is_some() && opacity == 255;
            g
        };
        let (label, color, opacity, charging, bar) = match status {
            Status::Battery { percent, charging } => {
                let low = percent <= LOW_BATTERY && !charging;
                let color = if low { red } else { fg };
                let bar = accent.map(|a| (percent as i32, if low { red } else { a }));
                (format!("{percent}%"), color, 255, charging, bar)
            }
            Status::ChargingNoLevel => {
                segs.push(icon_seg(fg, 255, s(3)));
                segs.push(seg(i, BOLT, true, green, 255, gap));
                continue;
            }
            Status::NotConnected => ("—".to_string(), fg, 110, false, None),
        };
        segs.push(icon_seg(color, opacity, s(5)));
        let mut text = seg(i, &label, false, color, opacity, if charging { s(2) } else { gap });
        text.bar = bar;
        segs.push(text);
        if charging {
            segs.push(seg(i, BOLT, true, green, opacity, gap));
        }
    }
    let bar_h = s(2).max(2);
    let bar_gap = s(3);
    let bar_room = bar_gap + bar_h;

    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    ReleaseDC(null_mut(), screen);
    let text_font = font("Segoe UI", -s(12));
    let mut icon_font = font("Segoe Fluent Icons", -s(15));

    // Windows 10 nao tem a Segoe Fluent Icons: usa a Segoe MDL2 Assets (mesmos codigos,
    // menos os fones intra-auriculares, que viram o icone de headset).
    SelectObject(dc, icon_font);
    if !has_glyph(dc, icon(Kind::Headset)) {
        DeleteObject(icon_font);
        icon_font = font("Segoe MDL2 Assets", -s(15));
        SelectObject(dc, icon_font);
    }
    if !has_glyph(dc, EARBUD_PAIR) {
        for seg in segs.iter_mut().filter(|s| s.text == wide(EARBUD_SINGLE)) {
            seg.text = wide(icon(Kind::Headset));
        }
    }

    // Fall back to the earbud pair if this font version lacks the single earbud.
    let single = wide(EARBUD_SINGLE);
    let mut glyph = 0u16;
    if GetGlyphIndicesW(dc, single.as_ptr(), 1, &mut glyph, GGI_MARK_NONEXISTING_GLYPHS) == GDI_ERROR as u32 || glyph == 0xFFFF {
        for seg in segs.iter_mut().filter(|s| s.text == single) {
            seg.text = wide(EARBUD_PAIR);
        }
    }

    // Medidas.
    for seg in &mut segs {
        SelectObject(dc, if seg.icon { icon_font } else { text_font });
        let mut size: SIZE = zeroed();
        GetTextExtentPoint32W(dc, seg.text.as_ptr(), seg.text.len() as i32 - 1, &mut size);
        seg.w = size.cx;
        seg.h = size.cy;
    }

    // Disposicao.
    let pad = s(6);
    let (width, height) = match layout {
        Layout::Row { height } => {
            let mut x = pad;
            for seg in &mut segs {
                seg.x = x;
                // Texto e barra de nivel centralizados juntos.
                let h = seg.h + if seg.bar.is_some() { bar_room } else { 0 };
                seg.y = (height - h) / 2;
                x += seg.w + seg.gap_after;
            }
            (x + pad, height)
        }
        Layout::Column { width } => {
            // Por dispositivo: o icone numa linha, o texto (e o raio) centralizado embaixo.
            let mut y = pad;
            for i in 0..items.len() {
                let mut it = segs.iter_mut().filter(|s| s.item == i);
                let first = it.next();
                let rest: Vec<_> = it.collect();
                if let Some(icon) = first {
                    icon.x = (width - icon.w) / 2;
                    icon.y = y;
                    y += icon.h + s(1);
                }
                let line_w: i32 = rest.iter().map(|s| s.w).sum::<i32>() + rest.iter().rev().skip(1).map(|s| s.gap_after).sum::<i32>();
                let line_h = rest.iter().map(|s| s.h).max().unwrap_or(0);
                let mut x = (width - line_w) / 2;
                for seg in rest {
                    seg.x = x;
                    seg.y = y + (line_h - seg.h) / 2;
                    x += seg.w + seg.gap_after;
                }
                y += line_h;
                if segs.iter().any(|s| s.item == i && s.bar.is_some()) {
                    y += bar_room;
                }
                if i + 1 < items.len() {
                    y += s(8);
                }
            }
            (width, y + pad)
        }
    };

    // White-on-black text into a 32bpp DIB; coverage becomes alpha below.
    let mut bi: BITMAPINFO = zeroed();
    bi.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height, // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB as _,
        ..zeroed()
    };
    let mut bits: *mut c_void = null_mut();
    let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if bmp.is_null() || bits.is_null() {
        DeleteObject(text_font);
        DeleteObject(icon_font);
        DeleteDC(dc);
        return (width, height);
    }
    let old_bmp = SelectObject(dc, bmp);
    SetBkMode(dc, TRANSPARENT as _);
    SetTextColor(dc, 0x00FF_FFFF);
    for seg in &segs {
        SelectObject(dc, if seg.icon { icon_font } else { text_font });
        TextOutW(dc, seg.x, seg.y, seg.text.as_ptr(), seg.text.len() as i32 - 1);
    }
    GdiFlush();

    // Cobertura do texto (branco sobre preto) -> camadas de cor compostas por cima umas das outras:
    // brilhos das marcas, glifos e textos, barras de nivel.
    let px = std::slice::from_raw_parts_mut(bits as *mut u32, (width * height) as usize);
    let cov: Vec<u32> = px.iter().map(|p| ((p >> 16) & 0xFF).max((p >> 8) & 0xFF).max(p & 0xFF)).collect();
    let mut canvas = Canvas { w: width, h: height, px: vec![0u32; (width * height) as usize] };
    let halo_r = s(3);
    for seg in segs.iter().filter(|g| g.halo) {
        let (gw, gh) = (seg.w + 2 * halo_r, seg.h + 2 * halo_r);
        let mut m = vec![0u32; (gw * gh) as usize];
        for y in 0..seg.h {
            for x in 0..seg.w {
                let (sx, sy) = (seg.x + x, seg.y + y);
                if sx >= 0 && sy >= 0 && sx < width && sy < height {
                    m[((y + halo_r) * gw + x + halo_r) as usize] = cov[(sy * width + sx) as usize];
                }
            }
        }
        let m = blur(&m, gw, gh, halo_r);
        let alpha = if light { 60 } else { 140 };
        for y in 0..gh {
            for x in 0..gw {
                let a = (m[(y * gw + x) as usize] * 3).min(255) * alpha / 255;
                canvas.blend(seg.x - halo_r + x, seg.y - halo_r + y, seg.color, a);
            }
        }
    }
    let mut done = vec![false; cov.len()];
    for seg in &segs {
        for y in seg.y.max(0)..(seg.y + seg.h).min(height) {
            for x in seg.x.max(0)..(seg.x + seg.w).min(width) {
                let i = (y * width + x) as usize;
                if !done[i] {
                    done[i] = true;
                    canvas.blend(x, y, seg.color, cov[i] * seg.opacity / 255);
                }
            }
        }
    }
    // Sobras do antisserrilhado fora das caixas medidas: cor do texto.
    for (i, c) in cov.iter().enumerate() {
        if !done[i] && *c > 0 {
            canvas.blend(i as i32 % width, i as i32 / width, fg, *c);
        }
    }
    let track = if light { 0x000000 } else { 0xFFFFFF };
    for seg in &segs {
        let Some((pct, color)) = seg.bar else { continue };
        // A barra ocupa a largura do texto, como a barra de bateria do Synapse.
        let by = seg.y + seg.h + bar_gap;
        let fill = ((pct * seg.w + 50) / 100).max(if pct > 0 { 1 } else { 0 });
        for y in by..by + bar_h {
            for x in 0..seg.w {
                let (c, a) = if x < fill { (color, seg.opacity) } else { (track, 45 * seg.opacity / 255) };
                canvas.blend(seg.x + x, y, c, a);
            }
        }
    }
    for (p, c) in px.iter_mut().zip(&canvas.px) {
        // Alfa 1 deixa o retangulo todo clicavel sem aparecer.
        *p = if *c >> 24 == 0 { 0x0100_0000 } else { *c };
    }

    sink(dc, width, height, px);

    SelectObject(dc, old_bmp);
    DeleteObject(bmp);
    DeleteObject(text_font);
    DeleteObject(icon_font);
    DeleteDC(dc);
    (width, height)
}

/// Pixels BGRA pre-multiplicados com composicao "source over".
struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u32>,
}

impl Canvas {
    fn blend(&mut self, x: i32, y: i32, color: u32, a: u32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || a == 0 {
            return;
        }
        let a = a.min(255);
        let d = &mut self.px[(y * self.w + x) as usize];
        let inv = 255 - a;
        let ch = |shift: u32| ((color >> shift) & 0xFF) * a / 255 + ((*d >> shift) & 0xFF) * inv / 255;
        let oa = a + (*d >> 24) * inv / 255;
        *d = (oa << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0);
    }
}

/// Desfoque de caixa (duas passadas) de uma mascara de cobertura, para o brilho das marcas.
fn blur(m: &[u32], w: i32, h: i32, r: i32) -> Vec<u32> {
    let n = (2 * r + 1) as u32;
    let mut a = m.to_vec();
    let mut tmp = vec![0u32; a.len()];
    for _ in 0..2 {
        for y in 0..h {
            for x in 0..w {
                let s: u32 = (-r..=r).filter(|k| (0..w).contains(&(x + k))).map(|k| a[(y * w + x + k) as usize]).sum();
                tmp[(y * w + x) as usize] = s / n;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let s: u32 = (-r..=r).filter(|k| (0..h).contains(&(y + k))).map(|k| tmp[((y + k) * w + x) as usize]).sum();
                a[(y * w + x) as usize] = s / n;
            }
        }
    }
    a
}

unsafe fn has_glyph(dc: HDC, ch: &str) -> bool {
    let w = wide(ch);
    let mut glyph = 0u16;
    GetGlyphIndicesW(dc, w.as_ptr(), 1, &mut glyph, GGI_MARK_NONEXISTING_GLYPHS) != GDI_ERROR as u32 && glyph != 0xFFFF
}

unsafe fn font(face: &str, height: i32) -> HFONT {
    let mut lf: LOGFONTW = zeroed();
    lf.lfHeight = height;
    lf.lfWeight = FW_NORMAL as _;
    lf.lfQuality = ANTIALIASED_QUALITY as _; // grayscale AA: coverage maps cleanly to alpha
    for (dst, src) in lf.lfFaceName.iter_mut().zip(face.encode_utf16().take(31)) {
        *dst = src;
    }
    CreateFontIndirectW(&lf)
}
