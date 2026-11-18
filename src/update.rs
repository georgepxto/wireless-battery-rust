//! "Procurar atualizacoes": consulta o ultimo release no GitHub, baixa o novo executavel,
//! confere o SHA-256 publicado no release, troca o arquivo e reinicia o app.
//!
//! So acessa a rede quando o usuario escolhe a opcao no menu. O Windows deixa renomear um
//! executavel em uso, entao o atual vira `wireless-battery.old` (apagado na proxima abertura)
//! e o novo ocupa o lugar dele, no mesmo caminho do "Iniciar com o Windows".

use crate::{json, wide};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

const REPO: &str = "georgepxto/wireless-battery-rust";
const ASSET: &str = "wireless-battery.exe";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub sha256: Option<String>,
}

/// "v0.8.1" -> [0, 8, 1]
fn parse_version(v: &str) -> Option<Vec<u32>> {
    v.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.parse().ok()).collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Le o JSON de /releases/latest.
pub fn parse_release(text: &str) -> Option<Release> {
    let v = json::parse(text)?;
    let tag = v.get("tag_name")?.str()?;
    let url = v
        .get("assets")?
        .arr()
        .iter()
        .find(|a| a.get("name").and_then(|n| n.str()) == Some(ASSET))?
        .get("browser_download_url")?
        .str()?;
    let body = v.get("body").and_then(|b| b.str()).unwrap_or("");
    Some(Release { version: tag.trim_start_matches(['v', 'V']).to_string(), url: url.to_string(), sha256: find_sha256(body) })
}

/// O release publica "SHA-256: `<64 hex>`".
fn find_sha256(body: &str) -> Option<String> {
    body.split(|c: char| !c.is_ascii_hexdigit()).find(|w| w.len() == 64).map(|w| w.to_ascii_lowercase())
}

/// "https://host/caminho?x" -> ("host", "/caminho?x")
fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    Some(match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    })
}

/// Ha alguma versao nova? `Ok(None)` = ja esta na mais recente.
pub fn check() -> Result<Option<Release>, String> {
    let body = http_get(&format!("https://api.github.com/repos/{REPO}/releases/latest"), "application/vnd.github+json")?;
    let text = String::from_utf8_lossy(&body);
    let rel = parse_release(&text).ok_or("resposta do GitHub inesperada")?;
    Ok(is_newer(&rel.version, CURRENT).then_some(rel))
}

/// Baixa, confere e troca o executavel. Depois disso basta abrir o novo e sair.
pub fn install(rel: &Release) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data = http_get(&rel.url, "application/octet-stream")?;
    if data.len() < 100_000 || !data.starts_with(b"MZ") {
        return Err("o arquivo baixado nao e um executavel valido".into());
    }
    if let Some(expected) = &rel.sha256 {
        let got = sha256_hex(&data)?;
        if &got != expected {
            return Err("o SHA-256 do arquivo baixado nao confere com o do release".into());
        }
    }
    let new = exe.with_extension("new");
    let old = exe.with_extension("old");
    std::fs::write(&new, &data).map_err(|e| format!("nao foi possivel gravar {}: {e}", new.display()))?;
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(format!("nao foi possivel substituir o executavel: {e}"));
    }
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // desfaz
        return Err(format!("nao foi possivel substituir o executavel: {e}"));
    }
    Ok(exe)
}

/// Apaga o executavel antigo deixado por uma atualizacao.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

pub fn restart(exe: &Path) -> bool {
    std::process::Command::new(exe).arg("--updated").spawn().is_ok()
}

fn http_get(url: &str, accept: &str) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Networking::WinHttp::*;
    let (host, path) = split_url(url).ok_or("endereco invalido")?;
    let fail = |what: &str| format!("{what} (erro {})", unsafe { windows_sys::Win32::Foundation::GetLastError() });
    struct H(*mut core::ffi::c_void);
    impl Drop for H {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
    unsafe {
        let agent = wide(&format!("wireless-battery/{CURRENT}"));
        // 4 = WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY (Windows 8.1+): usa o proxy do sistema.
        let session = H(WinHttpOpen(agent.as_ptr(), 4, null(), null(), 0));
        if session.0.is_null() {
            return Err(fail("sem acesso a rede"));
        }
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 30_000);
        let host_w = wide(host);
        let conn = H(WinHttpConnect(session.0, host_w.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if conn.0.is_null() {
            return Err(fail("nao foi possivel conectar"));
        }
        let path_w = wide(path);
        let get = wide("GET");
        let req = H(WinHttpOpenRequest(conn.0, get.as_ptr(), path_w.as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE));
        if req.0.is_null() {
            return Err(fail("nao foi possivel abrir a requisicao"));
        }
        let headers = wide(&format!("Accept: {accept}\r\n"));
        if WinHttpSendRequest(req.0, headers.as_ptr(), u32::MAX, null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req.0, null_mut()) == 0 {
            return Err(fail("falha ao falar com o GitHub"));
        }
        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status as *mut u32 as *mut _,
            &mut len,
            null_mut(),
        );
        if status != 200 {
            return Err(format!("o GitHub respondeu {status}"));
        }
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            if WinHttpReadData(req.0, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut read) == 0 {
                return Err(fail("download interrompido"));
            }
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
            if out.len() > 50 << 20 {
                return Err("arquivo grande demais".into());
            }
        }
        Ok(out)
    }
}

fn sha256_hex(data: &[u8]) -> Result<String, String> {
    use windows_sys::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut digest = [0u8; 32];
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, null(), 0, data.as_ptr(), data.len() as u32, digest.as_mut_ptr(), 32) };
    if st != 0 {
        return Err(format!("falha ao calcular o SHA-256 ({st:#x})"));
    }
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.8.1", "0.8.0"));
        assert!(is_newer("v0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.8.0", "0.8.0"));
        assert!(!is_newer("0.7.9", "0.8.0"));
        assert!(!is_newer("abc", "0.8.0"));
    }

    #[test]
    fn release_json() {
        let hash = "a".repeat(64);
        let text = format!(
            r#"{{"tag_name":"v0.9.0","body":"SHA-256: `{hash}`","assets":[{{"name":"sha256.md","browser_download_url":"https://x/a"}},{{"name":"wireless-battery.exe","browser_download_url":"https://github.com/georgepxto/wireless-battery-rust/releases/download/v0.9.0/wireless-battery.exe"}}]}}"#
        );
        let r = parse_release(&text).unwrap();
        assert_eq!(r.version, "0.9.0");
        assert!(r.url.ends_with("/v0.9.0/wireless-battery.exe"));
        assert_eq!(r.sha256.as_deref(), Some(hash.as_str()));
        assert_eq!(split_url(&r.url), Some(("github.com", "/georgepxto/wireless-battery-rust/releases/download/v0.9.0/wireless-battery.exe")));
    }
}
