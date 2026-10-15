use crate::sync::{self, Config, Peer};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
const EXT: &str = "dll";
#[cfg(not(windows))]
const EXT: &str = "so";

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn relays(cfg: &Config) -> impl Iterator<Item = &Peer> {
    cfg.peers.iter().filter(|p| !p.bdp_url.trim().is_empty())
}

fn module_path(cfg: &Config, name: &str) -> PathBuf {
    Path::new(&cfg.bondriver_dir).join(format!("BonDriver_Proxy_{name}.{EXT}"))
}

fn setting_path(module: &Path) -> PathBuf {
    if cfg!(windows) {
        module.with_extension("ini")
    } else {
        PathBuf::from(format!("{}.conf", module.display()))
    }
}

fn set_keys(text: &[u8], keys: &[(&str, String)]) -> Vec<u8> {
    let mut lines: Vec<Vec<u8>> = text.split(|&b| b == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    for (key, value) in keys {
        let entry = format!("{key}={value}").into_bytes();
        let prefix = format!("{key}=").into_bytes();
        if let Some(l) = lines.iter_mut().find(|l| l.trim_ascii_start().starts_with(&prefix)) {
            *l = entry;
        } else if let Some(i) = lines.iter().position(|l| l.trim_ascii().eq_ignore_ascii_case(b"[OPTION]")) {
            lines.insert(i + 1, entry);
        } else {
            lines.push(entry);
        }
    }
    let eol: &[u8] = if cfg!(windows) { b"\r\n" } else { b"\n" };
    let mut out = lines.join(eol);
    out.extend_from_slice(eol);
    out
}

fn write_module(cfg: &Config, peer: &Peer) -> Result<String, String> {
    let template = Path::new(&cfg.bondriver_dir).join(format!("BonDriver_Proxy.{EXT}"));
    let module = module_path(cfg, &peer.bdp_name);
    let setting = setting_path(&module);
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let src = fs::read(&template).map_err(|e| err(&template, e))?;
    if fs::read(&module).ok().as_deref() != Some(&src[..]) {
        fs::write(&module, &src).map_err(|e| err(&module, e))?;
    }
    let base = fs::read(&setting).or_else(|_| fs::read(setting_path(&template))).unwrap_or_default();
    let text = set_keys(
        &base,
        &[
            ("ADDRESS", "127.0.0.1".into()),
            ("PORT", peer.bdp_port.to_string()),
            ("BONDRIVER", peer.bdp_group.trim().into()),
            ("CHANNEL_LOCK", "0".into()),
            ("CONNECT_TIMEOUT", "10".into()),
        ],
    );
    fs::write(&setting, text).map_err(|e| err(&setting, e))?;
    Ok(format!("{} / {}", module.display(), setting.display()))
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(format!("{cmd} {}: {}", args.join(" "), if msg.is_empty() { out.status.to_string() } else { msg }))
}

fn exe() -> Result<String, String> {
    Ok(std::env::current_exe().map_err(|e| e.to_string())?.display().to_string())
}

#[cfg(windows)]
const TASK_PREFIX: &str = "EpgSync Relay ";

#[cfg(windows)]
fn register(name: &str) -> Result<(), String> {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let args = format!("--config \"{}\" relay {name}", sync::config_path().display());
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><BootTrigger><Enabled>true</Enabled></BootTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>S-1-5-18</UserId><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions>
</Task>
"#,
        esc(&exe()?),
        esc(&args)
    );
    let file = std::env::temp_dir().join(format!("epgsync-relay-{name}.xml"));
    let bytes: Vec<u8> = std::iter::once(0xFEFFu16).chain(xml.encode_utf16()).flat_map(u16::to_le_bytes).collect();
    fs::write(&file, bytes).map_err(|e| e.to_string())?;
    let task = format!("{TASK_PREFIX}{name}");
    let result = run("schtasks", &["/Create", "/TN", &task, "/XML", &file.display().to_string(), "/F"]);
    let _ = fs::remove_file(&file);
    result?;
    let _ = run("schtasks", &["/End", "/TN", &task]);
    run("schtasks", &["/Run", "/TN", &task])
}

#[cfg(windows)]
fn registered() -> Vec<String> {
    let Ok(out) = Command::new("schtasks").args(["/Query", "/FO", "CSV", "/NH"]).output() else { return Vec::new() };
    let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split(',').next())
        .map(|s| s.trim_matches('"').trim_start_matches('\\').to_string())
        .filter_map(|s| s.strip_prefix(TASK_PREFIX).map(str::to_string))
        .collect();
    names.dedup();
    names
}

#[cfg(windows)]
fn unregister(name: &str) -> Result<(), String> {
    let task = format!("{TASK_PREFIX}{name}");
    let _ = run("schtasks", &["/End", "/TN", &task]);
    run("schtasks", &["/Delete", "/TN", &task, "/F"])
}

#[cfg(not(windows))]
const UNIT_DIR: &str = "/etc/systemd/system";

#[cfg(not(windows))]
fn unit(name: &str) -> String {
    format!("epgsync-relay-{name}.service")
}

#[cfg(not(windows))]
fn register(name: &str) -> Result<(), String> {
    let text = format!(
        "[Unit]\nDescription=EpgSync relay {name}\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nExecStart=\"{}\" --config \"{}\" relay {name}\nRestart=always\nRestartSec=10\n\n[Install]\nWantedBy=multi-user.target\n",
        exe()?,
        sync::config_path().display()
    );
    let path = Path::new(UNIT_DIR).join(unit(name));
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", &unit(name)])?;
    run("systemctl", &["restart", &unit(name)])
}

#[cfg(not(windows))]
fn registered() -> Vec<String> {
    let Ok(dir) = fs::read_dir(UNIT_DIR) else { return Vec::new() };
    dir.filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_prefix("epgsync-relay-")?.strip_suffix(".service").map(str::to_string))
        .collect()
}

#[cfg(not(windows))]
fn unregister(name: &str) -> Result<(), String> {
    let _ = run("systemctl", &["disable", "--now", &unit(name)]);
    fs::remove_file(Path::new(UNIT_DIR).join(unit(name))).map_err(|e| e.to_string())?;
    run("systemctl", &["daemon-reload"])
}

pub fn apply(cfg: &Config) -> Vec<String> {
    let mut log = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut ports = Vec::new();
    let same = |a: &str, b: &str| if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b };
    for peer in relays(cfg) {
        let name = peer.bdp_name.trim();
        if !valid_name(name) || peer.bdp_port == 0 || peer.bdp_group.trim().is_empty() {
            log.push(format!("{}: BonDriver の名前・ポート・グループ名を確認してください", peer.bdp_url));
            continue;
        }
        if names.iter().any(|n| n.eq_ignore_ascii_case(name)) || ports.contains(&peer.bdp_port) {
            log.push(format!("{name}: 名前かポート ({}) がほかの取得先と重複しています", peer.bdp_port));
            continue;
        }
        names.push(name.to_string());
        ports.push(peer.bdp_port);
        match write_module(cfg, peer) {
            Ok(m) => log.push(format!("{name}: {m}")),
            Err(e) => {
                log.push(format!("{name}: {e}"));
                continue;
            }
        }
        match register(name) {
            Ok(()) => log.push(format!("{name}: 中継を登録しました (127.0.0.1:{} → {})", peer.bdp_port, peer.bdp_url)),
            Err(e) => log.push(format!("{name}: 中継の登録に失敗しました: {e}")),
        }
    }
    for stale in registered().into_iter().filter(|s| !names.iter().any(|n| same(n, s))) {
        match unregister(&stale) {
            Ok(()) => log.push(format!("{stale}: 中継の登録を外しました")),
            Err(e) => log.push(format!("{stale}: 中継の削除に失敗しました: {e}")),
        }
    }
    if log.is_empty() {
        log.push("BonDriver の設定がある取得先はありません".into());
    }
    log
}

pub fn relay(name: &str) -> Result<(), String> {
    loop {
        let found = sync::load().and_then(|cfg| {
            let peer = relays(&cfg).find(|p| p.bdp_name.trim() == name).cloned().ok_or(format!("{name}: not configured"))?;
            Ok((cfg, peer))
        });
        match found.and_then(|(cfg, peer)| Ok((sync::read_token(&peer.token_file).map_err(|e| format!("{name}: {}: {e}", peer.token_file))?, cfg, peer))) {
            Ok(((id, secret), cfg, peer)) => {
                let status = Command::new(&cfg.cloudflared)
                    .args(["access", "tcp", "--hostname", peer.bdp_url.trim(), "--url", &format!("127.0.0.1:{}", peer.bdp_port), "--loglevel", "warn"])
                    .env("TUNNEL_SERVICE_TOKEN_ID", id)
                    .env("TUNNEL_SERVICE_TOKEN_SECRET", secret)
                    .status();
                eprintln!("{name}: cloudflared exited: {status:?}");
            }
            Err(e) => eprintln!("{e}"),
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn apply_log() -> PathBuf {
    sync::config_path().with_file_name("apply.log")
}

pub fn apply_elevated() -> Result<Vec<String>, String> {
    let log = apply_log();
    let _ = fs::remove_file(&log);
    let exe = exe()?;
    let config = sync::config_path().display().to_string();
    let ok = if cfg!(windows) {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let args = format!("@('--config', {}, 'apply')", quote(&format!("\"{config}\"")));
        let script = format!("Start-Process -FilePath {} -ArgumentList {args} -Verb RunAs -Wait -WindowStyle Hidden", quote(&exe));
        Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).status()
    } else {
        Command::new("pkexec").args([exe.as_str(), "--config", &config, "apply"]).status()
    }
    .map_err(|e| e.to_string())?
    .success();
    match fs::read_to_string(&log) {
        Ok(text) => Ok(text.lines().map(str::to_string).collect()),
        Err(_) if !ok => Err("管理者権限での実行が取り消されたか、失敗しました".into()),
        Err(e) => Err(format!("{}: {e}", log.display())),
    }
}

pub fn apply_cli() -> Result<(), String> {
    let lines = apply(&sync::load()?);
    let _ = fs::write(apply_log(), lines.join("\n"));
    lines.iter().for_each(|l| println!("{l}"));
    Ok(())
}
