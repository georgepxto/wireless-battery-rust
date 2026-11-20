//! Marcas menores de mouse, cada uma com protocolo documentado pela comunidade:
//! Motospeed X6, receptores Xenta (Delux M900 Pro, Attack Shark X6 / R1 / X11 / X3 Max / V3 Pro),
//! Ajazz AJ179, AJAZZ AJ-series (3151), AJAZZ AJ139 V2, Wraith W1, Incott G23 V2 / G24 e Delux M800 Pro.
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), misc_mice.cpp, onde estao as
//! fontes de cada protocolo.

use super::{clamp100, hex4, read_until, sleep_ms, Ctx, Deadline, Status};
use crate::device::Kind;
use crate::hid::HidDevice;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

fn poll_motospeed(ctx: &mut Ctx) {
    for pid in [0xFFE0u16, 0xFFF2, 0xFFF1] {
        if ctx.is_claimed(0x0BDA, pid) {
            continue;
        }
        for c in ctx.find(0x0BDA, pid, |c| c.interface_number() == 4 && c.output_len() >= 21) {
            let mut s = Status::new(format!("motospeed:{}", hex4(pid)), "Motospeed X6", Kind::Mouse);
            if pid == 0xFFF1 {
                s.charging = true; // no cabo: carregando, sem leitura
                ctx.add(s, 0x0BDA, pid);
                break;
            }
            let Some(d) = ctx.open(&c) else { continue };
            d.flush();
            if d.write(&[0xB5, 0x06]).is_err() {
                continue;
            }
            let Some(raw) = read_until(&d, 1500, |r| (r.len() > 20 && r[0] == 0xB4).then(|| r[20] as i32)) else { continue };
            s.charging = raw >= 128;
            s.percent = clamp100(if s.charging { raw - 128 } else { raw });
            ctx.add(s, 0x0BDA, pid);
            break;
        }
    }
}

/// Receptor Xenta: transmite [03][modelo][40][estado][nivel] a cada ~2 s; o handle fica aberto
/// entre as leituras para os frames entrarem na fila. Uma leitura so vale quando dois frames
/// concordam (todo link comeca com um falso 100%).
struct Xenta {
    dev: HidDevice,
    last: Vec<u8>,
    prev: Vec<u8>,
    last_at: Instant,
}

static XENTA: Mutex<Option<HashMap<String, Xenta>>> = Mutex::new(None);

const XENTA_MODELS: &[(u8, &str, bool)] = &[
    (0x73, "Inphic IN10R MAX", false),
    (0x74, "Inphic IN10 MAX", false),
    (0x3D, "Delux M900 Pro", false),
    (0x30, "Delux M900 Pro", false),
    (0x50, "Delux M900 Pro", false),
    (0x24, "Attack Shark V6", false),
    (0x55, "Attack Shark X11", false),
    (0x4E, "Attack Shark X3 Max", false),
    (0xE5, "Attack Shark V3 Pro", false),
    (0x10, "Attack Shark X6 / R1", true), // nivel em dezenas (0-10)
];

fn poll_xenta(ctx: &mut Ctx) {
    let mut guard = XENTA.lock().unwrap();
    let listeners = guard.get_or_insert_with(HashMap::new);
    let mut present = Vec::new();
    for pid in [0xFA60u16, 0xFA61, 0xFA65, 0xFA55, 0x2155] {
        if ctx.is_claimed(0x1D57, pid) {
            continue;
        }
        for c in ctx.find(0x1D57, pid, |c| c.usage_page() == 0x000A && c.input_len() >= 5) {
            let path = c.path().to_string_lossy().into_owned();
            present.push(path.clone());
            let fresh = !listeners.contains_key(&path);
            if fresh {
                let Some(dev) = ctx.open_read_only(&c) else { continue };
                listeners.insert(path.clone(), Xenta { dev, last: Vec::new(), prev: Vec::new(), last_at: Instant::now() });
            }
            let l = listeners.get_mut(&path).unwrap();
            // Pega tudo o que chegou desde a ultima leitura; um ouvinte novo espera dois frames.
            let until = Deadline::in_ms(if fresh { 5000 } else { 0 });
            let mut got = 0;
            loop {
                let wait = if fresh && got < 2 {
                    if until.expired() {
                        break;
                    }
                    until.left().max(1)
                } else {
                    30
                };
                let Some(r) = l.dev.read(wait).filter(|r| r.len() >= 5) else { break };
                // Byte 2 e 40 ou 41 (Attack Shark V6, Delux M900 Pro modelo 50, Inphic).
                if r[0] != 0x03 || (r[2] != 0x40 && r[2] != 0x41) {
                    continue;
                }
                l.prev = std::mem::replace(&mut l.last, r[..5].to_vec());
                l.last_at = Instant::now();
                got += 1;
            }
            if l.last.is_empty() || l.last_at.elapsed().as_secs() > 15 {
                break; // mouse desligado: nenhum frame
            }
            let cable = pid == 0xFA55 || pid == 0x2155;
            let (code, st, level) = (l.last[1], l.last[3], l.last[4]);
            let model = XENTA_MODELS.iter().find(|m| m.0 == code);
            let mut s = Status::new(format!("xenta:{}", hex4(pid)), model.map_or("Wireless mouse", |m| m.1), Kind::Mouse);
            let agreed = l.prev == l.last;
            let delux = pid == 0xFA65 && (code == 0x30 || code == 0x50);
            if delux {
                if st == 0x02 {
                    s.percent = 100;
                } else if st == 0x03 {
                    s.charging = true;
                    if level <= 100 {
                        s.percent = level as i32;
                    }
                } else if agreed && st == 0x01 && (1..=100).contains(&level) {
                    s.percent = level as i32;
                }
            } else if cable || st == 0x02 || st == 0x03 {
                s.charging = true; // o nivel congela na energia externa
                if st == 0x02 && !cable {
                    s.percent = 100;
                }
            } else if agreed {
                if model.is_some_and(|m| m.2) {
                    if (1..=10).contains(&level) {
                        s.percent = level as i32 * 10;
                    }
                } else if st == 0x01 && (1..=100).contains(&level) {
                    s.percent = level as i32;
                }
            }
            if s.percent < 0 && !s.charging {
                s.asleep = true;
            }
            ctx.add(s, 0x1D57, pid);
            break;
        }
    }
    listeners.retain(|k, _| present.contains(k)); // esquece receptores desplugados
}

fn poll_ajazz(ctx: &mut Ctx) {
    for vendor in [0x249Au16, 0x248A] {
        for (vid, pid) in ctx.products(vendor) {
            for c in ctx.find(vid, pid, |c| c.interface_number() == 2 && c.output_len() == 33 && c.input_len() == 33) {
                let Some(d) = ctx.open(&c) else { continue };
                d.flush();
                if d.write(&[0x00, 0x10]).is_err() {
                    continue;
                }
                let Some(r) = read_until(&d, 1500, |r| {
                    (r.len() >= 32 && r[0] == 0x10 && r[1] == 0x00 && r[4..=30].iter().fold(0u8, |a, x| a.wrapping_add(*x)) == r[31]).then(|| r.to_vec())
                }) else {
                    continue;
                };
                if r[13] > 100 {
                    continue;
                }
                let model: String = r[4..8].iter().take_while(|b| (0x20..0x7F).contains(*b)).map(|b| *b as char).collect();
                let name = if model.len() == 4 && model.starts_with('M') { format!("Ajazz AJ{}", &model[1..]) } else { "Ajazz mouse".into() };
                let receiver = vid == 0x249A;
                let mut s = Status::new(format!("ajazz:{}:{}", hex4(vid), hex4(pid)), name, Kind::Mouse);
                s.connected = !receiver || r[14] != 0;
                s.charging = r[12] != 0;
                if s.connected {
                    s.percent = r[13] as i32;
                }
                ctx.add(s, vid, pid);
                break;
            }
        }
    }
}

fn poll_ajazz_aj_series(ctx: &mut Ctx) {
    for (vid, pid) in ctx.products(0x3151) {
        for c in ctx.find(vid, pid, |c| c.usage_page() == 0xFFFF && c.usage() == 0x02 && c.feature_len() >= 65) {
            let Some(d) = ctx.open_feature_only(&c) else { continue };
            let mut req = vec![0u8; c.feature_len() as usize];
            req[1] = 0xF7; // consulta de estado: liga o link de telemetria 2.4 GHz
            if d.send_feature_report(&req).is_err() {
                continue;
            }
            sleep_ms(30);
            let Some(r) = d.get_feature(0x05).filter(|r| r.len() >= 4 && r[0] == 0x05 && r[1] == 0 && r[2] == 0 && r[3] <= 100) else { continue };
            let name = c.product_string().unwrap_or("AJAZZ mouse").to_string();
            let mut s = Status::new(format!("ajazzaj:{}:{}", hex4(vid), hex4(pid)), name, Kind::Mouse);
            s.connected = r[3] > 0;
            if s.connected {
                s.percent = r[3] as i32;
            }
            ctx.add(s, vid, pid);
            break;
        }
    }
}

fn poll_ajazz_aj139(ctx: &mut Ctx) {
    let pid = 0x2255;
    for vendor in [0xA8A5u16, 0xA8A4] {
        if ctx.is_claimed(vendor, pid) {
            continue;
        }
        for c in ctx.find(vendor, pid, |c| c.usage_page() == 0xFF01 && c.usage() == 0x0010 && c.output_len() >= 65 && c.input_len() >= 65) {
            let Some(d) = ctx.open(&c) else { continue };
            let battery = |r: &[u8]| (r.len() > 9 && r[0] == 0xAA && r[1] == 0x30).then(|| (r[8], r[9]));
            d.flush();
            let mut got = None;
            if d.write(&[0x00, 0x55, 0x30, 0xA5, 0x2E, 0x2E]).is_ok() {
                got = read_until(&d, 700, battery);
            }
            if got.is_none() && d.write(&[0x00, 0x55, 0x30, 0xA5, 0x0B, 0x2E, 0x01, 0x01]).is_ok() {
                got = read_until(&d, 800, battery);
            }
            let Some((pct, power)) = got.filter(|g| g.0 <= 100) else { continue };
            let cable = vendor == 0xA8A4;
            let mut s = Status::new(format!("ajazz139:{}:{}", hex4(vendor), hex4(pid)), "AJAZZ wireless mouse", Kind::Mouse);
            s.charging = power != 0;
            s.connected = cable || pct > 0;
            if s.connected {
                s.percent = pct as i32;
            }
            ctx.add(s, vendor, pid);
            break;
        }
    }
}

/// Ultimo codigo de modelo W1 visto (522C, 622C); 0 = ainda nao e um W1.
static W1_MODEL: Mutex<[u8; 2]> = Mutex::new([0, 0]);

fn poll_wraith_w1(ctx: &mut Ctx) {
    for pid in [0x522Cu16, 0x622C] {
        if ctx.is_claimed(0x093A, pid) {
            continue;
        }
        let cable = pid == 0x622C;
        let mut colls = ctx.find(0x093A, pid, |c| c.usage_page() == 0xFF05 && c.feature_len() >= 9);
        if colls.is_empty() {
            continue;
        }
        colls.reverse(); // no cabo a primeira colecao FF05 recusa SET_FEATURE
        let mut dev = None;
        let mut replied = 0i32; // -1 = respondeu mas nao e W1
        for c in &colls {
            let Some(d) = ctx.open_feature_only(c) else { continue };
            if d.send_feature_report(&[0x09, 0x8F]).is_err() {
                continue;
            }
            for _ in 0..10 {
                sleep_ms(50);
                let Some(r) = d.get_feature(0x09) else { break };
                if r.len() >= 4 && r[0] == 0x09 && r[1] == 0x8F {
                    replied = if r[3] == 18 || r[3] == 20 { r[3] as i32 } else { -1 };
                    break;
                }
            }
            if replied != 0 {
                dev = Some(d);
                break;
            }
        }
        let mut known = W1_MODEL.lock().unwrap();
        let model = &mut known[cable as usize];
        if replied < 0 {
            *model = 0;
            continue; // outro mouse PixArt (Incott)
        }
        if replied > 0 {
            *model = replied as u8;
        }
        if *model == 0 {
            continue;
        }
        let name = if *model == 20 { "Wraith W1 Freestyle" } else { "Wraith W1" };
        drop(known);
        let mut s = Status::new(format!("wraithw1:{}", hex4(pid)), name, Kind::Mouse);
        s.connected = false;
        if let Some(d) = dev {
            let ins: Vec<HidDevice> = ctx
                .find(0x093A, pid, |c| c.usage_page() == 0xFF05 && c.input_len() >= 2)
                .iter()
                .filter_map(|c| ctx.open_read_only(c))
                .inspect(|i| i.flush())
                .collect();
            let read_button = |button: u8| -> bool {
                if d.send_feature_report(&[0x09, 0x86, button]).is_err() {
                    return false;
                }
                for t in 0..2 {
                    sleep_ms(if t == 0 { 150 } else { 20 });
                    match d.get_feature(0x09) {
                        None => return true,
                        Some(f) if f.len() >= 4 && f[1] == 0x86 => break,
                        _ => {}
                    }
                }
                true
            };
            let listen = |ms: u64| -> Option<u8> {
                let end = Deadline::in_ms(ms);
                while !end.expired() {
                    for i in &ins {
                        if let Some(r) = i.read(50).filter(|r| r.len() >= 2 && r[0] == 0x09 && r[1] & 0x7F <= 100) {
                            return Some(r[1]);
                        }
                    }
                }
                None
            };
            let mut got = None;
            if !ins.is_empty() {
                sleep_ms(200);
                let mut sent = false;
                for b in [0u8, 1, 2, 4, 3] {
                    sent |= read_button(b);
                }
                if sent {
                    got = listen(400);
                    for _ in 0..4 {
                        if got.is_some() || !read_button(0) {
                            break;
                        }
                        got = listen(550);
                    }
                }
            }
            if let Some(b) = got {
                let pct = (b & 0x7F) as i32;
                s.charging = b & 0x80 != 0;
                s.connected = cable || pct > 0 || s.charging;
                if s.connected {
                    s.percent = pct;
                }
            }
        }
        // Reivindicado mesmo dormindo, para o leitor Incott nunca consultar um W1.
        ctx.add(s, 0x093A, pid);
    }
}

fn poll_incott(ctx: &mut Ctx) {
    for pid in [0x522Cu16, 0x622C] {
        if ctx.is_claimed(0x093A, pid) {
            continue;
        }
        let mut colls = ctx.find(0x093A, pid, |c| c.usage_page() == 0xFF05 && c.feature_len() >= 9);
        colls.reverse();
        for c in &colls {
            let Some(d) = ctx.open_feature_only(c) else { continue };
            if d.send_feature_report(&[0x09, 0x89]).is_err() {
                continue;
            }
            let mut reply = None;
            for _ in 0..10 {
                sleep_ms(50);
                let Some(r) = d.get_feature(0x09) else { break };
                if r.len() >= 4 && r[0] == 0x09 && r[1] == 0x89 && (r[2] | r[3]) != 0 {
                    reply = Some(r);
                    break;
                }
            }
            let Some(r) = reply else { continue };
            let pct = (r[2] & 0x7F) as i32;
            if pct > 100 {
                continue;
            }
            let cable = pid == 0x622C;
            let mut s = Status::new(format!("incott:{}", hex4(pid)), "Incott mouse", Kind::Mouse);
            s.charging = r[2] & 0x80 != 0;
            s.connected = cable || pct > 0;
            if s.connected {
                s.percent = pct;
            }
            ctx.add(s, 0x093A, pid);
            break;
        }
    }
}

static DELUX_SEQ: AtomicU8 = AtomicU8::new(0);

fn poll_delux_m800(ctx: &mut Ctx) {
    for pid in [0x5B2Eu16, 0x5B2F] {
        if ctx.is_claimed(0x248A, pid) {
            continue;
        }
        for c in ctx.find(0x248A, pid, |c| c.usage_page() == 0xFF00 && c.usage() == 0x0001 && c.feature_len() >= 33) {
            let Some(d) = ctx.open_feature_only(&c) else { continue };
            let receiver = pid == 0x5B2F;
            let mut req = vec![0u8; c.feature_len() as usize];
            req[0] = 0x0C;
            req[1] = 0x01;
            req[2] = 0x20;
            let ask = |req: &mut Vec<u8>| -> Option<Vec<u8>> {
                req[4] = DELUX_SEQ.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
                d.send_feature_report(req).ok()?;
                sleep_ms(20);
                d.get_feature(0x0C).filter(|r| r.len() >= 20 && r[0] == 0x0C && r[2] == 0x20)
            };
            let Some(mut r) = ask(&mut req) else { continue };
            if receiver && r[6] == 0 {
                if let Some(r2) = ask(&mut req) {
                    r = r2;
                }
            }
            let pct = r[18] as i32;
            if pct > 100 {
                continue;
            }
            let st = r[19];
            let mut s = Status::new("delux:m800", "Delux M800 Pro", Kind::Mouse);
            s.connected = r[6] != 0 || pct > 0;
            if st == 2 {
                s.percent = 100;
                s.charging = true;
            } else {
                s.charging = st == 1;
                if s.connected && pct > 0 {
                    s.percent = clamp100(pct);
                }
            }
            if !receiver && s.connected {
                ctx.claim(0x248A, 0x5B2F);
            }
            ctx.add(s, 0x248A, pid);
            break;
        }
    }
}

pub fn poll(ctx: &mut Ctx) {
    poll_motospeed(ctx);
    poll_xenta(ctx);
    poll_delux_m800(ctx);
    poll_ajazz(ctx);
    poll_ajazz_aj_series(ctx);
    poll_ajazz_aj139(ctx);
    poll_wraith_w1(ctx); // antes do Incott: mesmos IDs, comandos diferentes
    poll_incott(ctx);
}
