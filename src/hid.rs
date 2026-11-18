//! Camada HID propria sobre a API do Windows (hid.dll + SetupAPI), sem dependencias.
//!
//! Segue a semantica do `hid.cpp` de krisgfx9/Wireless-Device-Battery-Monitor (GPL-3.0),
//! que por sua vez segue a do hidapi, e expoe a mesma interface do crate `hidapi` usada
//! pelos leitores mais antigos:
//! - Escrita / feature: `data[0]` e o report id (0 quando o dispositivo nao numera os
//!   reports); o buffer e completado com zeros ate o tamanho do report.
//! - Leitura: um report id 0 na frente e removido, entao `data[0]` e o primeiro byte do
//!   payload em reports sem numero e o report id nos numerados.

use std::ffi::{CStr, CString};
use std::fmt;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FlushQueue, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid, HidD_GetInputReport,
    HidD_GetManufacturerString, HidD_GetPreparsedData, HidD_GetProductString, HidD_GetSerialNumberString, HidD_SetFeature,
    HidD_SetOutputReport, HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS,
};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;

#[derive(Debug, Clone)]
pub struct HidError(pub String);

impl fmt::Display for HidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HidError {}

pub type HidResult<T> = Result<T, HidError>;

fn err(what: &str) -> HidError {
    HidError(format!("{what} (erro {})", unsafe { GetLastError() }))
}

/// Uma colecao HID (o Windows enumera cada colecao de topo como um dispositivo).
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    path: CString,
    vid: u16,
    pid: u16,
    iface: i32,
    usage_page: u16,
    usage: u16,
    in_len: u16,
    out_len: u16,
    feat_len: u16,
    product: String,
    manufacturer: String,
    serial: String,
}

impl DeviceInfo {
    pub fn path(&self) -> &CStr {
        &self.path
    }
    pub fn vendor_id(&self) -> u16 {
        self.vid
    }
    pub fn product_id(&self) -> u16 {
        self.pid
    }
    /// Interface USB (`MI_xx` no caminho), -1 quando desconhecida.
    pub fn interface_number(&self) -> i32 {
        self.iface
    }
    pub fn usage_page(&self) -> u16 {
        self.usage_page
    }
    pub fn usage(&self) -> u16 {
        self.usage
    }
    pub fn input_len(&self) -> u16 {
        self.in_len
    }
    pub fn output_len(&self) -> u16 {
        self.out_len
    }
    pub fn feature_len(&self) -> u16 {
        self.feat_len
    }
    pub fn product_string(&self) -> Option<&str> {
        (!self.product.is_empty()).then_some(self.product.as_str())
    }
    pub fn manufacturer_string(&self) -> Option<&str> {
        (!self.manufacturer.is_empty()).then_some(self.manufacturer.as_str())
    }
    pub fn serial_number(&self) -> Option<&str> {
        (!self.serial.is_empty()).then_some(self.serial.as_str())
    }
}

pub struct HidApi {
    list: Vec<DeviceInfo>,
}

impl HidApi {
    pub fn new() -> HidResult<Self> {
        Ok(Self { list: enumerate() })
    }

    pub fn refresh_devices(&mut self) -> HidResult<()> {
        self.list = enumerate();
        Ok(())
    }

    pub fn device_list(&self) -> impl Iterator<Item = &DeviceInfo> {
        self.list.iter()
    }

    fn info(&self, path: &CStr) -> HidResult<&DeviceInfo> {
        self.list.iter().find(|d| d.path.as_c_str() == path).ok_or_else(|| HidError("colecao nao encontrada".into()))
    }

    /// Leitura e escrita; se o Windows nao deixar (teclados e mouses ficam com o sistema),
    /// abre sem acesso, o que ainda permite feature reports. Igual ao hidapi.
    pub fn open_path(&self, path: &CStr) -> HidResult<HidDevice> {
        let info = self.info(path)?;
        HidDevice::open(info, Mode::ReadWrite).or_else(|_| HidDevice::open(info, Mode::FeatureOnly))
    }

    pub fn open(&self, info: &DeviceInfo, mode: Mode) -> HidResult<HidDevice> {
        HidDevice::open(info, mode)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Leitura e escrita, assincrono.
    ReadWrite,
    /// So leitura: para colecoes que so enviam e que outro app pode estar segurando.
    ReadOnly,
    /// Sem acesso: basta para feature reports, funciona em mouses e teclados.
    FeatureOnly,
}

fn wide_path(p: &CStr) -> Vec<u16> {
    p.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect()
}

fn utf16_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().to_string()
}

fn enumerate() -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    unsafe {
        let mut guid: GUID = zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set as isize == -1 {
            return out;
        }
        let mut i = 0;
        loop {
            let mut ifd: SP_DEVICE_INTERFACE_DATA = zeroed();
            ifd.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, null(), &guid, i, &mut ifd) == 0 {
                break;
            }
            i += 1;
            let mut need = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &ifd, null_mut(), 0, &mut need, null_mut());
            if need == 0 {
                continue;
            }
            // Buffer alinhado em u32 para a struct com cbSize + caminho.
            let mut buf = vec![0u32; (need as usize).div_ceil(4)];
            let det = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*det).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &ifd, det, need, null_mut(), null_mut()) == 0 {
                continue;
            }
            let path_ptr = std::ptr::addr_of!((*det).DevicePath) as *const u16;
            let mut len = 0;
            while *path_ptr.add(len) != 0 {
                len += 1;
            }
            let path = String::from_utf16_lossy(std::slice::from_raw_parts(path_ptr, len));
            if let Some(info) = describe(&path) {
                out.push(info);
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

unsafe fn describe(path: &str) -> Option<DeviceInfo> {
    let cpath = CString::new(path).ok()?;
    let wpath = wide_path(&cpath);
    let h = CreateFileW(wpath.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut attr: HIDD_ATTRIBUTES = zeroed();
    attr.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
    let (vid, pid) = if HidD_GetAttributes(h, &mut attr) != 0 { (attr.VendorID, attr.ProductID) } else { (0, 0) };
    let mut caps: HIDP_CAPS = zeroed();
    let mut pp = 0;
    if HidD_GetPreparsedData(h, &mut pp) != 0 {
        if HidP_GetCaps(pp, &mut caps) != HIDP_STATUS_SUCCESS {
            caps = zeroed();
        }
        HidD_FreePreparsedData(pp);
    }
    let string = |f: unsafe extern "system" fn(HANDLE, *mut core::ffi::c_void, u32) -> u8| {
        let mut b = [0u16; 128];
        if f(h, b.as_mut_ptr() as *mut _, (b.len() * 2 - 2) as u32) != 0 {
            utf16_string(&b)
        } else {
            String::new()
        }
    };
    let product = string(HidD_GetProductString);
    let manufacturer = string(HidD_GetManufacturerString);
    let serial = string(HidD_GetSerialNumberString);
    CloseHandle(h);

    let lower = path.to_ascii_lowercase();
    let iface = lower
        .find("&mi_")
        .and_then(|i| lower.get(i + 4..i + 6))
        .and_then(|s| i32::from_str_radix(s, 16).ok())
        .unwrap_or(-1);
    Some(DeviceInfo {
        path: cpath,
        vid,
        pid,
        iface,
        usage_page: caps.UsagePage,
        usage: caps.Usage,
        in_len: caps.InputReportByteLength,
        out_len: caps.OutputReportByteLength,
        feat_len: caps.FeatureReportByteLength,
        product,
        manufacturer,
        serial,
    })
}

pub struct HidDevice {
    h: HANDLE,
    overlapped: bool,
    in_len: usize,
    out_len: usize,
    feat_len: usize,
}

// O HANDLE e so um numero do kernel; o dispositivo e usado por uma thread de cada vez.
unsafe impl Send for HidDevice {}

impl Drop for HidDevice {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.h) };
    }
}

/// Evento + OVERLAPPED de uma operacao assincrona.
struct Async {
    ov: OVERLAPPED,
}

impl Async {
    fn new() -> Self {
        let mut ov: OVERLAPPED = unsafe { zeroed() };
        ov.hEvent = unsafe { CreateEventW(null(), 1, 0, null()) };
        Self { ov }
    }

    /// Espera ate `ms` (0 = so confere); cancela se nao terminou. Devolve os bytes transferidos.
    fn finish(&mut self, h: HANDLE, ms: u32) -> Option<u32> {
        let mut n = 0u32;
        unsafe {
            if ms > 0 && WaitForSingleObject(self.ov.hEvent, ms) == WAIT_OBJECT_0 {
                (GetOverlappedResult(h, &self.ov, &mut n, 0) != 0).then_some(n)
            } else {
                CancelIo(h);
                // Pode ter terminado nesse meio tempo.
                (GetOverlappedResult(h, &self.ov, &mut n, 1) != 0).then_some(n)
            }
        }
    }
}

impl Drop for Async {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.ov.hEvent) };
    }
}

impl HidDevice {
    fn open(info: &DeviceInfo, mode: Mode) -> HidResult<Self> {
        let (access, flags) = match mode {
            Mode::ReadWrite => (GENERIC_READ | GENERIC_WRITE, FILE_FLAG_OVERLAPPED),
            Mode::ReadOnly => (GENERIC_READ, FILE_FLAG_OVERLAPPED),
            Mode::FeatureOnly => (0, 0),
        };
        let wpath = wide_path(&info.path);
        let h = unsafe { CreateFileW(wpath.as_ptr(), access, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, flags, null_mut()) };
        if h == INVALID_HANDLE_VALUE {
            return Err(err("nao abriu"));
        }
        Ok(Self {
            h,
            overlapped: mode != Mode::FeatureOnly,
            in_len: info.in_len as usize,
            out_len: info.out_len as usize,
            feat_len: info.feat_len as usize,
        })
    }

    fn padded(data: &[u8], len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        let n = data.len().min(len);
        buf[..n].copy_from_slice(&data[..n]);
        buf
    }

    /// Output report pelo endpoint de interrupcao, com o buffer no tamanho do report.
    pub fn write(&self, data: &[u8]) -> HidResult<usize> {
        if !self.overlapped || self.out_len == 0 || data.is_empty() {
            return Err(HidError("colecao sem output report".into()));
        }
        let buf = Self::padded(data, self.out_len);
        let mut a = Async::new();
        let started = unsafe { WriteFile(self.h, buf.as_ptr(), buf.len() as u32, null_mut(), &mut a.ov) != 0 || GetLastError() == ERROR_IO_PENDING };
        if !started {
            return Err(err("escrita falhou"));
        }
        a.finish(self.h, 500).map(|_| data.len()).ok_or_else(|| err("escrita falhou"))
    }

    /// Le um input report. `Ok(0)` quando nada chegou no tempo (igual ao hidapi).
    pub fn read_timeout(&self, out: &mut [u8], ms: i32) -> HidResult<usize> {
        match self.read(ms.max(0) as u32) {
            Some(data) => {
                let n = data.len().min(out.len());
                out[..n].copy_from_slice(&data[..n]);
                Ok(n)
            }
            None => Ok(0),
        }
    }

    /// Le um input report (sem o report id 0). `None` quando nada chegou no tempo.
    pub fn read(&self, ms: u32) -> Option<Vec<u8>> {
        if !self.overlapped || self.in_len == 0 {
            return None;
        }
        let mut tmp = vec![0u8; self.in_len];
        let mut a = Async::new();
        let started = unsafe { ReadFile(self.h, tmp.as_mut_ptr(), tmp.len() as u32, null_mut(), &mut a.ov) != 0 || GetLastError() == ERROR_IO_PENDING };
        if !started {
            return None;
        }
        let got = a.finish(self.h, ms)? as usize;
        Self::strip(tmp, got)
    }

    fn strip(mut tmp: Vec<u8>, got: usize) -> Option<Vec<u8>> {
        if got == 0 {
            return None;
        }
        tmp.truncate(got);
        if tmp[0] == 0 {
            tmp.remove(0);
        }
        Some(tmp)
    }

    /// Para colecoes sem endpoint OUT (o WriteFile falha nelas): arma uma leitura, envia
    /// `data` com HidD_SetOutputReport e devolve o primeiro input report.
    pub fn set_output_and_read(&self, data: &[u8], ms: u32) -> Option<Vec<u8>> {
        if !self.overlapped || self.in_len == 0 || self.out_len == 0 || data.is_empty() {
            return None;
        }
        let out = Self::padded(data, self.out_len);
        let mut tmp = vec![0u8; self.in_len];
        let mut a = Async::new();
        let started = unsafe { ReadFile(self.h, tmp.as_mut_ptr(), tmp.len() as u32, null_mut(), &mut a.ov) != 0 || GetLastError() == ERROR_IO_PENDING };
        if !started {
            return None;
        }
        let sent = unsafe { HidD_SetOutputReport(self.h, out.as_ptr() as *const _, out.len() as u32) != 0 };
        let got = a.finish(self.h, if sent { ms } else { 0 })? as usize;
        if !sent {
            return None;
        }
        Self::strip(tmp, got)
    }

    /// Descarta os input reports na fila.
    pub fn flush(&self) {
        unsafe { HidD_FlushQueue(self.h) };
    }

    /// Feature report; `data[0]` e o report id.
    pub fn send_feature_report(&self, data: &[u8]) -> HidResult<()> {
        if self.feat_len == 0 {
            return Err(HidError("colecao sem feature report".into()));
        }
        let buf = Self::padded(data, self.feat_len);
        if unsafe { HidD_SetFeature(self.h, buf.as_ptr() as *const _, buf.len() as u32) } != 0 {
            Ok(())
        } else {
            Err(err("feature report falhou"))
        }
    }

    /// Le um feature report; `buf[0]` e o report id. Devolve os bytes copiados (id incluso).
    pub fn get_feature_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        let id = *buf.first().ok_or_else(|| HidError("buffer vazio".into()))?;
        let data = self.get_feature(id).ok_or_else(|| err("leitura de feature falhou"))?;
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    /// Feature report inteiro (`[0]` = report id).
    pub fn get_feature(&self, report_id: u8) -> Option<Vec<u8>> {
        if self.feat_len == 0 {
            return None;
        }
        let mut out = vec![0u8; self.feat_len];
        out[0] = report_id;
        (unsafe { HidD_GetFeature(self.h, out.as_mut_ptr() as *mut _, out.len() as u32) } != 0).then_some(out)
    }

    /// Input report atual pela linha de controle (`[0]` = report id).
    #[allow(dead_code)]
    pub fn get_input_report(&self, report_id: u8) -> Option<Vec<u8>> {
        if self.in_len == 0 {
            return None;
        }
        let mut out = vec![0u8; self.in_len];
        out[0] = report_id;
        (unsafe { HidD_GetInputReport(self.h, out.as_mut_ptr() as *mut _, out.len() as u32) } != 0).then_some(out)
    }

    #[allow(dead_code)]
    pub fn input_len(&self) -> usize {
        self.in_len
    }
    #[allow(dead_code)]
    pub fn output_len(&self) -> usize {
        self.out_len
    }
    pub fn feature_len(&self) -> usize {
        self.feat_len
    }
}
