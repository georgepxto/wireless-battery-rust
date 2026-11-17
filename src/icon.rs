//! Gera o icone da bandeja com o numero da bateria mais baixa, via GDI.

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
    DrawTextW, GdiFlush, GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_BOLD,
    OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON,
};

use crate::wide;

/// `rgb` em 0xRRGGBB.
pub fn render(text: &str, rgb: u32) -> HICON {
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size;
        bmi.bmiHeader.biHeight = -size; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB as _;

        let mut bits: *mut core::ffi::c_void = null_mut();
        let color = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let old_bmp = SelectObject(dc, color);

        // Altura da fonte: 3 digitos ("100") precisam ser menores.
        let chars = text.chars().count() as i32;
        let height = match chars {
            0..=2 => size * 92 / 100,
            _ => size * 64 / 100,
        };
        let face = wide("Segoe UI");
        let font = CreateFontW(
            -height, 0, 0, 0, FW_BOLD as _, 0, 0, 0,
            DEFAULT_CHARSET as _, OUT_DEFAULT_PRECIS as _, CLIP_DEFAULT_PRECIS as _,
            ANTIALIASED_QUALITY as _, 0, face.as_ptr(),
        );
        let old_font = SelectObject(dc, font);

        // Desenha branco sobre preto e usa a luminancia como canal alfa.
        SetBkMode(dc, TRANSPARENT as _);
        SetTextColor(dc, 0x00FF_FFFF);
        let mut rect = RECT { left: -2, top: 0, right: size + 2, bottom: size };
        let mut t: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(dc, t.as_mut_ptr(), t.len() as i32, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        GdiFlush();

        let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        for p in px.iter_mut() {
            let a = *p & 0xFF;
            *p = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
        }

        SelectObject(dc, old_font);
        SelectObject(dc, old_bmp);
        DeleteObject(font);

        let mask = CreateBitmap(size, size, 1, 1, null());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);

        DeleteObject(mask);
        DeleteObject(color);
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
        icon
    }
}
