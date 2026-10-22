import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Cable, Monitor, Moon, Plus, Sun, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type Peer = { token_file: string; bdp_url: string; bdp_port: number; bdp_group: string; bdp_name: string; bdp_count: number };
type Config = {
  peers: Peer[];
  bondriver_dir: string;
  cloudflared: string;
  edcb_ini: string;
};

function Field({ id, label, value, onChange, placeholder, onFound }: { id: string; label: string; value: string; onChange: (v: string) => void; placeholder?: string; onFound?: (found: boolean | null) => void }) {
  const [found, setFound] = useState<boolean | null>(null);
  const check = !!onFound;

  useEffect(() => {
    onFound?.(found);
  }, [found]);

  useEffect(() => {
    if (!check) return;
    let live = true;
    const timer = setTimeout(() => {
      invoke<boolean>("path_exists", { path: value }).then(
        (v) => live && setFound(v),
        () => live && setFound(null),
      );
    }, 300);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [check, value]);

  return (
    <div className="grid gap-2">
      <div className="flex items-center gap-2">
        <Label htmlFor={id}>{label}</Label>
        {check && found !== null && <Badge variant={found ? "default" : "destructive"} className={found ? "bg-green-800 text-white" : undefined}>{found ? "検出" : "未検出"}</Badge>}
      </div>
      <Input id={id} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}

function LogView({ lines }: { lines?: string[] }) {
  return lines?.length ? <pre className="rounded-md bg-muted p-3 text-xs whitespace-pre-wrap">{lines.join("\n")}</pre> : null;
}

type Cmd = "apply_bondriver";

type Theme = "light" | "dark" | "system";

function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem("theme") as Theme) || "system");

  useEffect(() => {
    localStorage.setItem("theme", theme);
    getCurrentWindow().setTheme(theme === "system" ? null : theme);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => document.documentElement.classList.toggle("dark", theme === "dark" || (theme === "system" && mq.matches));
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  const items: [Theme, typeof Sun, string][] = [
    ["light", Sun, "ライト"],
    ["dark", Moon, "ダーク"],
    ["system", Monitor, "システム"],
  ];
  return (
    <div className="flex rounded-md border p-0.5">
      {items.map(([value, Icon, label]) => (
        <Button key={value} variant={theme === value ? "secondary" : "ghost"} size="icon-sm" aria-label={label} title={label} onClick={() => setTheme(value)}>
          <Icon />
        </Button>
      ))}
    </div>
  );
}

export default function App() {
  const [cfg, setCfg] = useState<Config | null>(null);
  const [status, setStatus] = useState("");
  const [running, setRunning] = useState<Partial<Record<Cmd, boolean>>>({});
  const [logs, setLogs] = useState<Partial<Record<Cmd, string[]>>>({});
  const [missing, setMissing] = useState<Record<string, boolean>>({});
  const report = (id: string) => (found: boolean | null) => setMissing((m) => ({ ...m, [id]: found === false }));
  const blocked = Object.values(missing).some(Boolean);

  useEffect(() => {
    invoke<Config>("get_config")
      .then(setCfg)
      .catch((e) => setStatus(String(e)));
  }, []);

  if (!cfg) return <div className="p-6 text-sm text-muted-foreground">{status || "読み込み中…"}</div>;

  const set = (patch: Partial<Config>) => setCfg({ ...cfg, ...patch });
  const setPeer = (i: number, patch: Partial<Peer>) => set({ peers: cfg.peers.map((p, j) => (j === i ? { ...p, ...patch } : p)) });

  const run = async (cmd: Cmd) => {
    setRunning((r) => ({ ...r, [cmd]: true }));
    setLogs((l) => ({ ...l, [cmd]: [] }));
    let out: string[];
    try {
      out = await invoke<string[]>(cmd, { cfg });
      setCfg(await invoke<Config>("get_config"));
    } catch (e) {
      out = [String(e)];
    }
    setLogs((l) => ({ ...l, [cmd]: out }));
    setRunning((r) => ({ ...r, [cmd]: false }));
  };

  return (
    <main className="mx-auto flex max-w-3xl flex-col gap-6 p-6">
      <header className="flex items-end justify-between gap-4">
        <h1 className="text-2xl font-semibold">BonDriverProxyRelay</h1>
        <ThemeToggle />
      </header>

      <Card>
        <CardHeader>
          <CardTitle>基本設定</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <Field id="bondriver_dir" label="BonDriver フォルダー" value={cfg.bondriver_dir} onFound={report("bondriver_dir")} onChange={(v) => set({ bondriver_dir: v })} />
          <Field id="cloudflared" label="cloudflared のパス" value={cfg.cloudflared} onFound={report("cloudflared")} onChange={(v) => set({ cloudflared: v })} />
          <Field id="edcb_ini" label="EpgTimerSrv.ini のパス" value={cfg.edcb_ini} onFound={report("edcb_ini")} placeholder="空欄なら BonDriver フォルダーの 1 つ上を使います" onChange={(v) => set({ edcb_ini: v })} />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>中継先</CardTitle>
          <CardDescription>トークンファイルは TUNNEL_SERVICE_TOKEN_ID / TUNNEL_SERVICE_TOKEN_SECRET の 2 行です。</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3">
          {cfg.peers.map((p, i) => (
            <div key={i} className="grid gap-2 rounded-lg border p-3">
              <div className="grid grid-cols-[1fr_auto] gap-2">
                <Input value={p.token_file} placeholder="トークンファイルのパス" onChange={(e) => setPeer(i, { token_file: e.target.value })} />
                <Button variant="ghost" size="icon" aria-label="削除" onClick={() => set({ peers: cfg.peers.filter((_, j) => j !== i) })}>
                  <Trash2 />
                </Button>
              </div>
              <div className="grid grid-cols-[2fr_1fr_1fr_1fr_1fr] gap-2">
                <Input value={p.bdp_url} placeholder="チューナー: https://example.com/bdp" onChange={(e) => setPeer(i, { bdp_url: e.target.value })} />
                <Input type="number" value={p.bdp_port || ""} placeholder="ポート" onChange={(e) => setPeer(i, { bdp_port: Number(e.target.value) || 0 })} />
                <Input value={p.bdp_group} placeholder="グループ名" onChange={(e) => setPeer(i, { bdp_group: e.target.value })} />
                <Input value={p.bdp_name} placeholder="名前 (Z390)" onChange={(e) => setPeer(i, { bdp_name: e.target.value })} />
                <Input type="number" value={p.bdp_count || ""} placeholder="チューナー数" onChange={(e) => setPeer(i, { bdp_count: Number(e.target.value) || 0 })} />
              </div>
            </div>
          ))}
          <Button variant="outline" size="sm" className="w-fit" onClick={() => set({ peers: [...cfg.peers, { token_file: "", bdp_url: "", bdp_port: 0, bdp_group: "", bdp_name: "", bdp_count: 0 }] })}>
            <Plus />
            中継先を追加
          </Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>BonDriver</CardTitle>
          <CardDescription>チューナー欄がある中継先ごとに BonDriver_Proxy_名前 を作り、cloudflared の中継を常駐登録します (管理者権限)。</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3">
          <Button className="w-fit" disabled={running.apply_bondriver || blocked} onClick={() => run("apply_bondriver")}>
            <Cable />
            {running.apply_bondriver ? "適用中…" : "BonDriver を適用"}
          </Button>
          <LogView lines={logs.apply_bondriver} />
        </CardContent>
      </Card>
    </main>
  );
}
