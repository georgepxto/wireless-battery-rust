//! Logitech G435 LIGHTSPEED no dongle novo 046D:0ADF (nao fala HID++). Colecao FF00:0001, frames
//! de 64 bytes com CRC-16/XMODEM; comandos "PA" chegam ao radio do headset. A bateria vem do
//! GET_BATTERY do OTA em mV. Uma leitura nao interrompe audio nem microfone.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), logitech_g435_0adf.cpp/.h;
//! protocolo de portyporty/g435-battery-percentage (MIT).

use super::{hex4, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;

const VID: u16 = 0x046D;
const PID: u16 = 0x0ADF;

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// <cmd> <tamanho> <crc lo> <crc hi> <corpo...>, completado com zeros; o CRC cobre o frame todo
/// com o campo zerado.
pub fn frame(cmd: u8, body: &[u8]) -> [u8; 64] {
    let mut f = [0u8; 64];
    f[0] = cmd;
    f[1] = body.len() as u8;
    for (i, b) in body.iter().take(60).enumerate() {
        f[4 + i] = *b;
    }
    let crc = crc16(&f);
    f[2..4].copy_from_slice(&crc.to_le_bytes());
    f
}

pub fn parse_millivolts(d: &[u8]) -> Option<i32> {
    d.windows(8).find_map(|w| {
        (w[0] == 0 && w[1] == 7 && w[2] == 0 && w[3] == 6 && w[5] == 0)
            .then(|| u16::from_le_bytes([w[6], w[7]]) as i32)
            .filter(|mv| (2500..=4500).contains(mv))
    })
}

pub fn percent(mv: i32) -> i32 {
    const V: [i32; 16] = [4200, 4150, 4100, 4050, 4000, 3950, 3900, 3850, 3800, 3750, 3700, 3650, 3600, 3550, 3500, 3400];
    const P: [i32; 16] = [100, 95, 90, 85, 80, 75, 70, 62, 55, 45, 35, 25, 15, 10, 5, 0];
    if mv >= V[0] {
        return 100;
    }
    for i in 1..16 {
        if mv >= V[i] {
            return P[i] + ((P[i - 1] - P[i]) * (mv - V[i]) + (V[i - 1] - V[i]) / 2) / (V[i - 1] - V[i]);
        }
    }
    0
}

fn transact(d: &HidDevice, cmd: u8, body: &[u8], wait: u64, want_mv: bool) -> Option<Option<i32>> {
    let mut out = vec![0u8];
    out.extend_from_slice(&frame(cmd, body));
    d.flush();
    d.write(&out).ok()?;
    let end = Deadline::in_ms(wait);
    while !end.expired() {
        if let Some(r) = d.read(end.left()) {
            if want_mv {
                if let Some(mv) = parse_millivolts(&r) {
                    return Some(Some(mv));
                }
            }
        }
    }
    Some(None)
}

fn read_millivolts(d: &HidDevice) -> Option<i32> {
    const SESSION_INIT: [u8; 19] = [0x50, 0x41, 0x06, 0x01, 0x0D, 0x00, 0x25, 0x34, 0x12, 0x5A, 0x5A, 0x01, 0x00, 0x00, 0x00, 0xF0, 0x00, 0x00, 0x00];
    transact(d, 0x84, &[0x01], 50, false)?;
    struct Close<'a>(&'a HidDevice);
    impl Drop for Close<'_> {
        fn drop(&mut self) {
            let _ = transact(self.0, 0x85, &[0x02], 50, false);
        }
    }
    let _guard = Close(d);
    transact(d, 0x85, &[0x01], 50, false);
    transact(d, 0x80, &SESSION_INIT, 450, false);
    let mut seq = 2u8;
    let mut pa = |cat: u8, len: u8, payload: &[u8]| {
        let mut v = vec![0x50, 0x41, cat, seq, len];
        v.extend_from_slice(payload);
        seq = seq % 127 + 1;
        v
    };
    transact(d, 0x80, &pa(0x0E, 1, &[0xE0]), 120, false);
    transact(d, 0x80, &pa(0x0E, 2, &[0xE1, 0x01]), 120, false);
    transact(d, 0x80, &pa(0x0E, 1, &[0xE0]), 120, false);
    transact(d, 0x80, &pa(0x06, 1, &[0x00, 0x31]), 250, true).flatten()
}

pub fn poll(ctx: &mut Ctx) {
    if ctx.is_claimed(VID, PID) {
        return;
    }
    let colls = ctx.find(VID, PID, |c| c.usage_page() == 0xFF00 && c.usage() == 0x0001 && c.input_len() == 65 && c.output_len() == 65);
    let Some(d) = colls.first().and_then(|c| ctx.open(c)) else { return };
    let mut s = Status::new(format!("g435:{}", hex4(PID)), "Logitech G435", Kind::Headset);
    match read_millivolts(&d) {
        Some(mv) => s.percent = percent(mv),
        None => s.connected = false, // dongle presente, headset desligado ou fora de alcance
    }
    ctx.add(s, VID, PID);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_and_curve() {
        assert_eq!(crc16(b"123456789"), 0x31C3);
        let f = frame(0x84, &[0x01]);
        let mut z = f;
        z[2] = 0;
        z[3] = 0;
        assert_eq!(u16::from_le_bytes([f[2], f[3]]), crc16(&z));
        assert_eq!(parse_millivolts(&[9, 0, 7, 0, 6, 3, 0, 0xA0, 0x0F]), Some(4000));
        assert_eq!(percent(4200), 100);
        assert_eq!(percent(4000), 80);
        assert_eq!(percent(3300), 0);
    }
}
