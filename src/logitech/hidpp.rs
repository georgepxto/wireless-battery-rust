//! Minimal HID++ transport for Logitech devices.
//!
//! Message layout: [report_id, device_index, feature_index, (function << 4) | sw_id, params...]
//!
//! On Windows each report size is a separate HID collection, so we write long reports to the
//! long collection but also listen on the short one: receivers answer some errors (e.g. "no
//! device in this slot") with short HID++ 1.0 reports.

use hidapi::{HidApi, HidDevice};
use std::ffi::CString;
use std::fmt;
use std::time::{Duration, Instant};

pub const LOGITECH_VID: u16 = 0x046D;

const REPORT_SHORT: u8 = 0x10;
const REPORT_LONG: u8 = 0x11;
const LEN_LONG: usize = 20;
/// Our software id, so we can tell our replies apart from G HUB's.
const SW_ID: u8 = 0x0B;
const TIMEOUT: Duration = Duration::from_millis(600);

/// HID++ 1.0 error: no device paired at this receiver index.
pub const ERR_UNKNOWN_DEVICE: u8 = 0x08;

#[derive(Debug)]
pub enum Error {
    Hid(hidapi::HidError),
    /// No reply in time (device asleep or off).
    Timeout,
    /// Device or receiver returned an HID++ error code.
    Protocol(u8),
}

impl From<hidapi::HidError> for Error {
    fn from(e: hidapi::HidError) -> Self {
        Error::Hid(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Hid(e) => write!(f, "hid: {e}"),
            Error::Timeout => write!(f, "timeout"),
            Error::Protocol(c) => write!(f, "hid++ error 0x{c:02X}"),
        }
    }
}

/// One HID++-capable USB interface (a receiver, or a directly attached device/dongle).
pub struct Endpoint {
    pub pid: u16,
    pub iface: i32,
    pub product: String,
    short: Option<CString>,
    long: CString,
}

/// Groups Logitech vendor-defined HID collections by interface, keeping those that accept
/// long (HID++ 2.0) reports.
pub fn endpoints(api: &HidApi) -> Vec<Endpoint> {
    struct Partial {
        pid: u16,
        iface: i32,
        product: String,
        short: Option<CString>,
        long: Option<CString>,
    }
    let mut groups: Vec<Partial> = Vec::new();
    for d in api.device_list().filter(|d| d.vendor_id() == LOGITECH_VID && d.usage_page() >= 0xFF00) {
        let i = match groups.iter().position(|g| g.pid == d.product_id() && g.iface == d.interface_number()) {
            Some(i) => i,
            None => {
                groups.push(Partial {
                    pid: d.product_id(),
                    iface: d.interface_number(),
                    product: d.product_string().unwrap_or_default().trim().to_string(),
                    short: None,
                    long: None,
                });
                groups.len() - 1
            }
        };
        match d.usage() & 0xFF {
            0x01 => groups[i].short = Some(d.path().to_owned()),
            0x02 => groups[i].long = Some(d.path().to_owned()),
            _ => {}
        }
    }
    groups
        .into_iter()
        .filter_map(|g| Some(Endpoint { pid: g.pid, iface: g.iface, product: g.product, short: g.short, long: g.long? }))
        .collect()
}

pub struct Link {
    short: Option<HidDevice>,
    long: HidDevice,
}

impl Link {
    pub fn open(api: &HidApi, ep: &Endpoint) -> Result<Self, Error> {
        let long = api.open_path(&ep.long)?;
        let short = ep.short.as_ref().and_then(|p| api.open_path(p).ok());
        Ok(Self { short, long })
    }

    /// Sends a request to device `index` (1..=6 behind a receiver, 0xFF for a direct device)
    /// and waits for the matching reply; returns its 16 parameter bytes.
    pub fn request(&self, index: u8, feature_idx: u8, function: u8, params: &[u8]) -> Result<[u8; 16], Error> {
        let func_sw = (function << 4) | SW_ID;
        let mut tx = [0u8; LEN_LONG];
        tx[0] = REPORT_LONG;
        tx[1] = index;
        tx[2] = feature_idx;
        tx[3] = func_sw;
        let n = params.len().min(16);
        tx[4..4 + n].copy_from_slice(&params[..n]);
        self.long.write(&tx)?;

        let deadline = Instant::now() + TIMEOUT;
        let mut rx = [0u8; 64];
        loop {
            if let Some(short) = &self.short {
                loop {
                    let n = short.read_timeout(&mut rx, 0)?;
                    if n == 0 {
                        break;
                    }
                    if let Some(r) = reply(&rx[..n], index, feature_idx, func_sw) {
                        return r;
                    }
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            let slice = remaining.min(Duration::from_millis(10)).as_millis() as i32;
            let n = self.long.read_timeout(&mut rx, slice)?;
            if n > 0 {
                if let Some(r) = reply(&rx[..n], index, feature_idx, func_sw) {
                    return r;
                }
            }
        }
    }

    /// Root feature lookup: maps a feature id (e.g. 0x1001) to its index on the device.
    pub fn feature_index(&self, index: u8, feature: u16) -> Result<Option<u8>, Error> {
        let r = self.request(index, 0x00, 0x0, &feature.to_be_bytes())?;
        Ok((r[0] != 0).then_some(r[0]))
    }
}

/// Matches a report against our pending request.
fn reply(rx: &[u8], index: u8, feature_idx: u8, func_sw: u8) -> Option<Result<[u8; 16], Error>> {
    if rx.len() < 6 || (rx[0] != REPORT_SHORT && rx[0] != REPORT_LONG) || rx[1] != index {
        return None;
    }
    // HID++ 2.0 error (0xFF) or HID++ 1.0 error from the receiver (0x8F).
    if (rx[2] == 0xFF || rx[2] == 0x8F) && rx[3] == feature_idx && rx[4] == func_sw {
        return Some(Err(Error::Protocol(rx[5])));
    }
    if rx[2] == feature_idx && rx[3] == func_sw {
        let mut out = [0u8; 16];
        let m = (rx.len() - 4).min(16);
        out[..m].copy_from_slice(&rx[4..4 + m]);
        return Some(Ok(out));
    }
    None
}
