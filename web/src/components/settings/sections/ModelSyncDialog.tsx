import { useEffect, useState } from 'react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { channelRoute, newModel, type PublishCatalog } from '@/lib/model-config';
import type { Catalog, Channel } from '@/models';
import { rpc } from '@/rpc';
import { pushToast } from '@/toasts';

export function ModelSyncDialog({ channel, catalog, publish, onClose }: { channel: Channel; catalog: Catalog; publish: PublishCatalog; onClose: () => void }) {
  const [models, setModels] = useState<string[]>([]);
  const [selected, setSelected] = useState<Record<string, string>>({});
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [refresh, setRefresh] = useState(0);
  const linked = new Set(catalog.models.flatMap(m => m.routes).filter(r => r.channel_id === channel.id).map(r => r.upstream_model));
  const choices = Object.entries(selected).filter(([id]) => !linked.has(id) && models.includes(id));
  const visible = models.filter(id => id.toLowerCase().includes(query.toLowerCase()));
  useEffect(() => {
    let alive = true; setLoading(true); setError('');
    void rpc.call<{ data: string[] }>('models.list', { channel_id: channel.id }).then(result => { if (alive) setModels([...new Set(result.data)].sort()); }).catch(e => { if (alive) setError(`获取失败：${String(e)}`); }).finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [channel.id, refresh]);
  const suggest = (id: string) => {
    const matches = catalog.models.filter(m => m.display_name === id || m.routes.some(r => r.upstream_model === id));
    return matches.length === 1 ? matches[0].id : '__new';
  };
  const save = async () => {
    if (saving) return;
    setSaving(true); setError('');
    try {
      const next = structuredClone(catalog); const c = next.channels.find(c => c.id === channel.id);
      if (!c) throw new Error('渠道已被删除，请重新打开');
      c.discovered_models = models;
      for (const [id, target] of choices) {
        let m = next.models.find(m => m.id === target);
        if (!m && target !== '__new') throw new Error('目标模型已被删除，请重新选择');
        if (!m) { m = newModel(); m.display_name = id; next.models.push(m); }
        const r = channelRoute(c, id); r.priority = m.routes.length ? Math.min(...m.routes.map(r => r.priority)) - 1 : 0;
        m.routes.push(r);
      }
      if (!next.default_model && next.models.length) next.default_model = next.models[0].id;
      await publish(next); pushToast(choices.length ? `已关联 ${choices.length} 个模型` : '渠道模型列表已更新', 'success'); onClose();
    } catch (e) { setError(`保存失败，选择已保留：${String(e)}`); } finally { setSaving(false); }
  };
  return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}><DialogContent className="flex max-h-[90dvh] flex-col overflow-hidden sm:max-w-2xl" onInteractOutside={e => e.preventDefault()} onEscapeKeyDown={e => { if (saving) e.preventDefault(); }}>
    <DialogHeader><DialogTitle>{channel.name} · 可用模型</DialogTitle><DialogDescription>加入已有模型的渠道，或创建一个新模型。已有参数和会话绑定保持不变。</DialogDescription></DialogHeader>
    <Input aria-label="搜索渠道模型" placeholder="搜索服务商的模型 ID…" value={query} onChange={e => setQuery(e.target.value)} disabled={saving} />
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription><Button size="sm" variant="outline" disabled={loading || saving} onClick={() => setRefresh(n => n + 1)}>重新获取</Button></Alert>}
    <div className="flex min-h-0 flex-col gap-3 overflow-y-auto py-2">
      {loading ? <p role="status">正在获取模型列表…</p> : <>
        <Button size="sm" variant="ghost" className="self-start" disabled={saving} onClick={() => setSelected(current => ({ ...current, ...Object.fromEntries(visible.filter(id => !linked.has(id)).map(id => [id, current[id] ?? suggest(id)])) }))}>选择搜索结果</Button>
        <FieldGroup>{visible.map((id, index) => <Field key={id} className="rounded-lg border p-3"><div className="flex items-start gap-3"><Checkbox id={`discovered-${index}`} disabled={saving || linked.has(id)} checked={linked.has(id) || id in selected} onCheckedChange={checked => setSelected(current => { const next = { ...current }; if (checked) next[id] = suggest(id); else delete next[id]; return next; })} /><FieldLabel htmlFor={`discovered-${index}`} className="min-w-0 flex-1 break-all">{id}</FieldLabel>{linked.has(id) && <Badge variant="secondary">已关联</Badge>}</div>
          {id in selected && !linked.has(id) && <Select disabled={saving} value={selected[id]} onValueChange={value => setSelected({ ...selected, [id]: value })}><SelectTrigger aria-label={`${id} 关联到`}><SelectValue /></SelectTrigger><SelectContent><SelectGroup><SelectItem value="__new">创建新模型</SelectItem>{catalog.models.map(m => <SelectItem key={m.id} value={m.id}>关联到 {m.display_name}</SelectItem>)}</SelectGroup></SelectContent></Select>}
        </Field>)}</FieldGroup>
        {!visible.length && <p className="py-8 text-center text-sm text-muted-foreground">{models.length ? '没有匹配结果' : '没有可获取的模型，可返回后手动关联模型 ID。'}</p>}
      </>}
    </div>
    <DialogFooter><Button variant="outline" disabled={saving} onClick={onClose}>取消</Button><Button disabled={saving || loading || !!error && !models.length} onClick={() => void save()}>{saving ? '正在保存…' : choices.length ? `关联 ${choices.length} 个模型` : '更新列表'}</Button></DialogFooter>
  </DialogContent></Dialog>;
}
