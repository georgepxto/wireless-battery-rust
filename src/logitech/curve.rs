//! Voltage -> percentage conversion.
//!
//! When G HUB is installed, its per-device power models are used so our % matches G HUB:
//! C:\ProgramData\LGHUB\depots\<n>\<device>\*battery*.xml contain `<powermodel pid="0x....">`
//! plus a `<discharge>` CDATA table of `time,volts,used_energy` rows.
//! Percent = 100 * (1 - used_at_voltage / total_used).
//! Otherwise a generic Li-ion curve is used.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

struct Curve {
    /// (millivolts, used energy), voltage descending.
    rows: Vec<(u16, f32)>,
    total: f32,
}

impl Curve {
    fn percent(&self, mv: u16) -> u8 {
        if self.rows.first().is_some_and(|(v, _)| mv >= *v) {
            return 100;
        }
        let used = self.rows.iter().find(|(v, _)| *v <= mv).map_or(self.total, |(_, e)| *e);
        (100.0 * (1.0 - used / self.total)).round().clamp(0.0, 100.0) as u8
    }
}

#[derive(Default)]
pub struct Curves {
    by_pid: HashMap<u16, Curve>,
}

impl Curves {
    pub fn load() -> Self {
        let mut files = Vec::new();
        collect_battery_xml(Path::new(r"C:\ProgramData\LGHUB\depots"), 0, &mut files);
        let by_pid = files
            .iter()
            .filter_map(|f| {
                let text = fs::read_to_string(f).ok()?;
                Some((parse_pid(&text)?, parse_curve(&text)?))
            })
            .collect();
        Self { by_pid }
    }

    /// Uses the first G HUB curve found for any of `pids`, else the generic curve.
    pub fn percent(&self, pids: &[u16], mv: u16) -> u8 {
        pids.iter()
            .find_map(|p| self.by_pid.get(p))
            .map_or_else(|| pids.iter().find_map(|p| headset_percent(*p, mv)).unwrap_or_else(|| generic_percent(mv)), |c| c.percent(mv))
    }
}

fn collect_battery_xml(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() && depth < 3 {
            collect_battery_xml(&path, depth + 1, out);
        } else if path.extension().is_some_and(|x| x == "xml")
            && path.file_name().is_some_and(|n| n.to_string_lossy().contains("battery"))
        {
            out.push(path);
        }
    }
}

fn parse_pid(text: &str) -> Option<u16> {
    let model = &text[text.find("<powermodel")?..];
    let rest = &model[model.find("pid=\"0x")? + 7..];
    u16::from_str_radix(&rest[..rest.find('"')?], 16).ok()
}

fn parse_curve(text: &str) -> Option<Curve> {
    let body = &text[text.find("<discharge")?..];
    let body = &body[body.find("<![CDATA[")? + 9..];
    let body = &body[..body.find("]]>")?];

    let rows: Vec<(u16, f32)> = body
        .lines()
        .filter_map(|l| {
            let mut cols = l.trim().split(',');
            let _time = cols.next()?;
            let volts: f32 = cols.next()?.trim().parse().ok()?;
            let used: f32 = cols.next()?.trim().parse().ok()?;
            Some(((volts * 1000.0).round() as u16, used))
        })
        .collect();

    let total = rows.last()?.1;
    (total > 0.0).then_some(Curve { rows, total })
}

/// Generic Li-ion discharge curve (Solaar's approximation), linearly interpolated.
/// Curvas embutidas de headsets que so informam a tensao, para quando o G HUB nao esta instalado.
/// PRO X Wireless (0AAA / 0ABA): a tensao a cada 5% no modelo de energia do proprio G HUB
/// (pro_x_wireless_headset_battery.xml), conferida contra o G HUB; a tabela do HeadsetControl lia
/// 15 a 27 pontos abaixo. Os outros pela calibracao do HeadsetControl.
/// (krisgfx9/Wireless-Device-Battery-Monitor, GPL-3.0, logitech.cpp, be819ea)
fn headset_percent(pid: u16, mv: u16) -> Option<u8> {
    const PROX_V: &[i32] = &[4102, 4046, 3993, 3950, 3915, 3877, 3838, 3809, 3789, 3772, 3758, 3747, 3736, 3722, 3707, 3687, 3661, 3644, 3548, 3426, 3293];
    const PROX_P: &[i32] = &[100, 95, 90, 85, 80, 75, 70, 65, 60, 55, 50, 45, 40, 35, 30, 25, 20, 15, 10, 5, 0];
    const GPRO_V: &[i32] = &[4150, 3830, 3780, 3740, 3670, 3320];
    const GPRO_P: &[i32] = &[100, 50, 30, 20, 5, 0];
    const G533_V: &[i32] = &[4200, 3850, 3790, 3750, 3680, 3330];
    const G533_P: &[i32] = &[100, 50, 30, 20, 5, 0];
    const G535_V: &[i32] = &[4175, 3817, 3766, 3730, 3664, 3310];
    const G535_P: &[i32] = &[100, 50, 30, 20, 5, 0];
    const G633_V: &[i32] = &[4100, 3950, 3850, 3750, 3650, 3500, 3300, 3150];
    const G633_P: &[i32] = &[100, 80, 60, 40, 20, 10, 5, 0];
    let (v, p) = match pid {
        0x0AAA | 0x0ABA => (PROX_V, PROX_P),
        0x0AA7 | 0x0AFB => (GPRO_V, GPRO_P),
        0x0A66 => (G533_V, G533_P),
        0x0AC4 => (G535_V, G535_P),
        0x0A5C | 0x0A89 | 0x0A5B | 0x0A87 | 0x0AB5 | 0x0AFE => (G633_V, G633_P),
        _ => return None,
    };
    Some(crate::drivers::interpolate(mv as i32, v, p) as u8)
}

fn generic_percent(mv: u16) -> u8 {
    const CURVE: [(u16, u8); 13] = [
        (4186, 100), (4067, 90), (3989, 80), (3922, 70), (3859, 60), (3811, 50), (3778, 40),
        (3751, 30), (3717, 20), (3671, 10), (3646, 5), (3579, 2), (3500, 0),
    ];
    if mv >= CURVE[0].0 {
        return 100;
    }
    for w in CURVE.windows(2) {
        let ((hi_mv, hi_p), (lo_mv, lo_p)) = (w[0], w[1]);
        if mv >= lo_mv {
            let t = (mv - lo_mv) as f32 / (hi_mv - lo_mv) as f32;
            return (lo_p as f32 + t * (hi_p - lo_p) as f32).round() as u8;
        }
    }
    0
}
