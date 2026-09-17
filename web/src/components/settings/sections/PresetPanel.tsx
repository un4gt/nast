import { useEffect, useState } from 'react';
import { FolderOpen, Save, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Spinner } from '@/components/ui/spinner';
import { rpc } from '@/rpc';
import { pushToast } from '@/toasts';

/**
 * 预设管理（data/<user>/OpenAI Settings/*.json，ST 同构）：
 * 应用 = 预设字段合并进 oai_settings 草稿（用户再点设置保存）。
 */
export function PresetPanel({
  oai,
  applyToOai,
}: {
  oai: any;
  applyToOai: (merged: Record<string, unknown>) => void;
}) {
  const [presets, setPresets] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [saveOpen, setSaveOpen] = useState(false);
  const [saveName, setSaveName] = useState('');

  useEffect(() => {
    rpc.call<string[]>('presets.list', {}).then(setPresets).catch(() => {});
  }, []);

  const apply = async (name: string) => {
    setBusy(true);
    try {
      const preset = await rpc.call<Record<string, unknown>>('presets.get', { name });
      // Object.assign(oai_settings, preset) 语义
      applyToOai({ ...oai, ...preset });
      pushToast(`已应用预设「${name}」（记得保存设置）`, 'success');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
    }
  };

  const doSave = async () => {
    if (!saveName.trim()) return;
    setBusy(true);
    try {
      await rpc.call('presets.save', { name: saveName.trim(), preset: oai });
      const list = await rpc.call<string[]>('presets.list', {});
      setPresets(list);
      pushToast('预设已保存', 'success');
      setSaveOpen(false);
      setSaveName('');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
    }
  };

  const doDelete = async (name: string) => {
    setBusy(true);
    try {
      await rpc.call('presets.delete', { name });
      setPresets((p) => p.filter((x) => x !== name));
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="mb-1 text-sm font-semibold">预设（Presets）</h3>
        <p className="text-xs text-muted-foreground">
          Chat Completion 预设（data/&lt;user&gt;/OpenAI Settings/*.json，与 ST 互通）。
          应用 = 合并进 oai_settings 草稿。
        </p>
      </div>

      {busy && <Spinner className="size-4" />}

      <div className="flex items-center justify-between">
        <Label>预设列表</Label>
        <Button size="sm" variant="secondary" onClick={() => setSaveOpen(true)}>
          <Save className="size-3.5" />
          另存当前配置为预设
        </Button>
      </div>
      <ScrollArea className="max-h-56 rounded-md border p-1">
        <div className="flex flex-col gap-1">
          {presets.map((name) => (
            <div key={name} className="flex items-center gap-2 rounded px-2 py-1 hover:bg-accent/50">
              <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />
              <button
                className="min-w-0 flex-1 truncate text-left text-xs"
                onClick={() => void apply(name)}
                title="应用此预设"
              >
                {name}
              </button>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 px-2 text-[10px]"
                onClick={() => void apply(name)}
                disabled={busy}
              >
                应用
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-6 text-muted-foreground hover:text-destructive"
                onClick={() => void doDelete(name)}
                title="删除预设"
              >
                <Trash2 className="size-3" />
              </Button>
            </div>
          ))}
          {presets.length === 0 && (
            <p className="px-2 py-3 text-center text-xs text-muted-foreground">
              暂无预设；调好参数后「另存为预设」。
            </p>
          )}
        </div>
      </ScrollArea>

      <Dialog open={saveOpen} onOpenChange={setSaveOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>另存为预设</DialogTitle>
            <DialogDescription>将当前 oai_settings 草稿整体存为预设文件。</DialogDescription>
          </DialogHeader>
          <Input
            value={saveName}
            onChange={(e) => setSaveName(e.target.value)}
            placeholder="预设名称"
            onKeyDown={(e) => {
              if (e.key === 'Enter') void doSave();
            }}
          />
          <DialogFooter>
            <Button variant="ghost" onClick={() => setSaveOpen(false)}>取消</Button>
            <Button onClick={() => void doSave()} disabled={busy || !saveName.trim()}>
              {busy ? <Spinner /> : null}
              保存
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
