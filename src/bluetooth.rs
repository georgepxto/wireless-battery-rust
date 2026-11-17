//! Bluetooth batteries, two ways:
//! - Bluetooth LE: the standard GATT Battery Service (0x180F / Battery Level 0x2A19). Most BLE
//!   mice and keyboards expose it.
//! - Classic Bluetooth (e.g. headsets over Hands-Free): Windows stores the level the device
//!   reports in a device property, the same one Settings > Bluetooth shows.

use crate::device::{Kind, Observation, State};
use crate::wide;
use std::collections::HashSet;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristicUuids, GattCommunicationStatus, GattDeviceService, GattServiceUuids,
};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Foundation::Collections::IVectorView;
use windows::Storage::Streams::DataReader;
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::Properties::DEVPROPKEY;

/// DEVPKEY for the battery level Windows shows for Bluetooth (Hands-Free) devices.
const BATTERY_LEVEL_KEY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5),
    pid: 2,
};

pub fn scan() -> Vec<Observation> {
    let mut out = Vec::new();
    if let Err(e) = scan_ble(&mut out) {
        debug(&format!("ble scan: {e}"));
    }
    scan_classic(&mut out);
    out
}

fn debug(msg: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[bt] {msg}");
    }
}

fn scan_ble(out: &mut Vec<Observation>) -> windows::core::Result<()> {
    let selector = BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)?;
    let infos = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.get()?;
    for i in 0..infos.Size()? {
        let info = infos.GetAt(i)?;
        let Ok(dev) = BluetoothLEDevice::FromIdAsync(&info.Id()?)?.get() else { continue };
        match read_ble(&dev) {
            Ok(Some(o)) => out.push(o),
            Ok(None) => {}
            Err(e) => debug(&format!("{}: {e}", dev.Name().unwrap_or_default())),
        }
        let _ = dev.Close();
    }
    Ok(())
}

fn read_ble(dev: &BluetoothLEDevice) -> windows::core::Result<Option<Observation>> {
    // Appearance: skip phones (0x0040..) and computers (0x0080..); classify HID and audio.
    let appearance = dev.Appearance()?.RawValue()?;
    let kind = match appearance >> 6 {
        0x01 | 0x02 => return Ok(None),
        0x0F => match appearance & 0x3F {
            1 => Some(Kind::Keyboard),
            2 => Some(Kind::Mouse),
            3 | 4 => Some(Kind::Gamepad),
            _ => None,
        },
        // Wearable audio device: 0x0941 earbud, 0x0942 headset, 0x0943 headphones.
        0x25 => match appearance & 0x3F {
            1 => Some(Kind::Earbuds),
            2 | 3 => Some(Kind::Headset),
            _ => None, // generic: decide by name
        },
        _ => None,
    };

    let result = dev.GetGattServicesForUuidWithCacheModeAsync(GattServiceUuids::Battery()?, BluetoothCacheMode::Cached)?.get()?;
    if result.Status()? != GattCommunicationStatus::Success {
        return Ok(None);
    }
    let services = result.Services()?;
    let level = read_level(&services);
    for i in 0..services.Size()? {
        let _ = services.GetAt(i)?.Close();
    }
    let Some(percent) = level? else { return Ok(None) };

    let name = dev.Name()?.to_string();
    if name.to_ascii_lowercase().contains("xbox") {
        return Ok(None); // reported (more precisely) by the xbox module
    }
    Ok(Some(Observation {
        id: format!("ble:{:012x}", dev.BluetoothAddress()?),
        kind: kind.or_else(|| Some(Kind::from_name(&name))),
        name: Some(name),
        state: State::Battery { percent: percent.min(100), charging: false },
    }))
}

fn read_level(services: &IVectorView<GattDeviceService>) -> windows::core::Result<Option<u8>> {
    if services.Size()? == 0 {
        return Ok(None);
    }
    let chars = services
        .GetAt(0)?
        .GetCharacteristicsForUuidWithCacheModeAsync(GattCharacteristicUuids::BatteryLevel()?, BluetoothCacheMode::Cached)?
        .get()?;
    if chars.Status()? != GattCommunicationStatus::Success || chars.Characteristics()?.Size()? == 0 {
        return Ok(None);
    }
    let value = chars.Characteristics()?.GetAt(0)?.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?.get()?;
    if value.Status()? != GattCommunicationStatus::Success {
        return Ok(None);
    }
    Ok(Some(DataReader::FromBuffer(&value.Value()?)?.ReadByte()?))
}

fn scan_classic(out: &mut Vec<Observation>) {
    let mut seen = HashSet::new();
    for (addr, percent) in classic_levels() {
        if !seen.insert(addr) {
            continue; // one device has several profile nodes
        }
        let Ok(dev) = BluetoothDevice::FromBluetoothAddressAsync(addr).and_then(|op| op.get()) else { continue };
        let connected = dev.ConnectionStatus().is_ok_and(|s| s == BluetoothConnectionStatus::Connected);
        let name = dev.Name().map(|n| n.to_string()).unwrap_or_default();
        let cod = dev.ClassOfDevice().and_then(|c| c.RawValue()).unwrap_or(0);
        let kind = match ((cod >> 8) & 0x1F, (cod >> 2) & 0x3F) {
            // Audio/video. The minor class rarely separates earbuds from headphones
            // (AirPods report "headphones"), so the name decides.
            (0x04, _) => match Kind::from_name(&name) {
                Kind::Earbuds => Kind::Earbuds,
                _ => Kind::Headset,
            },
            (0x05, minor) if minor & 0x20 != 0 => Kind::Mouse, // peripheral: pointing
            (0x05, minor) if minor & 0x10 != 0 => Kind::Keyboard,
            (0x05, _) => Kind::Gamepad,
            _ => Kind::from_name(&name),
        };
        let _ = dev.Close();
        out.push(Observation {
            id: format!("bt:{addr:012x}"),
            // Paired-but-disconnected devices keep a stale level; only add them once seen connected.
            name: (connected && !name.is_empty()).then_some(name),
            kind: Some(kind),
            state: if connected { State::Battery { percent: percent.min(100), charging: false } } else { State::Off },
        });
    }
}

/// (bluetooth address, level) for every present BTHENUM device node carrying the battery property.
fn classic_levels() -> Vec<(u64, u8)> {
    let mut out = Vec::new();
    unsafe {
        let enumerator = wide("BTHENUM");
        let set = SetupDiGetClassDevsW(null(), enumerator.as_ptr(), null_mut(), DIGCF_ALLCLASSES | DIGCF_PRESENT);
        if set as isize == -1 {
            return out;
        }
        let mut i = 0;
        loop {
            let mut data: SP_DEVINFO_DATA = zeroed();
            data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, i, &mut data) == 0 {
                break;
            }
            i += 1;
            let (mut ptype, mut needed) = (0u32, 0u32);
            let mut level = [0u8; 4];
            if SetupDiGetDevicePropertyW(set, &data, &BATTERY_LEVEL_KEY, &mut ptype, level.as_mut_ptr(), 4, &mut needed, 0) == 0 {
                continue;
            }
            let mut id = [0u16; 512];
            if SetupDiGetDeviceInstanceIdW(set, &data, id.as_mut_ptr(), id.len() as u32, &mut needed) == 0 {
                continue;
            }
            let len = id.iter().position(|c| *c == 0).unwrap_or(id.len());
            if let Some(addr) = address_from_instance_id(&String::from_utf16_lossy(&id[..len])) {
                out.push((addr, level[0]));
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// "BTHENUM\{...}_VID&..._PID&...\7&3057B904&0&F5CFD77BCB0B_C00000000" -> 0xF5CFD77BCB0B
fn address_from_instance_id(id: &str) -> Option<u64> {
    let last = id.rsplit('\\').next()?;
    let mac = last.rsplit('&').next()?.split('_').next()?;
    (mac.len() == 12).then(|| u64::from_str_radix(mac, 16).ok()).flatten()
}
