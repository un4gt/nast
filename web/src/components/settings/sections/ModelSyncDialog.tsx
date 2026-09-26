import { useEffect, useRef, useState } from 'react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Spinner } from '@/components/ui/spinner';
import { newModel, protocols, validateConnection } from '@/lib/model-config';
import type { Catalog, Route } from '@/models';
import { rpc } from '@/rpc';
import { ModelParameters } from './ModelParameters';

export type ModelSyncDraft = { route: Route; sourceRouteId?: string; key?: string; name?: string; makeDefault: boolean };
const endpoint = (value: string) => value.trim().replace(/\/+$/, '');

// Compose the shadcn checkbox group example with its existing Dialog and Field primitives.
export function ModelSyncDialog({ draft, catalog, presetOutput, publish, reload, onClose, onSaved }: {
  draft: ModelSyncDraft; catalog: Catalog; presetOutput: number;
  publish: (next: Catalog, credentials: Record<string, string>, copies: Record<string, string>) => Promise<boolean>;
  reload: () => Promise<boolean>; onClose: () => void; onSaved: (count: number) => void;
}) {
  const [route, setRoute] = useState(() => structuredClone(draft.route));
  const [models, setModels] = useState<string[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [query, setQuery] = useState('');
  const [name, setName] = useState(draft.name ?? '');
  const [defaultId, setDefaultId] = useState('');
  const [makeDefault, setMakeDefault] = useState(draft.makeDefault);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [listError, setListError] = useState('');
  const [refresh, setRefresh] = useState(0);
  const saveLock = useRef(false);
  const existing = new Set(catalog.models.flatMap(m => m.routes)
    .filter(r => r.protocol === route.protocol && endpoint(r.config.endpoint) === endpoint(route.config.endpoint))
    .map(r => r.upstream_model));
  const choices = selected.filter(id => !existing.has(id) && models.includes(id));
  const visible = models.filter(id => id.toLowerCase().includes(query.trim().toLowerCase()));
  const available = visible.filter(id => !existing.has(id));
  const allSelected = available.length > 0 && available.every(id => choices.includes(id));
  const defaultModel = choices.includes(defaultId) ? defaultId : choices[0];
  const patch = (change: (value: Route) => void) => setRoute(current => {
    const next = structuredClone(current); change(next); return next;
  });

  useEffect(() => {
    let cancelled = false;
    setLoading(true); setListError('');
    void rpc.call<{ data: string[] }>('models.list', {
      connection: { endpoint: endpoint(draft.route.config.endpoint), key: draft.key, route_id: draft.sourceRouteId },
    }).then(result => {
      if (cancelled) return;
      const ids = [...new Set(result.data.filter(id => typeof id === 'string' && id.trim()).map(id => id.trim()))].sort();
      setModels(ids); setSelected(current => current.filter(id => ids.includes(id)));
    }).catch(e => { if (!cancelled) setListError(`获取失败：${String(e)}`); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [draft, refresh]);

  const save = async () => {
    if (saveLock.current || loading || !choices.length) return;
    const invalid = validateConnection({ ...route, upstream_model: choices[0] }, presetOutput);
    if (invalid) { setError(invalid); return; }
    saveLock.current = true; setSaving(true); setError('');
    try {
      const next = structuredClone(catalog);
      const credentials: Record<string, string> = {};
      const copies: Record<string, string> = {};
      for (const id of choices) {
        const model = newModel();
        const added = { ...structuredClone(route), id: model.routes[0].id, upstream_model: id, enabled: true };
        added.config.endpoint = endpoint(added.config.endpoint);
        delete added.config.credential_ref;
        added.credential_configured = draft.key !== undefined ? !!draft.key.trim() : !!draft.route.credential_configured;
        model.routes = [added];
        model.display_name = choices.length === 1 ? name.trim() || id : id;
        next.models.push(model);
        if (makeDefault && id === defaultModel) next.default_model = model.id;
        if (draft.key !== undefined) credentials[added.id] = draft.key;
        else if (draft.sourceRouteId && draft.route.credential_configured) copies[added.id] = draft.sourceRouteId;
      }
      if (await publish(next, credentials, copies)) onSaved(choices.length);
    } catch (e) { setError(`保存失败，选择已保留：${String(e)}`); }
    finally { saveLock.current = false; setSaving(false); }
  };

  return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
    <DialogContent className="flex flex-col overflow-hidden sm:max-w-xl" onInteractOutside={event => event.preventDefault()} onEscapeKeyDown={event => { if (saving) event.preventDefault(); }}>
      <DialogHeader className="shrink-0">
        <DialogTitle>同步模型</DialogTitle>
        <DialogDescription>搜索并勾选需要的模型，一次添加。已添加的模型和参数保持不变。</DialogDescription>
      </DialogHeader>
      <form className="flex min-h-0 min-w-0 flex-col gap-5 overflow-hidden" onSubmit={event => { event.preventDefault(); void save(); }}>
        <div className="flex min-h-0 flex-col gap-4 overflow-y-auto pr-1">
        <FieldSet disabled={saving}>
          <FieldDescription className="break-all">{protocols[route.protocol]} · {endpoint(route.config.endpoint)}<br />{draft.sourceRouteId && draft.key === undefined ? '使用已保存的 API 配置' : '使用刚填写的 API 配置'}</FieldDescription>
          <FieldSet>
            <FieldLegend>选择模型</FieldLegend>
            <Field><FieldLabel htmlFor="sync-search" className="sr-only">搜索模型</FieldLabel><Input id="sync-search" placeholder="搜索模型…" value={query} onChange={event => setQuery(event.target.value)} autoFocus /></Field>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <Field orientation="horizontal"><Checkbox id="sync-all" disabled={loading || !available.length} checked={allSelected ? true : available.some(id => choices.includes(id)) ? 'indeterminate' : false} onCheckedChange={checked => setSelected(current => checked ? [...new Set([...current, ...available])] : current.filter(id => !available.includes(id)))} /><FieldLabel htmlFor="sync-all">全选搜索结果</FieldLabel></Field>
              <Button type="button" size="sm" variant="ghost" disabled={!choices.length} onClick={() => setSelected([])}>清空选择</Button>
            </div>
            {loading ? <div role="status" className="flex items-center gap-2"><Spinner />正在获取模型列表…</div>
              : listError ? <Alert variant="destructive"><AlertDescription>{listError}</AlertDescription><Button type="button" variant="outline" size="sm" onClick={() => setRefresh(value => value + 1)}>重试</Button></Alert>
              : <ScrollArea className="h-56 rounded-md border p-3"><FieldGroup className="gap-3">
                {!visible.length && <FieldDescription>{models.length ? '没有匹配的模型，试试其他关键词。' : '服务没有返回模型，可以返回手动填写模型 ID。'}</FieldDescription>}
                {visible.map((id, index) => <Field key={id} orientation="horizontal" data-disabled={existing.has(id)}>
                  <Checkbox id={`sync-model-${index}`} aria-label={id} checked={existing.has(id) || choices.includes(id)} disabled={existing.has(id)} onCheckedChange={checked => setSelected(current => checked ? [...new Set([...current, id])] : current.filter(value => value !== id))} />
                  <FieldLabel htmlFor={`sync-model-${index}`} className="min-w-0 flex-1 break-all">{id}</FieldLabel>
                  {existing.has(id) && <Badge variant="secondary">已添加</Badge>}
                </Field>)}
              </FieldGroup></ScrollArea>}
            <FieldDescription role="status">已选 {choices.length} 个 · 共 {models.length} 个模型</FieldDescription>
          </FieldSet>
          {choices.length === 1 && <Field><FieldLabel htmlFor="sync-name">显示名称（选填）</FieldLabel><Input id="sync-name" value={name} onChange={event => setName(event.target.value)} placeholder={choices[0]} /></Field>}
          <ModelParameters route={route} patch={patch} presetOutput={presetOutput} />
          <FieldDescription>新增模型沿用这份 API 配置和模型参数，添加后可分别编辑。</FieldDescription>
          <Field orientation="horizontal"><FieldLabel htmlFor="sync-default">设置新的默认模型</FieldLabel><Checkbox id="sync-default" checked={makeDefault} onCheckedChange={checked => setMakeDefault(checked === true)} /></Field>
          {makeDefault && choices.length > 0 && <Field><FieldLabel htmlFor="sync-default-model">默认模型</FieldLabel><Select value={defaultModel} onValueChange={setDefaultId}><SelectTrigger id="sync-default-model"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{choices.map(id => <SelectItem value={id} key={id}>{id}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>}
        </FieldSet>
        {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription>{error.includes('conflict') && <Button type="button" variant="outline" size="sm" onClick={async () => { if (await reload()) setError('已加载最新配置，选择已保留，请确认后再次添加。'); else setError('加载最新配置失败，选择已保留，请稍后重试。'); }}>加载最新配置并保留选择</Button>}</Alert>}
        </div>
        <DialogFooter className="shrink-0"><Button type="button" variant="outline" disabled={saving} onClick={onClose}>返回</Button><Button type="submit" disabled={loading || saving || !choices.length}>{saving && <Spinner />}{saving ? '正在添加…' : `添加 ${choices.length} 个模型`}</Button></DialogFooter>
      </form>
    </DialogContent>
  </Dialog>;
}
