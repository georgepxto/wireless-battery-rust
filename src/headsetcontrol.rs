//! Integracao opcional com o HeadsetControl (https://github.com/Sapd/HeadsetControl), que le
//! dezenas de headsets com dongle (SteelSeries, HyperX, Corsair, Audeze, Roccat...).
//!
//! Roda como programa separado (`headsetcontrol -o json`), entao nao vem junto e a licenca
//! GPL dele nao se aplica a este projeto. So e usado se estiver ao lado do nosso .exe ou no
//! PATH, e so na cadencia do Bluetooth (a cada 3 min), porque abre um processo.
//! Baseado em renanleonel/windows-taskbar-battery (MIT), com o serde trocado por json.rs.

use crate::device::{Kind, Observation, State};
use crate::json::{self, Value};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const EXE_NAMES: &[&str] = &["headsetcontrol.exe", "headsetcontrol-windows-x86_64.exe"];
const TIMEOUT: Duration = Duration::from_secs(5);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Procura o executavel do HeadsetControl ao lado do nosso .exe ou no PATH.
pub fn find() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let path_dirs = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    beside
        .into_iter()
        .chain(path_dirs)
        .flat_map(|dir| EXE_NAMES.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

/// Roda o HeadsetControl e devolve os headsets. `skip(vid, pid)` descarta os que outra fonte
/// ja le (Logitech por HID++, MCHOSE pelo protocolo proprio).
pub fn scan(exe: &Path, skip: impl Fn(u16, u16) -> bool) -> Vec<Observation> {
    run(exe).map(|out| parse(&out, skip)).unwrap_or_default()
}

fn parse(text: &str, skip: impl Fn(u16, u16) -> bool) -> Vec<Observation> {
    let Some(root) = json::parse(text) else { return Vec::new() };
    let Some(devices) = root.get("devices") else { return Vec::new() };
    devices
        .arr()
        .iter()
        .filter_map(|d| {
            let vid = parse_hex(d.get("id_vendor")?.str()?)?;
            let pid = parse_hex(d.get("id_product")?.str()?)?;
            if skip(vid, pid) {
                return None;
            }
            let battery = d.get("battery")?;
            let level = battery.get("level").and_then(Value::num).unwrap_or(-1.0);
            let state = match battery.get("status").and_then(Value::str)? {
                "BATTERY_AVAILABLE" if level >= 0.0 => State::Battery { percent: level.min(100.0) as u8, charging: false },
                "BATTERY_CHARGING" if level >= 0.0 => State::Battery { percent: level.min(100.0) as u8, charging: true },
                "BATTERY_CHARGING" => State::ChargingNoLevel,
                _ => State::Off, // indisponivel: headset desligado
            };
            let name = [d.get("device"), d.get("product")]
                .into_iter()
                .flatten()
                .filter_map(Value::str)
                .find(|s| !s.is_empty())
                .unwrap_or("Headset")
                .to_string();
            Some(Observation {
                id: format!("hc:{vid:04x}:{pid:04x}"),
                kind: Some(match Kind::from_name(&name) {
                    Kind::Earbuds => Kind::Earbuds,
                    _ => Kind::Headset,
                }),
                name: Some(name),
                state,
            })
        })
        .collect()
}

fn run(exe: &Path) -> Option<String> {
    let mut child = Command::new(exe)
        .args(["-o", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    let deadline = Instant::now() + TIMEOUT;
    while child.try_wait().ok()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().ok()?;
    String::from_utf8(out.stdout).ok()
}

fn parse_hex(s: &str) -> Option<u16> {
    u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"name":"HeadsetControl","version":"4.3.0","device_count":3,"devices":[
        {"status":"success","device":"SteelSeries Arctis Nova 7","vendor":"SteelSeries","product":"Arctis Nova 7",
         "id_vendor":"0x1038","id_product":"0x2202","battery":{"status":"BATTERY_CHARGING","level":64}},
        {"status":"success","device":"Logitech PRO X","vendor":"Logitech","product":"PRO X",
         "id_vendor":"0x046d","id_product":"0x0aba","battery":{"status":"BATTERY_AVAILABLE","level":90}},
        {"status":"success","device":"HyperX Cloud II","product":"Cloud II",
         "id_vendor":"0x0951","id_product":"0x1718","battery":{"status":"BATTERY_UNAVAILABLE","level":-1}}]}"#;

    #[test]
    fn parses_headsetcontrol_json() {
        let obs = parse(SAMPLE, |vid, _| vid == 0x046d);
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].id, "hc:1038:2202");
        assert_eq!(obs[0].name.as_deref(), Some("SteelSeries Arctis Nova 7"));
        assert_eq!(obs[0].state, State::Battery { percent: 64, charging: true });
        assert_eq!(obs[1].state, State::Off);
    }
}
