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

fn module_file(name: &str) -> String {
    format!("BonDriver_Proxy_{name}.{EXT}")
}

fn module_path(cfg: &Config, name: &str) -> PathBuf {
    Path::new(&cfg.bondriver_dir).join(module_file(name))
}

fn utf16le(s: &str) -> Vec<u8> {
    std::iter::once(0xFEFFu16).chain(s.encode_utf16()).flat_map(u16::to_le_bytes).collect()
}

fn setting_path(module: &Path) -> PathBuf {
    if cfg!(windows) {
        module.with_extension("ini")
    } else {
        PathBuf::from(format!("{}.conf", module.display()))
    }
}

fn split_lines(text: &[u8]) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = text.split(|&b| b == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

fn join_lines(lines: &[Vec<u8>]) -> Vec<u8> {
    let eol: &[u8] = if cfg!(windows) { b"\r\n" } else { b"\n" };
    let mut out = lines.join(eol);
    out.extend_from_slice(eol);
    out
}

fn set_keys(text: &[u8], keys: &[(&str, String)]) -> Vec<u8> {
    let mut lines = split_lines(text);
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
    join_lines(&lines)
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
            ("RELAY_URL", peer.bdp_url.trim().into()),
            ("RELAY_TOKEN_FILE", peer.token_file.trim().into()),
            ("RELAY_CLOUDFLARED", cfg.cloudflared.trim().into()),
        ],
    );
    fs::write(&setting, text).map_err(|e| err(&setting, e))?;
    Ok(format!("{} / {}", module.display(), setting.display()))
}

fn ini_value(lines: &[Vec<u8>], key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    lines.iter().find_map(|l| l.trim_ascii().strip_prefix(prefix.as_bytes()).map(|v| String::from_utf8_lossy(v).trim().to_string()))
}

pub fn discover() -> Config {
    let mut cfg = Config::default();
    let suffix = if cfg!(windows) { ".ini".to_string() } else { format!(".{EXT}.conf") };
    let mut peers = Vec::new();
    for entry in fs::read_dir(&cfg.bondriver_dir).into_iter().flatten().flatten() {
        let Ok(file) = entry.file_name().into_string() else { continue };
        let Some(name) = file.strip_prefix("BonDriver_Proxy_").and_then(|f| f.strip_suffix(suffix.as_str())) else { continue };
        let Ok(text) = fs::read(entry.path()) else { continue };
        let lines = split_lines(&text);
        let Some(url) = ini_value(&lines, "RELAY_URL") else { continue };
        if let Some(c) = ini_value(&lines, "RELAY_CLOUDFLARED").filter(|c| !c.is_empty()) {
            cfg.cloudflared = c;
        }
        peers.push(Peer {
            token_file: ini_value(&lines, "RELAY_TOKEN_FILE").unwrap_or_default(),
            bdp_url: url,
            bdp_port: ini_value(&lines, "PORT").and_then(|p| p.parse().ok()).unwrap_or(0),
            bdp_group: ini_value(&lines, "BONDRIVER").unwrap_or_default(),
            bdp_name: name.to_string(),
            bdp_count: 0,
        });
    }
    peers.sort_by(|a, b| a.bdp_name.cmp(&b.bdp_name));
    cfg.peers = peers;
    cfg.edcb_ini = edcb_ini(&cfg).display().to_string();
    fill_counts(&mut cfg);
    cfg
}

pub fn default_edcb_ini(bondriver_dir: &str) -> PathBuf {
    if cfg!(windows) {
        Path::new(bondriver_dir).parent().unwrap_or(Path::new("")).join("EpgTimerSrv.ini")
    } else {
        PathBuf::from("/var/local/edcb/EpgTimerSrv.ini")
    }
}

pub fn edcb_ini(cfg: &Config) -> PathBuf {
    let ini = cfg.edcb_ini.trim();
    if !ini.is_empty() { PathBuf::from(ini) } else { default_edcb_ini(&cfg.bondriver_dir) }
}

fn num(v: &[u8]) -> Option<i64> {
    std::str::from_utf8(v).ok()?.trim().parse().ok()
}

fn section_range(lines: &[Vec<u8>], head: &[u8]) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| l.trim_ascii().eq_ignore_ascii_case(head))? + 1;
    let end = lines[start..].iter().position(|l| l.trim_ascii().starts_with(b"[")).map_or(lines.len(), |i| start + i);
    Some((start, end))
}

fn indexed(line: &[u8]) -> Option<(i64, &[u8])> {
    let t = line.trim_ascii();
    let p = t.iter().position(|&b| b == b'=')?;
    Some((num(&t[..p])?, t[p + 1..].trim_ascii()))
}

fn set_tuner(lines: &mut Vec<Vec<u8>>, file: &str, want: Option<u16>) -> bool {
    let on = want.is_some();
    let section = format!("[{file}]");
    let mut changed = false;
    match section_range(lines, section.as_bytes()) {
        None if on => {
            let mut bondriver = false;
            let mut priority = -1;
            for line in lines.iter().map(|l| l.trim_ascii()) {
                if line.starts_with(b"[") {
                    bondriver = line.len() > 10 && line[1..10].eq_ignore_ascii_case(b"BonDriver");
                } else if bondriver && let Some(v) = line.strip_prefix(b"Priority=") {
                    priority = priority.max(num(v).unwrap_or(-1));
                }
            }
            let entry = [String::new(), section, format!("Count={}", want.unwrap_or(0).max(1)), "EPGCount=1".into(), format!("Priority={}", priority + 1)];
            lines.extend(entry.map(String::into_bytes));
            changed = true;
        }
        Some((s, mut e)) => {
            if on && let Some(i) = (s..e).find(|&i| lines[i].trim_ascii().strip_prefix(b"GetEpg=").is_some_and(|v| num(v) == Some(0))) {
                lines.remove(i);
                e -= 1;
                changed = true;
            }
            let at =(s..e).find_map(|i| lines[i].trim_ascii().strip_prefix(b"Count=").map(|v| (i, num(v))));
            let count = at.and_then(|(_, n)| n).unwrap_or(0);
            let target = match want {
                None => 0,
                Some(0) => count.max(1),
                Some(n) => n.into(),
            };
            if count != target {
                let entry = format!("Count={target}").into_bytes();
                match at {
                    Some((i, _)) => lines[i] = entry,
                    None => lines.insert(s, entry),
                }
                changed = true;
            }
        }
        None => {}
    }
    let viewer = match section_range(lines, b"[TVTEST]") {
        None if on => {
            lines.push(b"[TVTEST]".to_vec());
            Some((lines.len(), lines.len()))
        }
        r => r,
    };
    if let Some((s, e)) = viewer {
        let mut keyed: Vec<(i64, &[u8])> = lines[s..e].iter().filter_map(|l| indexed(l)).collect();
        keyed.sort_by_key(|v| v.0);
        let mut values: Vec<Vec<u8>> = keyed.into_iter().map(|(_, v)| v.to_vec()).collect();
        let listed = values.iter().any(|v| v.eq_ignore_ascii_case(file.as_bytes()));
        if listed != on {
            if on {
                values.push(file.as_bytes().to_vec());
            } else {
                values.retain(|v| !v.eq_ignore_ascii_case(file.as_bytes()));
            }
            let rest: Vec<Vec<u8>> = lines.drain(s..e).filter(|l| !l.trim_ascii().starts_with(b"Num=") && indexed(l).is_none()).collect();
            let mut body = vec![format!("Num={}", values.len()).into_bytes()];
            body.extend(values.into_iter().enumerate().map(|(i, v)| [format!("{i}=").into_bytes(), v].concat()));
            body.extend(rest);
            lines.splice(s..s, body);
            changed = true;
        }
    }
    changed
}

fn read_ini(ini: &Path) -> Result<(Vec<Vec<u8>>, bool), String> {
    let raw = fs::read(ini).map_err(|e| format!("{}: {e}", ini.display()))?;
    let utf16 = raw.starts_with(&[0xFF, 0xFE]);
    let text = if utf16 {
        String::from_utf16_lossy(&raw[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).into_bytes()
    } else {
        raw
    };
    Ok((split_lines(&text), utf16))
}

pub fn fill_counts(cfg: &mut Config) {
    let Ok((lines, _)) = read_ini(&edcb_ini(cfg)) else { return };
    for peer in cfg.peers.iter_mut().filter(|p| p.bdp_count == 0 && valid_name(p.bdp_name.trim())) {
        let Some((s, e)) = section_range(&lines, format!("[{}]", module_file(peer.bdp_name.trim())).as_bytes()) else { continue };
        if let Some(n) = lines[s..e].iter().find_map(|l| num(l.trim_ascii().strip_prefix(b"Count=")?)) {
            peer.bdp_count = n.clamp(0, u16::MAX.into()) as u16;
        }
    }
}

fn update_tuners(cfg: &Config, changes: &[(String, Option<u16>)]) -> Result<Vec<(String, bool)>, String> {
    if changes.is_empty() {
        return Ok(Vec::new());
    }
    let ini = edcb_ini(cfg);
    let (mut lines, utf16) = read_ini(&ini)?;
    let mut done = Vec::new();
    for (name, want) in changes {
        if set_tuner(&mut lines, &module_file(name), *want) {
            done.push((name.clone(), want.is_some()));
        }
    }
    if !done.is_empty() {
        let out = join_lines(&lines);
        sync::write_atomic(&ini, if utf16 { utf16le(&String::from_utf8_lossy(&out)) } else { out })?;
    }
    Ok(done)
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
const TASK_PREFIX: &str = "BonDriverProxyRelay Relay ";

#[cfg(windows)]
fn register(name: &str, setting: &Path) -> Result<(), String> {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let args = format!("relay \"{}\"", setting.display());
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
    let file = std::env::temp_dir().join(format!("bondriverproxyrelay-relay-{name}.xml"));
    fs::write(&file, utf16le(&xml)).map_err(|e| e.to_string())?;
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
    format!("bondriverproxyrelay-relay-{name}.service")
}

#[cfg(not(windows))]
fn register(name: &str, setting: &Path) -> Result<(), String> {
    let text = format!(
        "[Unit]\nDescription=BonDriverProxyRelay relay {name}\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nExecStart=\"{}\" relay \"{}\"\nRestart=always\nRestartSec=10\n\n[Install]\nWantedBy=multi-user.target\n",
        exe()?,
        setting.display()
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
        .filter_map(|n| n.strip_prefix("bondriverproxyrelay-relay-")?.strip_suffix(".service").map(str::to_string))
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
    let existing = registered();
    let mut tuners: Vec<(String, Option<u16>)> = Vec::new();
    for peer in relays(cfg) {
        let name = peer.bdp_name.trim();
        if !valid_name(name) || peer.bdp_port == 0 || peer.bdp_group.trim().is_empty() {
            log.push(format!("{}: BonDriver の名前・ポート・グループ名を確認してください", peer.bdp_url));
            continue;
        }
        if names.iter().any(|n| n.eq_ignore_ascii_case(name)) || ports.contains(&peer.bdp_port) {
            log.push(format!("{name}: 名前かポート ({}) がほかの中継先と重複しています", peer.bdp_port));
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
        if peer.bdp_count > 0 || !existing.iter().any(|s| same(name, s)) {
            tuners.push((name.to_string(), Some(peer.bdp_count)));
        }
        match register(name, &setting_path(&module_path(cfg, name))) {
            Ok(()) => log.push(format!("{name}: 中継を登録しました (127.0.0.1:{} → {})", peer.bdp_port, peer.bdp_url)),
            Err(e) => log.push(format!("{name}: 中継の登録に失敗しました: {e}")),
        }
    }
    for stale in existing.into_iter().filter(|s| !names.iter().any(|n| same(n, s))) {
        match unregister(&stale) {
            Ok(()) => log.push(format!("{stale}: 中継の登録を外しました")),
            Err(e) => log.push(format!("{stale}: 中継の削除に失敗しました: {e}")),
        }
        let module = module_path(cfg, &stale);
        for file in [setting_path(&module), module] {
            if let Err(e) = fs::remove_file(&file).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) {
                log.push(format!("{}: {e}", file.display()));
            }
        }
        tuners.push((stale, None));
    }
    match update_tuners(cfg, &tuners) {
        Ok(done) => log.extend(done.into_iter().map(|(name, on)| {
            format!("{name}: EpgTimerSrv.ini のチューナー・視聴用の登録を{}しました (EpgTimer Service の再起動後に反映されます)", if on { "追加" } else { "解除" })
        })),
        Err(e) => log.push(format!("EpgTimerSrv.ini の更新に失敗しました: {e}")),
    }
    if log.is_empty() {
        log.push("BonDriver の設定がある中継先はありません".into());
    }
    log
}

fn relay_once(setting: &str) -> Result<std::process::ExitStatus, String> {
    let lines = split_lines(&fs::read(setting).map_err(|e| format!("{setting}: {e}"))?);
    let get = |key: &str| ini_value(&lines, key).filter(|v| !v.is_empty()).ok_or(format!("{setting}: missing {key}"));
    let (url, port, token, cloudflared) = (get("RELAY_URL")?, get("PORT")?, get("RELAY_TOKEN_FILE")?, get("RELAY_CLOUDFLARED")?);
    let (id, secret) = sync::read_token(&token).map_err(|e| format!("{token}: {e}"))?;
    Command::new(&cloudflared)
        .args(["access", "tcp", "--hostname", &url, "--url", &format!("127.0.0.1:{port}"), "--loglevel", "warn"])
        .env("TUNNEL_SERVICE_TOKEN_ID", id)
        .env("TUNNEL_SERVICE_TOKEN_SECRET", secret)
        .status()
        .map_err(|e| format!("{cloudflared}: {e}"))
}

pub fn relay(setting: &str) -> Result<(), String> {
    loop {
        match relay_once(setting) {
            Ok(status) => eprintln!("{setting}: cloudflared exited: {status}"),
            Err(e) => eprintln!("{e}"),
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn temp_file(ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bondriverproxyrelay-apply.{ext}"))
}

pub fn apply_elevated(cfg: &Config) -> Result<Vec<String>, String> {
    let (input, log) = (temp_file("json"), temp_file("log"));
    let _ = fs::remove_file(&log);
    fs::write(&input, serde_json::to_string(cfg).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", input.display()))?;
    let exe = exe()?;
    let arg = input.display().to_string();
    let ok = if cfg!(windows) {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let args = format!("@('apply', {})", quote(&format!("\"{arg}\"")));
        let script = format!("Start-Process -FilePath {} -ArgumentList {args} -Verb RunAs -Wait -WindowStyle Hidden", quote(&exe));
        Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).status()
    } else {
        Command::new("pkexec").args([exe.as_str(), "apply", &arg]).status()
    }
    .map_err(|e| e.to_string());
    let _ = fs::remove_file(&input);
    let ok = ok?.success();
    match fs::read_to_string(&log) {
        Ok(text) => Ok(text.lines().map(str::to_string).collect()),
        Err(_) if !ok => Err("管理者権限での実行が取り消されたか、失敗しました".into()),
        Err(e) => Err(format!("{}: {e}", log.display())),
    }
}

pub fn apply_cli(input: &str) -> Result<(), String> {
    let text = fs::read_to_string(input).map_err(|e| format!("{input}: {e}"))?;
    let cfg: Config = serde_json::from_str(&text).map_err(|e| format!("{input}: {e}"))?;
    let lines = apply(&cfg);
    let _ = fs::write(temp_file("log"), lines.join("\n"));
    lines.iter().for_each(|l| println!("{l}"));
    Ok(())
}
