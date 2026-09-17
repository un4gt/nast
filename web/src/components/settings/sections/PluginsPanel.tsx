import { useEffect, useState } from 'react';
import { Puzzle, RefreshCw } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Spinner } from '@/components/ui/spinner';
import { rpc } from '@/rpc';
import { pushToast } from '@/toasts';

interface PluginInfo {
  name: string;
  hooks: string[];
  commands: string[];
}

/** 服务端 Lua 插件管理（plugins/ 目录）。 */
export function PluginsPanel() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = () => {
    rpc.call<{ plugins: PluginInfo[] }>('plugins.list', {})
      .then((r) => setPlugins(r.plugins ?? []))
      .catch(() => {});
  };

  useEffect(refresh, []);

  const reload = async () => {
    setBusy(true);
    try {
      const r = await rpc.call<{ loaded: string[] }>('plugins.reload', {});
      pushToast(`已重载 ${r.loaded.length} 个插件`, 'success');
      refresh();
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="mb-1 text-sm font-semibold">插件（服务端 Lua）</h3>
        <p className="text-xs text-muted-foreground">
          plugins/*.lua 在服务端专用线程运行（互不阻塞生成），API：
          nast.on / register_command / get_var / set_var / toast / json_decode / json_encode。
          斜杠命令在输入框直接使用。
        </p>
      </div>

      <div className="flex items-center justify-between">
        <Label>已加载（{plugins.length}）</Label>
        <Button size="sm" variant="secondary" onClick={() => void reload()} disabled={busy}>
          {busy ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />}
          重载
        </Button>
      </div>

      <div className="flex flex-col gap-2">
        {plugins.map((p) => (
          <div key={p.name} className="flex flex-col gap-1.5 rounded-md border p-2">
            <div className="flex items-center gap-1.5">
              <Puzzle className="size-3.5 text-primary" />
              <span className="text-sm font-medium">{p.name}</span>
            </div>
            {p.hooks.length > 0 && (
              <div className="flex flex-wrap gap-1">
                {p.hooks.map((h) => (
                  <Badge key={h} variant="secondary" className="px-1.5 text-[10px]">{h}</Badge>
                ))}
              </div>
            )}
            {p.commands.length > 0 && (
              <div className="flex flex-wrap gap-1">
                {p.commands.map((c) => (
                  <Badge key={c} variant="outline" className="px-1.5 font-mono text-[10px]">
                    /{c}
                  </Badge>
                ))}
              </div>
            )}
          </div>
        ))}
        {plugins.length === 0 && (
          <p className="rounded-md border border-dashed p-4 text-center text-xs text-muted-foreground">
            plugins/ 目录暂无 .lua 插件；放入文件后点「重载」。
          </p>
        )}
      </div>
    </div>
  );
}
