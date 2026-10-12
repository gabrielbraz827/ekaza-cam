//! Descoberta de câmeras na rede via ONVIF WS-Discovery (multicast + unicast /24).

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

/// Um dispositivo descoberto: IP + endpoint do serviço ONVIF.
#[derive(Clone, Debug)]
pub struct Device {
    pub ip: Ipv4Addr,
    pub device_service: String,
}

/// Monta o XML de Probe do WS-Discovery.
fn probe_message() -> String {
    let msg_id: u128 = rand::random();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope" xmlns:w="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery" xmlns:dn="http://www.onvif.org/ver10/network/wsdl"><e:Header><w:MessageID>urn:uuid:{msg_id:032x}</w:MessageID><w:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</w:To><w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action></e:Header><e:Body><d:Probe><d:Types>dn:NetworkVideoTransmitter</d:Types></d:Probe></e:Body></e:Envelope>"#
    )
}

/// Extrai o primeiro XAddrs (URL do device_service) de uma resposta de discovery.
fn extract_xaddr(body: &str) -> Option<String> {
    let re = regex::Regex::new(r"XAddrs>\s*([^<\s]+)").ok()?;
    let caps = re.captures(body)?;
    caps.get(1).map(|m| m.as_str().trim().to_string())
}

/// Descobre o /24 local conectando (sem enviar pacote) a um destino externo.
pub fn local_subnet() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    if let std::net::IpAddr::V4(v4) = sock.local_addr().ok()?.ip() {
        let o = v4.octets();
        return Some(format!("{}.{}.{}", o[0], o[1], o[2]));
    }
    None
}

/// WS-Discovery combinando multicast e varredura unicast do /24.
pub fn discover(subnet: Option<String>, timeout: Duration) -> Vec<Device> {
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    sock.set_read_timeout(Some(Duration::from_millis(400))).ok();
    let _ = sock.set_broadcast(true);
    let msg = probe_message();

    // 1) Probe multicast
    let _ = sock.send_to(msg.as_bytes(), "239.255.255.250:3702");

    // 2) Varredura unicast em todo o /24
    let prefix = subnet.or_else(local_subnet);
    if let Some(pfx) = &prefix {
        for host in 1..=254u8 {
            let addr = format!("{pfx}.{host}:3702");
            let _ = sock.send_to(msg.as_bytes(), &addr);
        }
    }

    // 3) Coleta respostas até o timeout
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut devices: Vec<Device> = Vec::new();
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 8192];
    while Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, src)) => {
                let body = String::from_utf8_lossy(&buf[..n]);
                if let Some(xaddr) = extract_xaddr(&body) {
                    let ip = match src.ip() {
                        std::net::IpAddr::V4(v4) => v4,
                        _ => continue,
                    };
                    if found.insert(xaddr.clone()) {
                        devices.push(Device {
                            ip,
                            device_service: xaddr,
                        });
                    }
                }
            }
            Err(_) => { /* timeout de leitura: segue até o deadline */ }
        }
    }
    devices.sort_by_key(|d| d.ip);
    devices
}

/// Resolve o device_service de um único IP, via probe unicast; fallback para portas comuns.
pub fn resolve_single(ip: Ipv4Addr, onvif_port: u16, timeout: Duration) -> Option<Device> {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        sock.set_read_timeout(Some(timeout)).ok();
        let _ = sock.send_to(probe_message().as_bytes(), (ip, 3702));
        let mut buf = [0u8; 8192];
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            let body = String::from_utf8_lossy(&buf[..n]);
            if let Some(xaddr) = extract_xaddr(&body) {
                return Some(Device {
                    ip,
                    device_service: xaddr,
                });
            }
        }
    }
    let ports: Vec<u16> = if onvif_port != 0 {
        vec![onvif_port]
    } else {
        vec![80, 8080, 8000, 10000, 2020, 8899]
    };
    for p in ports {
        if TcpStream::connect_timeout(&SocketAddr::from((ip, p)), Duration::from_millis(500))
            .is_ok()
        {
            return Some(Device {
                ip,
                device_service: format!("http://{ip}:{p}/onvif/device_service"),
            });
        }
    }
    None
}
