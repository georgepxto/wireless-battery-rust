//! Headsets HyperX sem fio (VID 03F0 da HP e 0951 da Kingston).
//! Portado de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0), hyperx.cpp; protocolos de
//! Medprj/HyperXWM, LennardKittner/HyperHeadset, davidsantanaalves/HyperBatteryMonitor (MIT)
//! e Sapd/HeadsetControl.

use super::{clamp100, hex4, interpolate, read_until, sleep_ms, Ctx, Status};
use crate::device::Kind;
use crate::hid::{DeviceInfo, HidDevice};

fn query(d: &HidDevice, req: &[u8], ms: u64, accept: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    d.write(req).ok()?;
    read_until(d, ms, |r| accept(r).then(|| r.to_vec()))
}

fn base(pid: u16, name: &str) -> Status {
    Status::new(format!("hyperx:{}", hex4(pid)), name, Kind::Headset)
}

/// Cloud III Wireless: 66 <cmd>; 82 -> [2] conectado, 8A -> [2] carregando, 89 -> [4] %.
fn cloud3(d: &HidDevice, pid: u16) -> Option<Status> {
    let q = |cmd: u8| query(d, &[0x66, cmd], 800, |r| r.len() >= 5 && r[0] == 0x66 && r[1] == cmd);
    let r = q(0x82)?;
    let mut s = base(pid, if pid == 0x069F || pid == 0x0995 { "HyperX Cloud II Core Wireless" } else { "HyperX Cloud III Wireless" });
    s.connected = r[2] != 0;
    if s.connected {
        if let Some(r) = q(0x8A) {
            s.charging = r[2] == 1;
        }
        if let Some(r) = q(0x89).filter(|r| r[4] <= 100) {
            s.percent = r[4] as i32;
        }
    }
    Some(s)
}

/// Cloud II Wireless (HP): 06 FF BB <cmd>; 02 -> [7] %, 03 -> [4] carregando.
fn cloud2(d: &HidDevice, pid: u16) -> Option<Status> {
    let q = |cmd: u8| query(d, &[0x06, 0xFF, 0xBB, cmd], 1000, |r| r.len() >= 8 && r[0] == 0x06 && r[1] == 0xFF && r[2] == 0xBB && r[3] == cmd);
    let r = q(0x02).filter(|r| r[7] <= 100)?;
    let mut s = base(pid, "HyperX Cloud II Wireless");
    s.percent = r[7] as i32;
    s.charging = q(0x03).is_some_and(|r| r[4] == 1);
    Some(s)
}

/// Cloud II Wireless (Kingston 0951:1718): frame fixo, resposta 0B 00 BB <cmd>.
fn cloud2_kingston(d: &HidDevice, pid: u16) -> Option<Status> {
    let q = |cmd: u8| {
        let req = [0x06, 0x00, 0x02, 0x00, 0x9A, 0x00, 0x00, 0x68, 0x4A, 0x8E, 0x0A, 0x00, 0x00, 0x00, 0xBB, cmd, 0x00];
        query(d, &req, 1000, |r| r.len() >= 8 && r[0] == 0x0B && r[2] == 0xBB && r[3] == cmd)
    };
    let r = q(0x02).filter(|r| r[7] <= 100)?;
    let mut s = base(pid, "HyperX Cloud II Wireless");
    s.percent = r[7] as i32;
    s.charging = q(0x03).is_some_and(|r| r[4] == 1);
    Some(s)
}

/// Cloud Alpha Wireless: 21 BB <cmd>; 03 -> [3]==1 offline, 0C -> carregando, 0B -> %.
fn cloud_alpha(d: &HidDevice, pid: u16) -> Option<Status> {
    let q = |cmd: u8| query(d, &[0x21, 0xBB, cmd], 1000, |r| r.len() >= 4 && r[0] == 0x21 && r[2] == cmd);
    let r = q(0x03)?;
    let mut s = base(pid, "HyperX Cloud Alpha Wireless");
    s.connected = r[3] != 0x01;
    if s.connected {
        s.charging = q(0x0C).is_some_and(|r| r[3] == 0x01);
        if let Some(r) = q(0x0B).filter(|r| r[3] <= 100) {
            s.percent = r[3] as i32;
        }
    }
    Some(s)
}

/// Cloud Alpha 2 Wireless: [cmd][sub] -> [cmd+1][sub]; 52 01 -> [2] conectado, 50 02 -> [2] %, [3] carregando.
fn cloud_alpha2(d: &HidDevice, pid: u16) -> Option<Status> {
    let q = |cmd: u8, sub: u8| query(d, &[cmd, sub], 1000, |r| r.len() >= 8 && r[0] == cmd + 1 && r[1] == sub);
    let r = q(0x52, 0x01)?;
    let mut s = base(pid, "HyperX Cloud Alpha 2 Wireless");
    s.connected = r[2] != 0;
    if s.connected {
        if let Some(r) = q(0x50, 0x02).filter(|r| r[2] <= 100) {
            s.percent = r[2] as i32;
            s.charging = r[3] == 0x01;
        }
    }
    Some(s)
}

/// Cloud Flight: 21 FF 05 -> [3..4] mV (acima de 0x100B = carregando).
fn cloud_flight(d: &HidDevice, pid: u16) -> Option<Status> {
    let r = query(d, &[0x21, 0xFF, 0x05], 1000, |r| r.len() >= 5)?;
    let mv = u16::from_be_bytes([r[3], r[4]]) as f64;
    let mut s = base(pid, "HyperX Cloud Flight");
    if mv > 0x100B as f64 {
        s.charging = true;
    } else if mv <= 3648.0 {
        s.percent = clamp100((0.00125 * mv) as i32);
    } else if mv > 3975.0 {
        s.percent = 100;
    } else {
        let p = 0.00000002547505 * mv.powi(4) - 0.0003900299 * mv.powi(3) + 2.238321 * mv * mv - 5706.256 * mv + 5452299.0;
        s.percent = clamp100(p.round() as i32);
    }
    Some(s)
}

/// Cloud III S Wireless: report 0C [02 03 01 00 <cmd>]; resposta [5] = cmd, [6] = valor.
/// 02 -> [6] == 2 conectado, 06 -> %, 48 -> 0 descarregando / 1 carregando / 2 cheio.
fn cloud3s(d: &HidDevice, pid: u16) -> Option<Status> {
    let q = |cmd: u8| query(d, &[0x0C, 0x02, 0x03, 0x01, 0x00, cmd], 1000, |r| r.len() >= 7 && r[0] == 0x0C && r[5] == cmd && r[6] != 0xFF);
    let r = q(0x02)?;
    let mut s = base(pid, "HyperX Cloud III S Wireless");
    s.connected = r[6] == 2;
    if s.connected {
        if let Some(r) = q(0x06).filter(|r| r[6] <= 100) {
            s.percent = r[6] as i32;
        }
        s.charging = q(0x48).is_some_and(|r| r[6] == 1 || r[6] == 2);
    }
    Some(s)
}

/// Cloud Stinger Core Wireless: feature report FF na colecao FF00:0001.
fn stinger_core(d: &HidDevice, pid: u16) -> Option<Status> {
    const CONNECTION: [u8; 20] = [0xFF, 0x07, 0x00, 0xFD, 0x04, 0x00, 0x00, 0x02, 0x82, 0x03, 0x05, 0x29, 0x0F, 0x00, 0x00, 0x03, 0x68, 0x10, 0x00, 0x00];
    const BATTERY: [u8; 12] = [0xFF, 0x07, 0x00, 0xFD, 0x04, 0x0C, 0xF1, 0x02, 0x01, 0x04, 0xF0, 0x0C];
    const PCT: [i32; 11] = [100, 90, 80, 70, 60, 50, 40, 30, 20, 10, 0];
    const CHARGING_MV: [i32; 11] = [4130, 4090, 4050, 4000, 3967, 3946, 3930, 3916, 3871, 3831, 3400];
    const DISCHARGING_MV: [i32; 11] = [4121, 4042, 3954, 3905, 3838, 3785, 3763, 3738, 3700, 3651, 3517];
    let ps = pid == 0x0A93;
    let exchange = |cmd: &[u8], good: fn(&[u8]) -> bool| -> (bool, Option<Vec<u8>>) {
        if d.send_feature_report(cmd).is_err() {
            return (false, None);
        }
        let mut last = None;
        for _ in 0..(if ps { 10 } else { 1 }) {
            sleep_ms(20);
            let Some(r) = d.get_feature(0xFF).filter(|r| r.len() >= 13) else { return (false, last) };
            let ok = !ps || good(&r);
            last = Some(r);
            if ok {
                return (true, last);
            }
        }
        (false, last)
    };
    let fresh: fn(&[u8]) -> bool = |r| !(r[1] == 0x01 && r[2] == 0x00);
    let battery: fn(&[u8]) -> bool = |r| r[0] == 0xFF && r[1] == 0x12 && r[2] == 0x05;
    let (ok, r) = exchange(&CONNECTION, fresh);
    let r = match (ok, r) {
        (true, Some(r)) => r,
        (false, Some(r)) if ps => r, // cabecalho nao documentado: depois de 10 leituras usa a ultima
        _ => return None,
    };
    let mut s = base(pid, "HyperX Cloud Stinger Core Wireless");
    s.connected = r[11] == 0x01;
    if !s.connected {
        return Some(s);
    }
    let (ok, r) = exchange(&BATTERY, battery);
    let Some(r) = r.filter(|_| ok) else { return Some(s) };
    let code = r[9];
    if !matches!(code, 0x03 | 0x05 | 0x06) {
        return Some(s);
    }
    let mv = u16::from_le_bytes([r[11], r[12]]) as i32;
    s.charging = code == 0x05;
    s.percent = if code == 0x06 { 100 } else { interpolate(mv, if code == 0x05 { &CHARGING_MV } else { &DISCHARGING_MV }, &PCT) };
    Some(s)
}

type Poll = fn(&HidDevice, u16) -> Option<Status>;
type Accept = fn(&DeviceInfo) -> bool;

const MODELS: &[(u16, &[u16], Accept, Poll, bool)] = &[
    (0x03F0, &[0x05B7, 0x0C9D], |c| c.output_len() == 62 && c.input_len() == 62, cloud3, false),
    (0x03F0, &[0x069F, 0x0995], |c| c.usage_page() >= 0xFF00 && c.output_len() >= 62 && c.input_len() >= 62, cloud3, false),
    (0x03F0, &[0x06BE, 0x02CC], |c| c.output_len() == 64 && c.input_len() == 64, cloud3s, false),
    (0x03F0, &[0x0696, 0x018B, 0x0D93], |c| c.usage_page() == 0xFF90 && c.usage() == 0x0303, cloud2, false),
    (0x0951, &[0x1718], |c| c.output_len() == 62, cloud2_kingston, false),
    (0x03F0, &[0x098D], |c| c.usage_page() >= 0xFF00 && c.output_len() >= 3, cloud_alpha, false),
    (0x03F0, &[0x08BE], |c| c.usage_page() == 0xFF13 && c.usage() == 0xFF00, cloud_alpha2, false),
    (0x0951, &[0x16C4, 0x1723, 0x1749], |c| c.usage_page() >= 0xFF00 && c.output_len() >= 3, cloud_flight, false),
    (0x03F0, &[0x0E90], |c| c.usage_page() >= 0xFF00 && c.output_len() >= 3, cloud_flight, false),
    (0x03F0, &[0x0A93], |c| c.usage_page() == 0xFF00 && c.usage() == 0x0001 && c.feature_len() >= 32, stinger_core, true),
    (0x0951, &[0x170B], |c| c.usage_page() == 0xFF00 && c.usage() == 0x0001 && c.feature_len() >= 32, stinger_core, true),
];

pub fn poll(ctx: &mut Ctx) {
    for &(vid, pids, accept, poll, feature) in MODELS {
        for &pid in pids {
            if ctx.is_claimed(vid, pid) {
                continue;
            }
            for c in ctx.find(vid, pid, accept) {
                let dev = if feature { ctx.open(&c).or_else(|| ctx.open_feature_only(&c)) } else { ctx.open(&c) };
                if let Some(s) = dev.and_then(|d| poll(&d, pid)) {
                    ctx.add(s, vid, pid);
                    break;
                }
            }
        }
    }
}
