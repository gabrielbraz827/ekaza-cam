//! Utilitarios RTSP: normalizar URL, checar necessidade de credenciais (DESCRIBE)
//! com suporte a Digest e Basic, e montar a URL com credenciais para o player.
#![allow(dead_code)] // alguns helpers ficam disponiveis para recursos futuros (ex: botao "testar auth")

use crate::crypto;
use regex::Regex;
use std::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug)]
pub enum AuthStatus {
    /// Stream abre sem credenciais.
    Open,
    /// Exige credenciais e as fornecidas funcionaram.
    AuthOk,
    /// Exige credenciais e as fornecidas falharam.
    AuthFail,
    /// Nao foi possivel determinar.
    Unknown(String),
}

impl fmt::Display for AuthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthStatus::Open => write!(f, "sem senha ✓"),
            AuthStatus::AuthOk => write!(f, "senha OK ✓"),
            AuthStatus::AuthFail => write!(f, "senha necessaria ✗ (credenciais invalidas)"),
            AuthStatus::Unknown(s) => write!(f, "indeterminado ({s})"),
        }
    }
}

/// Garante esquema rtsp:// e porta :554 quando ausente; remove userinfo embutido.
pub fn normalize(uri: &str) -> String {
    let rest = uri.strip_prefix("rtsp://").unwrap_or(uri);
    let rest = match rest.find('@') {
        Some(i) => &rest[i + 1..],
        None => rest,
    };
    // separa authority (host[:port]) do path
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let authority = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:554")
    };
    format!("rtsp://{authority}{path}")
}

/// Insere user:pass@ na URL (para passar ao player). Sem usuario, devolve normalizada.
pub fn with_credentials(uri: &str, user: &str, pass: &str) -> String {
    let norm = normalize(uri);
    if user.is_empty() {
        return norm;
    }
    let rest = &norm["rtsp://".len()..];
    format!("rtsp://{user}:{pass}@{rest}")
}

/// Mascara a senha para log.
pub fn redact(uri: &str) -> String {
    let re = Regex::new(r"rtsp://([^:/@]+):([^@/]*)@").unwrap();
    re.replace(uri, "rtsp://$1:***@").to_string()
}

/// Separa (host, porta) de uma URL normalizada.
fn host_port(norm: &str) -> Option<(String, u16)> {
    let rest = norm.strip_prefix("rtsp://")?;
    let authority = rest.split('/').next()?;
    let (h, p) = authority.split_once(':')?;
    Some((h.to_string(), p.parse().ok()?))
}

/// Checa via DESCRIBE se o stream exige credenciais e se as fornecidas funcionam.
pub fn check_auth(norm_uri: &str, user: &str, pass: &str, timeout: Duration) -> AuthStatus {
    let (host, port) = match host_port(norm_uri) {
        Some(x) => x,
        None => return AuthStatus::Unknown("url invalida".into()),
    };

    // DESCRIBE sem autenticacao.
    let first = match describe(&host, port, norm_uri, None, 1, timeout) {
        Ok(r) => r,
        Err(e) => return AuthStatus::Unknown(e),
    };
    let code = status_code(&first);
    match code {
        Some(200) => return AuthStatus::Open,
        Some(401) => {}
        Some(c) => return AuthStatus::Unknown(format!("HTTP {c}")),
        None => return AuthStatus::Unknown("sem status".into()),
    }

    // Precisa autenticar. Monta header com base no desafio.
    let hdr = match build_auth_header(&first, "DESCRIBE", norm_uri, user, pass) {
        Some(h) => h,
        None => return AuthStatus::Unknown("desafio de auth nao reconhecido".into()),
    };
    let second = match describe(&host, port, norm_uri, Some(&hdr), 2, timeout) {
        Ok(r) => r,
        Err(e) => return AuthStatus::Unknown(e),
    };
    match status_code(&second) {
        Some(200) => AuthStatus::AuthOk,
        Some(401) | Some(403) => AuthStatus::AuthFail,
        Some(c) => AuthStatus::Unknown(format!("HTTP {c}")),
        None => AuthStatus::Unknown("sem status".into()),
    }
}

fn describe(
    host: &str,
    port: u16,
    url: &str,
    auth: Option<&str>,
    cseq: u32,
    timeout: Duration,
) -> Result<String, String> {
    let mut stream = TcpStream::connect((host, port)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    let mut req = format!(
        "DESCRIBE {url} RTSP/1.0\r\nCSeq: {cseq}\r\nAccept: application/sdp\r\nUser-Agent: ekaza-cam\r\n"
    );
    if let Some(a) = auth {
        req.push_str(a);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;

    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
}

fn status_code(resp: &str) -> Option<u16> {
    // "RTSP/1.0 200 OK"
    let first = resp.lines().next()?;
    first.split_whitespace().nth(1)?.parse().ok()
}

/// Monta Authorization a partir do WWW-Authenticate (Digest preferido, Basic fallback).
fn build_auth_header(
    challenge: &str,
    method: &str,
    url: &str,
    user: &str,
    pass: &str,
) -> Option<String> {
    let lower = challenge.to_ascii_lowercase();
    if lower.contains("digest") {
        let realm = extract(challenge, "realm")?;
        let nonce = extract(challenge, "nonce")?;
        let ha1 = crypto::md5_hex(&format!("{user}:{realm}:{pass}"));
        let ha2 = crypto::md5_hex(&format!("{method}:{url}"));
        let response = crypto::md5_hex(&format!("{ha1}:{nonce}:{ha2}"));
        Some(format!(
            "Authorization: Digest username=\"{user}\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{url}\", response=\"{response}\""
        ))
    } else if lower.contains("basic") {
        let token = crypto::b64(format!("{user}:{pass}").as_bytes());
        Some(format!("Authorization: Basic {token}"))
    } else {
        None
    }
}

fn extract(s: &str, key: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"{key}\s*=\s*"([^"]+)""#)).ok()?;
    re.captures(s).map(|c| c[1].to_string())
}
