import { rpc } from '../../../rpc';
import { useStore } from '../../../store';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { useEffect, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { pushToast } from '../../../toasts';

export function AboutPanel() {
  const { connected } = useStore();
  const [plugins, setPlugins] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const r = await rpc.call<{ lua: string[] }>('plugins.list', {});
      setPlugins(r.lua);
    } catch {
      setPlugins([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">关于</h3>
        <p className="text-xs text-muted-foreground">nast — Not A SillyTavern</p>
      </div>

      <div className="flex flex-col gap-1 text-xs text-muted-foreground">
        <span>连接状态：{connected ? '已连接' : '未连接'}</span>
        <span>行为基准：SillyTavern release 1.18.0</span>
        <span>数据兼容：角色卡 PNG（chara/ccv3）· 世界书 · 聊天 jsonl · 设置字段同构</span>
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium">插件（plugins/）</span>
          <Button variant="ghost" size="icon" className="size-6" onClick={() => void load()} title="重新加载">
            {loading ? <Spinner /> : <RefreshCw />}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-xs"
            onClick={async () => {
              try {
                const r = await rpc.call<{ loaded: string[] }>('plugins.reload', {});
                setPlugins(r.loaded);
                pushToast(`已重载 ${r.loaded.length} 个插件`, 'success');
              } catch (e) {
                pushToast(e instanceof Error ? e.message : String(e), 'error');
              }
            }}
          >
            重载
          </Button>
        </div>
        {plugins.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {plugins.map((p) => (
              <span key={p} className="rounded bg-secondary px-1.5 py-0.5 text-[10px]">
                {p}
              </span>
            ))}
          </div>
        ) : (
          <p className="text-xs text-muted-foreground">无已加载插件（plugins/*.lua）</p>
        )}
      </div>
    </div>
  );
}
