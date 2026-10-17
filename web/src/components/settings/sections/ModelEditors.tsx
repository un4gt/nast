import { useState } from 'react';
import { ArrowDown, ArrowUp, Trash2 } from 'lucide-react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import { channelRoute, endpoints, protocols, validateConnection, type PublishCatalog } from '@/lib/model-config';
import type { Catalog, Channel, LogicalModel, Route } from '@/models';
import { rpc } from '@/rpc';
import { useStore } from '@/store';
import { pushToast } from '@/toasts';
import { ModelParameters } from './ModelParameters';

export function LogicalModelEditor({ model, catalog, publish, onClose }: { model: LogicalModel; catalog: Catalog; publish: PublishCatalog; onClose: () => void }) {
  const [draft, setDraft] = useState(() => ({ ...structuredClone(model), routes: structuredClone(model.routes).sort((a, b) => b.priority - a.priority) }));
  const [selected, setSelected] = useState(draft.routes[0]?.id ?? '');
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [version, setVersion] = useState(catalog.version);
  const output = useStore(s => s.settings?.oai_settings?.openai_max_tokens ?? 300);
  const route = draft.routes.find(r => r.id === selected);
  const patch = (change: (r: Route) => void) => setDraft(current => { const next = structuredClone(current); const r = next.routes.find(r => r.id === selected); if (r) change(r); return next; });
  const move = (index: number, offset: number) => setDraft(current => { const next = structuredClone(current); [next.routes[index], next.routes[index + offset]] = [next.routes[index + offset], next.routes[index]]; return next; });
  const add = (channelId: string) => {
    const c = catalog.channels.find(c => c.id === channelId); if (!c) return;
    const next = channelRoute(c); setDraft(d => ({ ...d, routes: [...d.routes, next] })); setSelected(next.id);
  };
  const save = async () => {
    if (saving) return;
    setError('');
    if (!draft.display_name.trim()) { setError('给这个模型起一个名称。'); return; }
    const normalized = structuredClone(draft);
    normalized.display_name = normalized.display_name.trim();
    for (const [index, r] of normalized.routes.entries()) {
      const c = catalog.channels.find(c => c.id === r.channel_id);
      if (!c) { setError('关联的渠道已被删除，请移除该关联后重新选择。'); return; }
      r.protocol = c.protocol; r.config.endpoint = c.endpoint; r.upstream_model = r.upstream_model.trim(); r.priority = normalized.routes.length - index;
      const invalid = validateConnection(r, output); if (invalid) { setSelected(r.id); setError(invalid); return; }
    }
    setSaving(true);
    try {
      const exists = catalog.models.some(m => m.id === normalized.id);
      await publish({ ...catalog, version, default_model: catalog.models.length ? catalog.default_model : normalized.id, models: exists ? catalog.models.map(m => m.id === normalized.id ? normalized : m) : [...catalog.models, normalized] });
      pushToast('模型已保存', 'success'); onClose();
    } catch (e) { setError(`保存失败，草稿已保留：${String(e)}`); } finally { setSaving(false); }
  };
  return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}><DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl" onInteractOutside={e => e.preventDefault()} onEscapeKeyDown={e => { if (saving) e.preventDefault(); }}>
    <DialogHeader><DialogTitle>{catalog.models.some(m => m.id === model.id) ? '配置模型' : '添加模型'}</DialogTitle><DialogDescription>为同一个模型连接多个渠道。首次按顺序选择，后续沿用，故障自动切换。</DialogDescription></DialogHeader>
    <form onSubmit={e => { e.preventDefault(); void save(); }} className="flex flex-col gap-5"><fieldset disabled={saving} className="min-w-0"><FieldGroup>
      <Field><FieldLabel htmlFor="logical-name">模型名称</FieldLabel><Input id="logical-name" value={draft.display_name} onChange={e => setDraft({ ...draft, display_name: e.target.value })} placeholder="例如 Claude Sonnet" required /></Field>
      <Field><FieldLabel>关联渠道</FieldLabel><FieldDescription>从上到下为首次选择和故障切换的优先顺序。</FieldDescription>
        <div className="flex flex-col gap-2">{draft.routes.map((r, index) => <div key={r.id} className="flex min-w-0 items-center gap-1 rounded-lg border p-2">
          <Button type="button" variant={selected === r.id ? 'secondary' : 'ghost'} className="min-w-0 flex-1 justify-start" onClick={() => setSelected(r.id)}><span className="mr-1 tabular-nums">{index + 1}</span><span className="truncate">{catalog.channels.find(c => c.id === r.channel_id)?.name ?? '已删除渠道'}</span>{!r.enabled && <span>· 停用</span>}</Button>
          <Button type="button" size="icon-sm" variant="ghost" aria-label={`上移渠道 ${index + 1}`} disabled={!index} onClick={() => move(index, -1)}><ArrowUp /></Button>
          <Button type="button" size="icon-sm" variant="ghost" aria-label={`下移渠道 ${index + 1}`} disabled={index === draft.routes.length - 1} onClick={() => move(index, 1)}><ArrowDown /></Button>
          <Button type="button" size="icon-sm" variant="ghost" aria-label={`移除渠道 ${index + 1}`} onClick={() => { setDraft(d => ({ ...d, routes: d.routes.filter(x => x.id !== r.id) })); if (selected === r.id) setSelected(draft.routes.find(x => x.id !== r.id)?.id ?? ''); }}><Trash2 /></Button>
        </div>)}</div>
        <Select value="" onValueChange={add}><SelectTrigger aria-label="关联一个渠道" disabled={!catalog.channels.length}><SelectValue placeholder={catalog.channels.length ? '＋ 关联一个渠道' : '请先在渠道页添加连接'} /></SelectTrigger><SelectContent><SelectGroup>{catalog.channels.map(c => <SelectItem key={c.id} value={c.id}>{c.name}</SelectItem>)}</SelectGroup></SelectContent></Select>
      </Field>
      {route && <FieldGroup className="rounded-xl border bg-muted/20 p-4">
        <Field><FieldLabel htmlFor="route-upstream">这个渠道的模型 ID</FieldLabel><Input id="route-upstream" value={route.upstream_model} onChange={e => patch(r => { r.upstream_model = e.target.value; })} placeholder="服务商提供的实际模型 ID" required list="channel-model-ids" /><datalist id="channel-model-ids">{catalog.channels.find(c => c.id === route.channel_id)?.discovered_models.map(id => <option key={id} value={id} />)}</datalist></Field>
        <div className="grid gap-4 sm:grid-cols-2"><Field><FieldLabel htmlFor="route-images">图片输入</FieldLabel><Select value={route.config.supports_images == null ? 'unknown' : String(route.config.supports_images)} onValueChange={value => patch(r => { r.config.supports_images = value === 'unknown' ? null : value === 'true'; })}><SelectTrigger id="route-images"><SelectValue /></SelectTrigger><SelectContent><SelectGroup><SelectItem value="unknown">未知</SelectItem><SelectItem value="true">支持</SelectItem><SelectItem value="false">不支持</SelectItem></SelectGroup></SelectContent></Select></Field>
        <Field orientation="horizontal"><FieldLabel htmlFor="route-enabled">启用此关联</FieldLabel><Switch id="route-enabled" checked={route.enabled} onCheckedChange={value => patch(r => { r.enabled = value; })} /></Field></div>
        <ModelParameters route={route} patch={patch} presetOutput={output} />
      </FieldGroup>}
    </FieldGroup></fieldset>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription>{version !== catalog.version && <Button type="button" size="sm" variant="outline" onClick={() => { setVersion(catalog.version); setError('已载入最新目录，当前模型草稿保留。请检查后再次保存。'); }}>载入最新目录</Button>}</Alert>}
    <DialogFooter><Button type="button" variant="outline" disabled={saving} onClick={onClose}>取消</Button><Button disabled={saving} type="submit">{saving ? '正在保存…' : '保存模型'}</Button></DialogFooter></form>
  </DialogContent></Dialog>;
}

export function ChannelEditor({ channel, catalog, publish, onClose }: { channel: Channel; catalog: Catalog; publish: PublishCatalog; onClose: () => void }) {
  const [draft, setDraft] = useState(structuredClone(channel));
  const [key, setKey] = useState<string>();
  const [headers, setHeaders] = useState(JSON.stringify(channel.headers, null, 2));
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [busy, setBusy] = useState(false);
  const [version, setVersion] = useState(catalog.version);
  const validate = (): Channel => {
    const parsed = JSON.parse(headers || '{}');
    if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object' || Object.values(parsed).some(v => typeof v !== 'string')) throw new Error('公共请求头须为 JSON 字符串键值对象');
    const c = { ...draft, name: draft.name.trim(), endpoint: draft.endpoint.trim().replace(/\/+$/, ''), headers: parsed };
    const invalid = validateConnection(channelRoute(c, 'test'), 1); if (invalid) throw new Error(invalid);
    if (!c.name) throw new Error('请填写渠道名称');
    if (channel.credential_configured && key === undefined && (c.endpoint !== channel.endpoint.replace(/\/+$/, '') || c.protocol !== channel.protocol)) throw new Error('地址或协议已更改，请重新填写密钥，或清除旧密钥');
    return c;
  };
  const save = async () => {
    if (busy) return;
    try {
      const c = validate(); setBusy(true); setError('');
      await publish({ ...catalog, version, channels: catalog.channels.some(x => x.id === c.id) ? catalog.channels.map(x => x.id === c.id ? c : x) : [...catalog.channels, c] }, key === undefined ? {} : { [c.id]: key });
      pushToast('渠道已保存', 'success'); onClose();
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  const test = async () => {
    try {
      const c = validate(); setBusy(true); setError(''); setNotice('');
      const result = await rpc.call<{ data: string[] }>('models.list', { connection: { endpoint: c.endpoint, channel_id: c.id, key, headers: c.headers } });
      setNotice(`连接成功，发现 ${result.data.length} 个模型。保存后可在渠道页选择关联。`);
      setDraft(d => ({ ...d, discovered_models: result.data }));
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  return <Dialog open onOpenChange={open => { if (!open && !busy) onClose(); }}><DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-xl" onInteractOutside={e => e.preventDefault()} onEscapeKeyDown={e => { if (busy) e.preventDefault(); }}><DialogHeader><DialogTitle>{catalog.channels.some(c => c.id === channel.id) ? '编辑渠道' : '添加渠道'}</DialogTitle><DialogDescription>地址和密钥统一维护，所有关联模型共享这份连接。</DialogDescription></DialogHeader>
    <form onSubmit={e => { e.preventDefault(); void save(); }} className="flex flex-col gap-5"><fieldset disabled={busy}><FieldGroup>
      <Field><FieldLabel htmlFor="channel-name">渠道名称</FieldLabel><Input id="channel-name" placeholder="例如 官方 API、备用服务" required value={draft.name} onChange={e => setDraft({ ...draft, name: e.target.value })} /></Field>
      <Field><FieldLabel htmlFor="channel-protocol">API 协议</FieldLabel><Select value={draft.protocol} onValueChange={protocol => setDraft({ ...draft, protocol, endpoint: draft.endpoint === endpoints[draft.protocol] ? endpoints[protocol] : draft.endpoint })}><SelectTrigger id="channel-protocol"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{Object.entries(protocols).map(([id, name]) => <SelectItem key={id} value={id}>{name}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>
      <Field><FieldLabel htmlFor="channel-endpoint">API 地址</FieldLabel><Input id="channel-endpoint" type="url" required value={draft.endpoint} onChange={e => setDraft({ ...draft, endpoint: e.target.value })} /></Field>
      <Field><FieldLabel htmlFor="channel-key">API 密钥</FieldLabel><Input id="channel-key" type="password" autoComplete="new-password" value={key ?? ''} placeholder={channel.credential_configured ? '已保存，留空保持不变' : '本地免密服务可留空'} onChange={e => setKey(e.target.value || undefined)} />{channel.credential_configured && <Button type="button" size="sm" variant="ghost" className="self-start" onClick={() => setKey('')}>{key === '' ? '保存时将清除密钥' : '清除已保存的密钥'}</Button>}</Field>
      <details><summary className="cursor-pointer text-sm text-muted-foreground">高级连接设置</summary><Field className="mt-3"><FieldLabel htmlFor="channel-headers">公共请求头（JSON）</FieldLabel><Textarea id="channel-headers" value={headers} onChange={e => setHeaders(e.target.value)} /><FieldDescription>鉴权请使用上方密钥字段。</FieldDescription></Field></details>
      <Field orientation="horizontal"><FieldLabel htmlFor="channel-enabled">启用渠道</FieldLabel><Switch id="channel-enabled" checked={draft.enabled} onCheckedChange={enabled => setDraft({ ...draft, enabled })} /></Field>
    </FieldGroup></fieldset>
    {notice && <Alert><AlertDescription>{notice}</AlertDescription></Alert>}{error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription>{version !== catalog.version && <Button type="button" size="sm" variant="outline" onClick={() => { setVersion(catalog.version); setError('已载入最新目录，请检查当前草稿后再次保存。'); }}>载入最新目录</Button>}</Alert>}
    <DialogFooter className="gap-2">{draft.protocol === 'openai' && <Button type="button" variant="outline" disabled={busy} onClick={() => void test()}>测试连接</Button>}<Button type="button" variant="ghost" disabled={busy} onClick={onClose}>取消</Button><Button type="submit" disabled={busy}>{busy ? '处理中…' : '保存渠道'}</Button></DialogFooter></form>
  </DialogContent></Dialog>;
}
