//! Avisos de bateria baixa (notificacoes do Windows).
//! Apps sem pacote podem mostrar toasts depois de registrar o AppUserModelID em
//! HKCU\Software\Classes\AppUserModelId\<id>. Baseado em renanleonel/windows-taskbar-battery (MIT).

use crate::device::{Reading, Status};
use crate::render::LOW_BATTERY;
use crate::settings;
use std::collections::HashSet;
use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

pub const APP_ID: &str = "WirelessBattery";
/// Rearma o aviso so depois de subir acima disto (evita repetir perto do limite).
const REARM_ABOVE: u8 = LOW_BATTERY + 5;

#[derive(Default)]
pub struct LowBatteryAlerts {
    alerted: HashSet<String>,
}

impl LowBatteryAlerts {
    pub fn check(&mut self, readings: &[Reading]) {
        for r in readings {
            let (percent, charging, stale) = match r.status {
                Status::Battery { percent, charging, stale } => (percent, charging, stale),
                Status::ChargingNoLevel => {
                    self.alerted.remove(&r.id);
                    continue;
                }
                Status::NotConnected => continue,
            };
            if charging || percent > REARM_ABOVE {
                self.alerted.remove(&r.id);
            } else if percent <= LOW_BATTERY && !stale && self.alerted.insert(r.id.clone()) && settings::alerts_enabled() {
                let body = format!("{} está com {percent}%. Hora de carregar.", r.name);
                if let Err(e) = show("Bateria baixa", &body) {
                    if cfg!(debug_assertions) {
                        eprintln!("[notify] {e}");
                    }
                }
            }
        }
    }
}

pub fn show(title: &str, body: &str) -> windows::core::Result<()> {
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
