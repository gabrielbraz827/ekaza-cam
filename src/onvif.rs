//! Cliente ONVIF minimo: GetProfiles + GetStreamUri, com WS-Security opcional.
#![allow(dead_code)]

use crate::crypto;
use regex::Regex;
use std::net::Ipv4Addr;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Profile {
    pub token: String,
    pub uri: String,
}

#[derive(Clone, Debug)]
pub struct CameraInfo {
    pub ip: Ipv4Addr,
    pub device_service: String,
    pub profiles: Vec<Profile>,
    /// true se o ONVIF so respondeu apos autenticar.
    pub auth_required: bool,
}

impl CameraInfo {
    /// Escolhe o perfil para visualizar: sub-stream se pedido e existir, senao o principal.
    pub fn pick_uri(&self, sub: bool) -> Option<String> {
        if self.profiles.is_empty() {
            return None;
        }
        if sub && self.profiles.len() >= 2 {
            Some(self.profiles[1].uri.clone())
        } else {
            Some(self.profiles[0].uri.clone())
        }
    }
}

/// Consulta uma camera: resolve perfis e URLs RTSP.
pub fn query(
    ip: Ipv4Addr,
    device_service: &str,
    user: &str,
    pass: &str,
    timeout: Duration,
) -> Result<CameraInfo, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .build();

    let endpoints = media_endpoints(device_service);

    // 1) GetProfiles sem autenticacao.
    let mut auth_required = false;
    let mut profiles_resp: Option<(String, String)> = None; // (endpoint, body)
    for ep in &endpoints {
        let body = soap(&agent, ep, &get_profiles_body(None));
        if let Some(body) = body {
            if has_profiles(&body) {
                profiles_resp = Some((ep.clone(), body));
                break;
            }
        }
    }

    // 2) Se falhou e temos senha, tenta com WS-Security.
    if profiles_resp.is_none() {
        for ep in &endpoints {
            let sec = security_header(user, pass);
            let body = soap(&agent, ep, &get_profiles_body(Some(&sec)));
            if let Some(body) = body {
                if has_profiles(&body) {
                    auth_required = true;
                    profiles_resp = Some((ep.clone(), body));
                    break;
                }
            }
        }
    }

    let (endpoint, body) = profiles_resp.ok_or_else(|| {
        "sem perfis (ONVIF nao respondeu ou exige credenciais corretas)".to_string()
    })?;

    let tokens = parse_profile_tokens(&body);
    if tokens.is_empty() {
        return Err("perfis vazios na resposta ONVIF".into());
    }

    // 3) GetStreamUri para cada perfil.
    let mut profiles = Vec::new();
    for token in tokens {
        let sec = if auth_required {
            Some(security_header(user, pass))
        } else {
            None
        };
        let req = get_stream_uri_body(&token, sec.as_deref());
        if let Some(resp) = soap(&agent, &endpoint, &req) {
            if let Some(uri) = parse_uri(&resp) {
                profiles.push(Profile { token, uri });
            }
        }
    }

    Ok(CameraInfo {
        ip,
        device_service: device_service.to_string(),
        profiles,
        auth_required,
    })
}

/// POST SOAP; devolve o corpo mesmo em erro HTTP (faults vem como 4xx/5xx com body).
fn soap(agent: &ureq::Agent, url: &str, xml: &str) -> Option<String> {
    let res = agent
        .post(url)
        .set("Content-Type", "application/soap+xml; charset=utf-8")
        .send_bytes(xml.as_bytes());
    match res {
        Ok(resp) => resp.into_string().ok(),
        Err(ureq::Error::Status(_, resp)) => resp.into_string().ok(),
        Err(_) => None,
    }
}

/// Deriva endpoints de media a partir do device_service.
fn media_endpoints(device_service: &str) -> Vec<String> {
    // base = esquema://host:porta
    let base = device_service
        .rsplit_once("/onvif/")
        .map(|(b, _)| b.to_string())
        .or_else(|| {
            // fallback: tira o path se houver
            device_service
                .find("/onvif")
                .map(|i| device_service[..i].to_string())
        })
        .unwrap_or_else(|| device_service.to_string());

    vec![
        format!("{base}/onvif/media_service"),
        format!("{base}/onvif/Media"),
        format!("{base}/onvif/media"),
        device_service.to_string(), // alguns aceitam tudo no device_service
    ]
}

fn security_header(user: &str, pass: &str) -> String {
    let (digest, nonce, created) = crypto::ws_password_digest(pass);
    format!(
        r#"<wsse:Security xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd" xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd" s:mustUnderstand="1"><wsse:UsernameToken><wsse:Username>{user}</wsse:Username><wsse:Password Type="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest">{digest}</wsse:Password><wsse:Nonce EncodingType="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary">{nonce}</wsse:Nonce><wsu:Created>{created}</wsu:Created></wsse:UsernameToken></wsse:Security>"#
    )
}

fn envelope(header_inner: Option<&str>, body_inner: &str) -> String {
    let header = header_inner.unwrap_or("");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:trt="http://www.onvif.org/ver10/media/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema"><s:Header>{header}</s:Header><s:Body>{body_inner}</s:Body></s:Envelope>"#
    )
}

fn get_profiles_body(sec: Option<&str>) -> String {
    envelope(sec, "<trt:GetProfiles/>")
}

fn get_stream_uri_body(token: &str, sec: Option<&str>) -> String {
    let body = format!(
        r#"<trt:GetStreamUri><trt:StreamSetup><tt:Stream>RTP-Unicast</tt:Stream><tt:Transport><tt:Protocol>RTSP</tt:Protocol></tt:Transport></trt:StreamSetup><trt:ProfileToken>{token}</trt:ProfileToken></trt:GetStreamUri>"#
    );
    envelope(sec, &body)
}

fn has_profiles(body: &str) -> bool {
    body.contains("GetProfilesResponse") || body.contains(":Profiles") || body.contains("<Profiles")
}

fn parse_profile_tokens(body: &str) -> Vec<String> {
    let re = Regex::new(r#"(?s)Profiles[^>]*?token="([^"]+)""#).unwrap();
    let mut out = Vec::new();
    for c in re.captures_iter(body) {
        let t = c[1].to_string();
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn parse_uri(body: &str) -> Option<String> {
    let re = Regex::new(r#"(?s)Uri>\s*(rtsp://[^<\s]+)"#).unwrap();
    re.captures(body).map(|c| c[1].to_string())
}
