import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Cable, Download, Monitor, Moon, Plus, RefreshCw, Save, Sun, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

type Peer = { url: string; token_file: string; bdp_url: string; bdp_port: number; bdp_group: string; bdp_name: string; bdp_count: number };
type Config = {
  epg_dir: string;
  listen: string;
  own: string[];
  own_invert: boolean;
  peers: Peer[];
  reload_addr: string;
  bondriver_dir: string;
  cloudflared: string;
  edcb_ini: string;
};

function Field({ id, label, value, onChange, placeholder }: { id: string; label: string; value: string; onChange: (v: string) => void; placeholder?: string }) {
  return (
    <div className="grid gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input id={id} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}

function LogView({ lines }: { lines?: string[] }) {
  return lines?.length ? <pre className="rounded-md bg-muted p-3 text-xs whitespace-pre-wrap">{lines.join("\n")}</pre> : null;
}

type Cmd = "run_pull" | "apply_bondriver";

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
  const [ownText, setOwnText] = useState("");
  const [path, setPath] = useState("");
  const [ownFiles, setOwnFiles] = useState<string[] | null>(null);
  const [status, setStatus] = useState("");
  const [running, setRunning] = useState<Partial<Record<Cmd, boolean>>>({});
  const [logs, setLogs] = useState<Partial<Record<Cmd, string[]>>>({});

  useEffect(() => {
    invoke<string>("config_path").then(setPath);
    invoke<Config>("get_config")
      .then((c) => {
        setCfg(c);
        setOwnText(c.own.join(" "));
      })
      .catch((e) => setStatus(String(e)));
  }, []);

  if (!cfg) return <div className="p-6 text-sm text-muted-foreground">{status || "読み込み中…"}</div>;

  const set = (patch: Partial<Config>) => setCfg({ ...cfg, ...patch });
  const setPeer = (i: number, patch: Partial<Peer>) => set({ peers: cfg.peers.map((p, j) => (j === i ? { ...p, ...patch } : p)) });
  const current = (): Config => ({ ...cfg, own: ownText.split(/[\s,]+/).filter(Boolean) });

  const save = async () => {
    try {
      await invoke("save_config", { cfg: current() });
      setStatus("保存しました");
    } catch (e) {
      setStatus(`保存に失敗しました: ${e}`);
    }
  };

  const preview = async () => {
    try {
      setOwnFiles(await invoke<string[]>("list_own", { cfg: current() }));
    } catch (e) {
      setOwnFiles(null);
      setStatus(String(e));
    }
  };

  const run = async (cmd: Cmd) => {
    setRunning((r) => ({ ...r, [cmd]: true }));
    setLogs((l) => ({ ...l, [cmd]: [] }));
    let out: string[];
    try {
      await invoke("save_config", { cfg: current() });
      out = await invoke<string[]>(cmd);
    } catch (e) {
      out = [String(e)];
    }
    setLogs((l) => ({ ...l, [cmd]: out }));
    setRunning((r) => ({ ...r, [cmd]: false }));
  };

  return (
    <main className="mx-auto flex max-w-3xl flex-col gap-6 p-6">
      <header className="flex items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">EpgSync</h1>
          <p className="text-sm text-muted-foreground break-all">{path}</p>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted-foreground">{status}</span>
          <ThemeToggle />
          <Button onClick={save}>
            <Save />
            保存
          </Button>
        </div>
      </header>

      <Card>
        <CardHeader>
          <CardTitle>基本設定</CardTitle>
          <CardDescription>serve と pull が共通で使う設定です。</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          <Field id="epg_dir" label="EPG フォルダー" value={cfg.epg_dir} onChange={(v) => set({ epg_dir: v })} />
          <div className="grid grid-cols-2 gap-4">
            <Field id="listen" label="serve の待ち受けアドレス" value={cfg.listen} onChange={(v) => set({ listen: v })} />
            <Field id="reload_addr" label="EpgTimerSrv のアドレス" value={cfg.reload_addr} onChange={(v) => set({ reload_addr: v })} />
          </div>
          <Field id="bondriver_dir" label="BonDriver フォルダー" value={cfg.bondriver_dir} onChange={(v) => set({ bondriver_dir: v })} />
          <Field id="cloudflared" label="cloudflared のパス" value={cfg.cloudflared} onChange={(v) => set({ cloudflared: v })} />
          <Field id="edcb_ini" label="EpgTimerSrv.ini のパス" value={cfg.edcb_ini} placeholder="空欄なら自動で探します" onChange={(v) => set({ edcb_ini: v })} />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>担当</CardTitle>
          <CardDescription>ファイル名の先頭一致で判定します (空白区切り)。例: 0004FFFF 7E277E27 7EA</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          <Input value={ownText} onChange={(e) => setOwnText(e.target.value)} placeholder="0004FFFF 0006FFFF 7E277E27" />
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Switch id="own_invert" checked={cfg.own_invert} onCheckedChange={(v) => set({ own_invert: v })} />
              <Label htmlFor="own_invert">列挙したもの以外を担当する</Label>
            </div>
            <Button variant="outline" size="sm" onClick={preview}>
              <RefreshCw />
              担当ファイルを確認
            </Button>
          </div>
          {ownFiles && (
            <div className="flex flex-wrap gap-1.5">
              {ownFiles.length ? ownFiles.map((f) => <Badge key={f} variant="secondary" className="font-mono">{f}</Badge>) : <span className="text-sm text-muted-foreground">該当なし</span>}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>取得先</CardTitle>
          <CardDescription>トークンファイルは TUNNEL_SERVICE_TOKEN_ID / TUNNEL_SERVICE_TOKEN_SECRET の 2 行です。</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3">
          {cfg.peers.map((p, i) => (
            <div key={i} className="grid gap-2 rounded-lg border p-3">
              <div className="grid grid-cols-[1fr_1fr_auto] gap-2">
                <Input value={p.url} placeholder="https://example.com/epg/" onChange={(e) => setPeer(i, { url: e.target.value })} />
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
          <Button variant="outline" size="sm" className="w-fit" onClick={() => set({ peers: [...cfg.peers, { url: "", token_file: "", bdp_url: "", bdp_port: 0, bdp_group: "", bdp_name: "", bdp_count: 0 }] })}>
            <Plus />
            取得先を追加
          </Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>BonDriver</CardTitle>
          <CardDescription>チューナー欄がある取得先ごとに BonDriver_Proxy_名前 を作り、cloudflared の中継を常駐登録します (管理者権限)。</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3">
          <Button className="w-fit" disabled={running.apply_bondriver} onClick={() => run("apply_bondriver")}>
            <Cable />
            {running.apply_bondriver ? "適用中…" : "BonDriver を適用"}
          </Button>
          <LogView lines={logs.apply_bondriver} />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>今すぐ取得</CardTitle>
          <CardDescription>設定を保存してから pull を 1 回実行します。</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3">
          <Button className="w-fit" disabled={running.run_pull} onClick={() => run("run_pull")}>
            <Download />
            {running.run_pull ? "取得中…" : "pull を実行"}
          </Button>
          <LogView lines={logs.run_pull} />
        </CardContent>
      </Card>
    </main>
  );
}
