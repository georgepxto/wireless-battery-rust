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
    w: i32,
    h: i32,
    gap_after: i32,
}

/// Renders the widget and returns its width in physical pixels.
pub unsafe fn draw(hwnd: HWND, items: &[(Kind, Status)], dpi: u32, height: i32, light: bool) -> i32 {
    render(items, dpi, height, light, |dc, width, _| {
        let size = SIZE { cx: width, cy: height };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(hwnd, null_mut(), null(), &size, dc, &origin, 0, &blend, ULW_ALPHA);
    })
}

/// Desenha o widget e entrega o DC, a largura e os pixels BGRA pre-multiplicados ao `sink`.
/// Usado pelo widget (UpdateLayeredWindow) e pelo `--snapshot` (PNG para o README).
pub unsafe fn render(items: &[(Kind, Status)], dpi: u32, height: i32, light: bool, sink: impl FnOnce(HDC, i32, &[u32])) -> i32 {
    let s = |v: i32| v * dpi as i32 / 96;
    let fg = if light { 0x000000 } else { 0xFFFFFF };
    let red = if light { 0xC42B1C } else { 0xFF6B6B };
    let green = if light { 0x0F7B0F } else { 0x6CCB5F };

    let mut segs = Vec::new();
    let seg = |text: &str, icon: bool, color: u32, opacity: u32, gap_after: i32| Seg {
        text: wide(text),
        icon,
        color,
        opacity,
        x: 0,
        w: 0,
        h: 0,
        gap_after,
    };
    for (i, (kind, status)) in items.iter().enumerate() {
        let gap = if i == items.len() - 1 { 0 } else { s(12) };
        let (label, color, opacity, charging) = match *status {
            Status::Battery { percent, charging } => {
                let color = if percent <= LOW_BATTERY && !charging { red } else { fg };
                (format!("{percent}%"), color, 255, charging)
            }
            Status::ChargingNoLevel => {
                segs.push(seg(icon(*kind), true, fg, 255, s(3)));
                segs.push(seg(BOLT, true, green, 255, gap));
                continue;
            }
            Status::NotConnected => ("—".to_string(), fg, 110, false),
        };
        segs.push(seg(icon(*kind), true, color, opacity, s(5)));
        if charging {
            segs.push(seg(&label, false, color, opacity, s(2)));
            segs.push(seg(BOLT, true, green, opacity, gap));
        } else {
            segs.push(seg(&label, false, color, opacity, gap));
        }
    }

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

    // Layout.
    let pad = s(6);
    let mut x = pad;
    for seg in &mut segs {
        SelectObject(dc, if seg.icon { icon_font } else { text_font });
        let mut size: SIZE = zeroed();
        GetTextExtentPoint32W(dc, seg.text.as_ptr(), seg.text.len() as i32 - 1, &mut size);
        seg.x = x;
        seg.w = size.cx;
        seg.h = size.cy;
        x += size.cx + seg.gap_after;
    }
    let width = x + pad;

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
        return width;
    }
    let old_bmp = SelectObject(dc, bmp);
    SetBkMode(dc, TRANSPARENT as _);
    SetTextColor(dc, 0x00FF_FFFF);
    for seg in &segs {
        SelectObject(dc, if seg.icon { icon_font } else { text_font });
        TextOutW(dc, seg.x, (height - seg.h) / 2, seg.text.as_ptr(), seg.text.len() as i32 - 1);
    }
    GdiFlush();

    // Per-column color/opacity, then convert coverage -> premultiplied BGRA.
    let mut columns = vec![(fg, 255u32); width as usize];
    for seg in &segs {
        let end = (seg.x + seg.w + seg.gap_after).min(width);
        for c in seg.x.max(0)..end {
            columns[c as usize] = (seg.color, seg.opacity);
        }
    }
    let px = std::slice::from_raw_parts_mut(bits as *mut u32, (width * height) as usize);
    for (i, p) in px.iter_mut().enumerate() {
        let (color, opacity) = columns[i % width as usize];
        let cov = ((*p >> 16) & 0xFF).max((*p >> 8) & 0xFF).max(*p & 0xFF);
        let a = cov * opacity / 255;
        // Alpha 1 keeps the whole rect clickable while staying invisible.
        *p = if a == 0 {
            0x0100_0000
        } else {
            let ch = |shift: u32| ((color >> shift) & 0xFF) * a / 255;
            (a << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
        };
    }

    sink(dc, width, px);

    SelectObject(dc, old_bmp);
    DeleteObject(bmp);
    DeleteObject(text_font);
    DeleteObject(icon_font);
    DeleteDC(dc);
    width
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
