use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Peer {
    pub token_file: String,
    pub bdp_url: String,
    pub bdp_port: u16,
    pub bdp_group: String,
    pub bdp_name: String,
    pub bdp_count: u16,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub peers: Vec<Peer>,
    pub bondriver_dir: String,
    pub cloudflared: String,
    pub edcb_ini: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            peers: Vec::new(),
            bondriver_dir: if cfg!(windows) {
                r"C:\DTV\EDCB\BonDriver".into()
            } else {
                "/usr/local/lib/edcb".into()
            },
            cloudflared: if cfg!(windows) {
                r"C:\Program Files (x86)\cloudflared\cloudflared.exe".into()
            } else {
                "/usr/bin/cloudflared".into()
            },
            edcb_ini: String::new(),
        }
    }
}

pub fn write_atomic(path: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, data).and_then(|_| fs::rename(&tmp, path)).map_err(|e| format!("{}: {e}", path.display()))
}

pub(crate) fn read_token(path: &str) -> Result<(String, String), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (mut id, mut secret) = (None, None);
    for line in text.trim_start_matches('\u{feff}').lines() {
        let l = line.trim_start();
        let l = match l.strip_prefix("set") {
            Some(r) if r.starts_with(char::is_whitespace) => r.trim_start(),
            _ => l,
        };
        match l.split_once('=') {
            Some(("TUNNEL_SERVICE_TOKEN_ID", v)) => id = Some(v.trim().to_string()),
            Some(("TUNNEL_SERVICE_TOKEN_SECRET", v)) => secret = Some(v.trim().to_string()),
            _ => {}
        }
    }
    Ok((
        id.ok_or("missing TUNNEL_SERVICE_TOKEN_ID")?,
        secret.ok_or("missing TUNNEL_SERVICE_TOKEN_SECRET")?,
    ))
}
