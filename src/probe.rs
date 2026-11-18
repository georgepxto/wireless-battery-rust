//! `--probe-mchose` (so com `--features probe`): sonda o protocolo do MCHOSE V9 Pro,
//! testando as combinacoes de pedido de bateria e imprimindo tudo o que chegar em
//! qualquer colecao HID. Baseado no `--probe` do jmjp/mchose-hub-native (MIT).

use crate::hid::{HidApi, HidDevice};
use std::time::{Duration, Instant};

/// Dongle MCHOSE (291D) e o headset pelo cabo USB (chip C-Media, 0D8C, nome "MCHOSE").
fn is_mchose(d: &crate::hid::DeviceInfo) -> bool {
    d.vendor_id() == 0x291D || (d.vendor_id() == 0x0D8C && d.product_string().unwrap_or("").to_uppercase().contains("MCHOSE"))
}

pub fn run() {
    let Ok(api) = HidApi::new() else {
        println!("HID falhou");
        return;
    };
    let mut ids: Vec<(u16, u16)> = api.device_list().filter(|d| is_mchose(d)).map(|d| (d.vendor_id(), d.product_id())).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        println!("nenhum dispositivo MCHOSE encontrado");
        return;
    }
    for (vid, pid) in ids {
        println!("\n======== {vid:04X}:{pid:04X} ========");
        probe_device(&api, vid, pid);
    }
    println!("\nfim");
}

fn probe_device(api: &HidApi, vid: u16, pid: u16) {
    let infos: Vec<_> = api.device_list().filter(|d| d.vendor_id() == vid && d.product_id() == pid).collect();
    let mut handles: Vec<(u16, HidDevice)> = Vec::new();
    for d in &infos {
        let page = d.usage_page();
        match api.open_path(d.path()) {
            Ok(h) => {
                println!("aberta {:04X}:{:04X} usage_page={page:04X} usage={:04X}", d.vendor_id(), d.product_id(), d.usage());
                handles.push((page, h));
            }
            Err(e) => println!("falhou {:04X} usage_page={page:04X}: {e}", d.product_id()),
        }
    }
    let get = |page: u16| handles.iter().find(|(p, _)| *p == page).map(|(_, h)| h);

    // Firmware por feature report 170 na colecao 0xFF22: alvo 0 = headset, 1 = dongle.
    if let Some(fw) = get(0xFF22) {
        for (target, label) in [(0u8, "headset"), (1u8, "dongle")] {
            let mut tx = [0u8; 64];
            tx[0] = 170;
            tx[1] = 0x01;
            tx[2] = target;
            let sent = fw.send_feature_report(&tx);
            std::thread::sleep(Duration::from_millis(300));
            let mut rx = [0u8; 65];
            rx[0] = 170;
            let got = fw.get_feature_report(&mut rx);
            match got {
                Ok(n) => println!("firmware {label}: envio {sent:?}, resposta {:02X?}", &rx[..n.min(12)]),
                Err(e) => println!("firmware {label}: envio {sent:?}, erro {e}"),
            }
        }
    }

    println!("\n-- escutando 2 s sem enviar nada --");
    listen(&handles, Duration::from_millis(2000));

    let unframed = {
        let mut p = vec![0u8; 63];
        p[0] = 0x65;
        p[1] = 0x01;
        p
    };
    // Pedido com moldura: AA 01 00 01 00 01 0B <xor> (GET_BATTERY_SLEEP).
    // xor = len(01) ^ tipo(00 01) ^ cmd(0B) = 0B.
    let framed = {
        let mut p = vec![0u8; 63];
        p[..8].copy_from_slice(&[0xAA, 0x01, 0x00, 0x01, 0x00, 0x01, 0x0B, 0x0B]);
        p
    };
    let short = {
        let mut p = vec![0u8; 15];
        p[0] = 0x65;
        p[1] = 0x01;
        p
    };

    for writer in [0xFF90u16, 0xFF82] {
        let Some(dev) = get(writer) else { continue };
        for report_id in [85u8, 65] {
            for (label, payload) in [("simples 0x65", &unframed), ("moldura cmd 11", &framed), ("curto 15 B", &short)] {
                let mut tx = vec![report_id];
                tx.extend_from_slice(payload);
                let res = dev.write(&tx);
                println!("\n-- {writer:04X} report {report_id} {label}: escrita {res:?} --");
                if res.is_ok() {
                    listen(&handles, Duration::from_millis(1500));
                }
            }
        }
    }
}

fn listen(handles: &[(u16, HidDevice)], dur: Duration) {
    let deadline = Instant::now() + dur;
    let mut any = false;
    let mut buf = [0u8; 128];
    while Instant::now() < deadline {
        for (page, h) in handles {
            if let Ok(n) = h.read_timeout(&mut buf, 5) {
                if n > 0 {
                    any = true;
                    println!("   {page:04X} <- {:02X?}", &buf[..n.min(24)]);
                }
            }
        }
    }
    if !any {
        println!("   (nada)");
    }
}
