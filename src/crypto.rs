//! Helpers de hashing/encoding usados no WS-Security (ONVIF) e no Digest (RTSP).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

pub fn b64(data: &[u8]) -> String {
    STANDARD.encode(data)
}

pub fn sha1(data: &[u8]) -> Vec<u8> {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(data);
    h.finalize().to_vec()
}

pub fn md5_hex(data: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(data.as_bytes());
    let out = h.finalize();
    let mut s = String::with_capacity(32);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Gera o trio (PasswordDigest, NonceBase64, Created) do WS-Security UsernameToken.
pub fn ws_password_digest(password: &str) -> (String, String, String) {
    let nonce: [u8; 16] = rand::random();
    let created = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut material = Vec::new();
    material.extend_from_slice(&nonce);
    material.extend_from_slice(created.as_bytes());
    material.extend_from_slice(password.as_bytes());
    let digest = b64(&sha1(&material));
    (digest, b64(&nonce), created)
}
