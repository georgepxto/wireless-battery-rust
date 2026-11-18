//! Mouses, teclados e dongles Razer sem fio (VID 1532). Feature report de 90 bytes (id 0):
//! [0] status [1] transaction id [5] tamanho [6] classe [7] id [8..87] argumentos [88] crc (xor 2..87).
//! Bateria: classe 07 id 80 -> args[1] 0..255. Carregando: 07 84 -> args[1] = 1.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), razer.cpp; protocolo do OpenRazer.

use super::{clamp100, hex4, sleep_ms, Ctx, Status};
use crate::device::{Kind, State};
use crate::hid::HidDevice;

fn keyboard_tid(pid: u16) -> u8 {
    match pid {
        0x025C | 0x0271 | 0x0290 | 0x0296 | 0x02BA | 0x02D2 | 0x02D5 => 0x9F,
        0x0258 | 0x0292 | 0x0298 | 0x02B9 | 0x02CD | 0x02D4 | 0x02D7 => 0x1F,
        0x025A => 0x3F,
        _ => 0,
    }
}

#[derive(PartialEq)]
enum Rz {
    Ok(Vec<u8>),
    Offline,
    Unsupported,
    NoReply,
}

fn command(d: &HidDevice, pid: u16, tid: u8, cls: u8, id: u8, size: u8) -> Rz {
    let mut rep = [0u8; 91];
    {
        let r = &mut rep[1..];
        r[1] = tid;
        r[5] = size;
        r[6] = cls;
        r[7] = id;
        r[88] = r[2..88].iter().fold(0, |a, x| a ^ x);
    }
    let mut send = true;
    let mut res = Rz::NoReply;
    for attempt in 0..8 {
        if send && d.send_feature_report(&rep).is_err() {
            return Rz::NoReply;
        }
        // Teclados sem fio precisam de ~5 ms; esperar demais deixa o Synapse trocar a resposta.
        sleep_ms(if keyboard_tid(pid) != 0 { 5 } else if attempt == 0 { 15 } else { 40 });
        send = true;
        res = Rz::NoReply;
        let mut buf = [0u8; 91];
        let Ok(n) = d.get_feature_report(&mut buf) else { continue };
        if n < 91 {
            continue;
        }
        let a = &buf[1..];
        let crc = a[2..88].iter().fold(0, |x, y| x ^ y);
        if a[1] != tid || a[2] != 0 || a[3] != 0 || a[4] != 0 || a[5] != size || a[6] != cls || a[7] != id || a[88] != crc {
            continue; // o Synapse divide a caixa de feature reports
        }
        match a[0] {
            0x01 => send = false, // ocupado: espera este pedido terminar
            0x02 => return Rz::Ok(a[8..88].to_vec()),
            0x04 => res = Rz::Offline,
            0x03 | 0x05 => return Rz::Unsupported,
            _ => {}
        }
    }
    res
}

fn clean_name(n: &str) -> String {
    let mut n = n.to_string();
    for suffix in [" Dongle", " (Wired)", " (Wireless)", " Wireless Receiver", " Receiver", " HyperSpeed Dongle"] {
        if n.len() > suffix.len() && n.ends_with(suffix) {
            n.truncate(n.len() - suffix.len());
        }
    }
    if n.is_empty() {
        "Razer device".into()
    } else {
        n
    }
}

const KEYBOARD_PIDS: &[u16] = &[
    0x010D, 0x010E, 0x010F, 0x0111, 0x0113, 0x0118, 0x011A, 0x011B, 0x011C, 0x0201, 0x0202, 0x0203, 0x0204, 0x0205, 0x0207, 0x0208, 0x0209, 0x020F, 0x0210, 0x0211, 0x0214, 0x0216, 0x0217,
    0x021A, 0x021E, 0x021F, 0x0220, 0x0221, 0x0224, 0x0225, 0x0226, 0x0227, 0x0228, 0x022A, 0x022B, 0x022C, 0x022D, 0x022F, 0x0232, 0x0233, 0x0234, 0x0235, 0x0237, 0x0239, 0x023A, 0x023B,
    0x023F, 0x0240, 0x0241, 0x0243, 0x0244, 0x0245, 0x0246, 0x024A, 0x024B, 0x024C, 0x024D, 0x024E, 0x0252, 0x0253, 0x0255, 0x0256, 0x0257, 0x0258, 0x0259, 0x025A, 0x025C, 0x025D, 0x025E,
    0x0266, 0x0268, 0x0269, 0x026A, 0x026B, 0x026C, 0x026D, 0x026E, 0x026F, 0x0270, 0x0271, 0x0276, 0x0279, 0x027A, 0x0282, 0x0287, 0x028A, 0x028B, 0x028C, 0x028D, 0x028F, 0x0290, 0x0292,
    0x0293, 0x0294, 0x0295, 0x0296, 0x0298, 0x029D, 0x029E, 0x029F, 0x02A0, 0x02A1, 0x02A2, 0x02A3, 0x02A5, 0x02A6, 0x02A7, 0x02B0, 0x02B6, 0x02B8, 0x02B9, 0x02BA, 0x02C5, 0x02C6, 0x02C7,
    0x02CD, 0x02CF, 0x02D2, 0x02D4, 0x02D5, 0x02D7, 0x02E0, 0x02E1, 0x02EA, 0x0A24,
];

const MOUSE_PIDS: &[u16] = &[
    0x0013, 0x0015, 0x0016, 0x001F, 0x0020, 0x0024, 0x0025, 0x0029, 0x002E, 0x002F, 0x0032, 0x0034, 0x0036, 0x0037, 0x0038, 0x0039, 0x003E, 0x003F, 0x0040, 0x0041, 0x0042, 0x0043, 0x0044,
    0x0045, 0x0046, 0x0048, 0x004C, 0x004F, 0x0050, 0x0053, 0x0054, 0x0059, 0x005A, 0x005B, 0x005C, 0x005E, 0x005F, 0x0060, 0x0062, 0x0064, 0x0065, 0x0067, 0x006A, 0x006B, 0x006C, 0x006E,
    0x006F, 0x0070, 0x0071, 0x0072, 0x0073, 0x0077, 0x0078, 0x007A, 0x007B, 0x007C, 0x007D, 0x0080, 0x0083, 0x0084, 0x0085, 0x0086, 0x0088, 0x008A, 0x008C, 0x008D, 0x008F, 0x0090, 0x0091,
    0x0094, 0x0095, 0x0096, 0x0098, 0x0099, 0x009A, 0x009C, 0x009E, 0x009F, 0x00A1, 0x00A3, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00AA, 0x00AB, 0x00AF, 0x00B0, 0x00B2, 0x00B3, 0x00B4, 0x00B6,
    0x00B7, 0x00B8, 0x00B9, 0x00BE, 0x00BF, 0x00C0, 0x00C1, 0x00C2, 0x00C3, 0x00C4, 0x00C5, 0x00C7, 0x00C8, 0x00CB, 0x00CC, 0x00CD, 0x00D0, 0x00D1, 0x00D3, 0x00D4, 0x00D6, 0x00D7, 0x00DA,
    0x00DB, 0x00DE, 0x00DF, 0x00E0, 0x00E5, 0x00E6,
];

pub fn poll(ctx: &mut Ctx) {
    let mut seen: Vec<String> = Vec::new();
    for (vid, pid) in ctx.products(0x1532) {
        for c in ctx.find(vid, pid, |c| c.feature_len() == 91) {
            let Some(d) = ctx.open_feature_only(&c) else { continue };
            let kt = keyboard_tid(pid);
            let tids: &[u8] = if kt != 0 { &[kt] } else { &[0x1F, 0x3F, 0xFF, 0x08, 0x9F] };
            let mut found = None;
            let mut offline = false;
            for &t in tids {
                match command(&d, pid, t, 0x07, 0x80, 0x02) {
                    Rz::Ok(args) => {
                        found = Some((t, args));
                        break;
                    }
                    Rz::Offline => offline = true, // pode ser "transaction id errado": continua
                    _ => {}
                }
            }
            if found.is_none() && !offline {
                continue;
            }
            let name = clean_name(c.product_string().unwrap_or(""));
            let kind = if KEYBOARD_PIDS.contains(&pid) {
                Kind::Keyboard
            } else if MOUSE_PIDS.contains(&pid) {
                Kind::Mouse
            } else {
                match ctx.kind_from_collections(vid, pid) {
                    Kind::Other => Kind::from_name(&name),
                    k => k,
                }
            };
            let mut s = Status::new(format!("razer:{}", hex4(pid)), name.clone(), kind);
            match found {
                None => s.connected = false,
                Some((tid, args)) => {
                    s.percent = clamp100((args[1] as i32 * 100 + 127) / 255);
                    if let Rz::Ok(ch) = command(&d, pid, tid, 0x07, 0x84, 0x02) {
                        s.charging = ch[1] == 1;
                    }
                }
            }
            // Um mouse carregando aparece duas vezes (cabo + dongle): fica um so.
            if !seen.contains(&name) {
                seen.push(name);
                ctx.add(s, vid, pid);
            } else {
                ctx.claim(vid, pid);
                if s.charging {
                    for o in ctx.out.iter_mut().filter(|o| o.name.as_deref() == Some(name.as_str())) {
                        if let State::Battery { charging, .. } = &mut o.state {
                            *charging = true;
                        }
                    }
                }
            }
            break;
        }
    }
}
