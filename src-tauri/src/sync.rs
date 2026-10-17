use serde::{Deserialize, Serialize};
use std::fs::{self, File, FileTimes};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const USER_AGENT: &str = "epgsync/1.0";

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Peer {
    pub url: String,
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
    pub epg_dir: String,
    pub listen: String,
    pub own: Vec<String>,
    pub own_invert: bool,
    pub peers: Vec<Peer>,
    pub reload_addr: String,
    pub bondriver_dir: String,
    pub cloudflared: String,
    pub edcb_ini: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            epg_dir: if cfg!(windows) {
                r"C:\DTV\EDCB\Setting\EpgData".into()
            } else {
                "/var/local/edcb/Setting/EpgData".into()
            },
            listen: "127.0.0.1:8770".into(),
            own: Vec::new(),
            own_invert: false,
            peers: Vec::new(),
            reload_addr: "127.0.0.1:4510".into(),
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

pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("EPGSYNC_CONFIG") {
        return p.into();
    }
    if cfg!(windows) {
        PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into()))
            .join("EpgSync")
            .join("config.json")
    } else {
        PathBuf::from("/etc/epgsync/config.json")
    }
}

pub fn load() -> Result<Config, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save(cfg: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    write_atomic(&path, text)
}

pub fn write_atomic(path: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, data).and_then(|_| fs::rename(&tmp, path)).map_err(|e| format!("{}: {e}", path.display()))
}

fn is_name(n: &str) -> bool {
    n.len() == 16
        && n.is_ascii()
        && n[..8].bytes().all(|b| b.is_ascii_hexdigit())
        && n[8..].eq_ignore_ascii_case("_epg.dat")
}

pub fn is_own(cfg: &Config, n: &str) -> bool {
    if !is_name(n) {
        return false;
    }
    let hit = cfg.own.iter().map(|p| p.trim().as_bytes()).any(|p| !p.is_empty() && n.len() >= p.len() && n.as_bytes()[..p.len()].eq_ignore_ascii_case(p));
    hit != cfg.own_invert
}

fn meta_secs(meta: &fs::Metadata) -> Option<u64> {
    if !meta.is_file() {
        return None;
    }
    Some(meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

fn mtime_secs(path: &Path) -> Option<u64> {
    meta_secs(&fs::metadata(path).ok()?)
}

pub fn list_own(cfg: &Config) -> Result<Vec<(String, u64, u64)>, String> {
    let mut rows = Vec::new();
    for entry in fs::read_dir(&cfg.epg_dir).map_err(|e| format!("{}: {e}", cfg.epg_dir))? {
        let Ok(entry) = entry else { continue };
        let Ok(name) = entry.file_name().into_string() else { continue };
        if !is_own(cfg, &name) {
            continue;
        }
        let Ok(meta) = fs::metadata(entry.path()) else { continue };
        let Some(m) = meta_secs(&meta) else { continue };
        rows.push((name, meta.len(), m));
    }
    rows.sort();
    Ok(rows)
}

pub fn serve() -> Result<(), String> {
    let cfg = load()?;
    let server = tiny_http::Server::http(&cfg.listen).map_err(|e| format!("{}: {e}", cfg.listen))?;
    for req in server.incoming_requests() {
        std::thread::spawn(move || handle(req));
    }
    Ok(())
}

fn handle(req: tiny_http::Request) {
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let resp = match load() {
        Err(e) => {
            eprintln!("{e}");
            text(503, "config error\n".into())
        }
        Ok(cfg) => respond(&cfg, &path).unwrap_or_else(|| text(404, "not found\n".into())),
    };
    let _ = req.respond(resp);
}

fn content_type(v: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], v.as_bytes()).unwrap()
}

fn text(code: u16, body: String) -> tiny_http::ResponseBox {
    tiny_http::Response::from_data(body.into_bytes()).with_status_code(code).with_header(content_type("text/plain")).boxed()
}

fn respond(cfg: &Config, path: &str) -> Option<tiny_http::ResponseBox> {
    if path == "/epg" || path == "/epg/" {
        let rows: Vec<String> = list_own(cfg).ok()?.into_iter().map(|(n, s, m)| format!("{n}\t{s}\t{m}")).collect();
        return Some(text(200, rows.join("\n") + "\n"));
    }
    let name = path.strip_prefix("/epg/")?;
    if !is_own(cfg, name) {
        return None;
    }
    let data = fs::read(Path::new(&cfg.epg_dir).join(name)).ok()?;
    Some(tiny_http::Response::from_data(data).with_header(content_type("application/octet-stream")).boxed())
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

pub fn reload_epg(addr: &str) -> Result<i32, String> {
    let sa = addr.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("invalid address")?;
    (|| -> std::io::Result<i32> {
        let mut s = TcpStream::connect_timeout(&sa, Duration::from_secs(30))?;
        s.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut req = [0u8; 8];
        req[..4].copy_from_slice(&2i32.to_le_bytes());
        s.write_all(&req)?;
        let mut res = [0u8; 4];
        s.read_exact(&mut res)?;
        Ok(i32::from_le_bytes(res))
    })()
    .map_err(|e| e.to_string())
}

fn pull_peer(cfg: &Config, agent: &ureq::Agent, peer: &Peer, changed: &mut Vec<String>, log: &mut Vec<String>) -> Result<(), String> {
    let (id, secret) = read_token(&peer.token_file)?;
    let base = if peer.url.ends_with('/') { peer.url.clone() } else { format!("{}/", peer.url) };
    let get = |url: &str| -> Result<Vec<u8>, String> {
        let resp = agent
            .get(url)
            .set("CF-Access-Client-Id", &id)
            .set("CF-Access-Client-Secret", &secret)
            .call()
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        resp.into_reader().read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    };
    let list = String::from_utf8_lossy(&get(&base)?).into_owned();
    for line in list.lines() {
        let [name, size, mtime] = line.split('\t').collect::<Vec<_>>()[..] else { continue };
        if !is_name(name) || is_own(cfg, name) {
            continue;
        }
        let (Ok(size), Ok(mtime)) = (size.parse::<u64>(), mtime.parse::<u64>()) else { continue };
        let dst = Path::new(&cfg.epg_dir).join(name);
        if mtime_secs(&dst).is_some_and(|m| m >= mtime) {
            continue;
        }
        let data = get(&format!("{base}{name}"))?;
        if data.len() as u64 != size {
            log.push(format!("{name}: size mismatch {} != {size}", data.len()));
            continue;
        }
        let tmp = dst.with_file_name(format!("{name}.sync"));
        let t = UNIX_EPOCH + Duration::from_secs(mtime);
        let written = (|| -> std::io::Result<()> {
            let mut f = File::create(&tmp)?;
            f.write_all(&data)?;
            f.set_times(FileTimes::new().set_accessed(t).set_modified(t))?;
            drop(f);
            fs::rename(&tmp, &dst)
        })();
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(format!("{name}: {e}"));
        }
        changed.push(name.to_string());
    }
    Ok(())
}

pub fn pull(cfg: &Config) -> Vec<String> {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(30)).timeout_read(Duration::from_secs(120)).user_agent(USER_AGENT).build();
    let (mut log, mut changed) = (Vec::new(), Vec::new());
    for peer in &cfg.peers {
        if !Path::new(&peer.token_file).is_file() {
            log.push(format!("{}: skipped (no token)", peer.url));
            continue;
        }
        if let Err(e) = pull_peer(cfg, &agent, peer, &mut changed, &mut log) {
            log.push(format!("{}: {e}", peer.url));
        }
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    log.push(format!("{now} updated: {}", if changed.is_empty() { "-".into() } else { changed.join(" ") }));
    if !changed.is_empty() {
        match reload_epg(&cfg.reload_addr) {
            Ok(r) => log.push(format!("reload: {r}")),
            Err(e) => log.push(format!("reload failed: {e}")),
        }
    }
    log
}
