//! Xbox and other XInput-compatible controllers, through Windows.Gaming.Input.
//! The battery report gives real capacity (mWh), so the percentage is exact.
//!
//! The controller list fills in shortly after the API is first used, so a controller
//! shows up from the second scan on.

use crate::device::{Kind, Observation, State};
use windows::Gaming::Input::{Gamepad, RawGameController};
use windows::System::Power::BatteryStatus;

/// Dongles e cabos que se apresentam como controle Xbox mas informam uma bateria fixa
/// (sempre cheia), sem relacao com o nivel real. Machenike G5 Pro / G2 Pro: 2345:2104.
const FAKE_BATTERY_VENDORS: &[u16] = &[0x2345];

pub fn scan() -> Vec<Observation> {
    scan_gamepads().unwrap_or_else(|e| {
        if cfg!(debug_assertions) {
            eprintln!("[xbox] {e}");
        }
        Vec::new()
    })
}

fn scan_gamepads() -> windows::core::Result<Vec<Observation>> {
    let pads = Gamepad::Gamepads()?;
    let mut out = Vec::new();
    for i in 0..pads.Size()? {
        let pad = pads.GetAt(i)?;
        let raw = RawGameController::FromGameController(&pad)?;
        if raw.HardwareVendorId().is_ok_and(|v| FAKE_BATTERY_VENDORS.contains(&v)) {
            continue;
        }
        let report = pad.TryGetBatteryReport()?;
        let status = report.Status()?;
        if status == BatteryStatus::NotPresent {
            continue; // wired controller without a battery
        }
        // Null (unknown) capacities come back as errors: skip those controllers.
        let (Ok(remaining), Ok(full)) = (report.RemainingCapacityInMilliwattHours(), report.FullChargeCapacityInMilliwattHours()) else {
            continue;
        };
        let (remaining, full) = (remaining.Value()?, full.Value()?);
        if full <= 0 {
            continue;
        }
        let percent = (100.0 * remaining as f32 / full as f32).round().clamp(0.0, 100.0) as u8;

        let name = raw.DisplayName().map(|n| n.to_string()).unwrap_or_default();
        let id = raw.NonRoamableId().map(|n| n.to_string()).unwrap_or_else(|_| i.to_string());
        out.push(Observation {
            id: format!("xinput:{id}"),
            name: Some(if name.is_empty() { "Xbox controller".to_string() } else { name }),
            kind: Some(Kind::Gamepad),
            state: State::Battery { percent, charging: status == BatteryStatus::Charging },
        });
    }
    Ok(out)
}
