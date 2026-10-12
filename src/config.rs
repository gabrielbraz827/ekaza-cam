//! Persistência de configuração e cache de câmeras.
//!
//! Guarda credenciais/preferências e a lista de câmeras já descobertas, para
//! não precisar re-escanear a rede a cada abertura. O arquivo fica no diretório
//! de configuração do sistema (Linux: `~/.config/ekaza-cam/config.json`,
//! Windows: `%APPDATA%\ekaza-cam\config.json`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Uma câmera em cache (já resolvida via ONVIF).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedCamera {
    pub ip: String,
    pub main_uri: Option<String>,
    pub sub_uri: Option<String>,
    #[serde(default)]
    pub status: String,
}

/// Configuração persistida do app.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub user: String,
    pub pass: String,
    pub remember_pass: bool,
    pub subnet: String,
    pub timeout: u64,
    pub use_sub: bool,
    pub grid: bool,
    pub cameras: Vec<SavedCamera>,
    /// Momento da última varredura (RFC 3339), só informativo.
    pub last_scan: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            user: "admin".to_string(),
            pass: String::new(),
            remember_pass: true,
            subnet: String::new(),
            timeout: 4,
            use_sub: false,
            grid: false,
            cameras: Vec::new(),
            last_scan: None,
        }
    }
}

impl Config {
    /// Caminho do arquivo de config, se o diretório do sistema for acessível.
    pub fn path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "ekaza-cam")?;
        Some(dirs.config_dir().join("config.json"))
    }

    /// Carrega do disco; em qualquer falha, devolve o padrão.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(txt) => serde_json::from_str(&txt).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Salva no disco (cria o diretório se preciso). Respeita `remember_pass`.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut to_write = self.clone();
        if !to_write.remember_pass {
            to_write.pass.clear();
        }
        let json = serde_json::to_string_pretty(&to_write)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, json)
    }
}
