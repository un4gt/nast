import { useEffect, useRef, useState } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Checkbox } from '@/components/ui/checkbox';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Spinner } from '@/components/ui/spinner';
import { endpoints, newConnection, newModel, primaryConnection, protocols, validateConnection } from '@/lib/model-config';
import { Catalog, LogicalModel, Route } from '@/models';
import { rpc } from '@/rpc';
import { useStore } from '@/store';
import { pushToast } from '@/toasts';
import { ModelParameters } from './ModelParameters';
import { ModelSyncDialog, type ModelSyncDraft } from './ModelSyncDialog';

type Edit = { model: LogicalModel; routeId: string; isNew: boolean; key?: string; makeDefault: boolean; originalEndpoint: string; originalProtocol: string };

// Cline's simple API setup, composed with the project's shadcn Dialog and Field.
export function ModelsPanel() {
  const [catalog, setCatalog] = useState<Catalog>();
  const [edit, setEdit] = useState<Edit>();
  const [removing, setRemoving] = useState<LogicalModel>();
  const [error, setError] = useState('');
  const [formError, setFormError] = useState('');
  const [saving, setSaving] = useState(false);
  const saveLock = useRef(false);
  const [sync, setSync] = useState<ModelSyncDraft>();
  const presetOutput = useStore(s => s.settings?.oai_settings?.openai_max_tokens ?? 300);

  const load = async () => {
    try { setCatalog(await rpc.call<Catalog>('model_catalog.get')); setError(''); return true; }
    catch (e) { setError(String(e)); return false; }
  };
  useEffect(() => { void load(); }, []);
  const route = edit?.model.routes.find(r => r.id === edit.routeId);
  const patch = (change: (r: Route) => void) => setEdit(current => {
    if (!current) return current;
    const next = structuredClone(current);
    change(next.model.routes.find(r => r.id === next.routeId)!);
    return next;
  });
  const openEditor = (model?: LogicalModel) => {
    const next = model ? structuredClone(model) : newModel();
    if (!next.routes.length) next.routes.push(newConnection());
    const connection = primaryConnection(next)!;
    setEdit({ model: next, routeId: connection.id, isNew: !model,
      makeDefault: !model || catalog?.default_model === model.id,
      originalEndpoint: connection.config.endpoint, originalProtocol: connection.protocol });
    setFormError('');
  };
  const publish = async (next: Catalog, credentials: Record<string, string> = {}, credentialCopies: Record<string, string> = {}) => {
    if (saveLock.current) return false;
    saveLock.current = true; setSaving(true);
    try {
      const result = await rpc.call<{version: number}>('model_catalog.save', { catalog: next, credentials, credential_copies: credentialCopies });
      setCatalog({ ...next, version: result.version, models: next.models.map(m => ({ ...m, routes: m.routes.map(r => ({ ...r, credential_configured: r.id in credentials ? !!credentials[r.id] : r.credential_configured })) })) });
      setError(''); return true;
    } finally { saveLock.current = false; setSaving(false); }
  };
  const save = async () => {
    if (!catalog || !edit || !route || saveLock.current) return;
    setFormError('');
    const invalid = validateConnection(route, presetOutput);
    if (invalid) { setFormError(invalid); return; }
    if (route.credential_configured && edit.key === undefined && (route.config.endpoint !== edit.originalEndpoint || route.protocol !== edit.originalProtocol)) {
      setFormError('API 地址或类型已更改，请重新输入密钥，或明确清除原密钥。'); return;
    }
    const model = structuredClone(edit.model);
    model.display_name = model.display_name.trim() || route.upstream_model.trim();
    const connection = model.routes.find(r => r.id === edit.routeId)!;
    connection.upstream_model = connection.upstream_model.trim();
    connection.config.endpoint = connection.config.endpoint.trim().replace(/\/+$/, '');
    connection.enabled = true;
    const next = structuredClone(catalog);
    if (edit.isNew) next.models.push(model);
    else {
      const index = next.models.findIndex(m => m.id === model.id);
      if (index < 0) { setFormError('这个模型已被删除，请取消后重新添加。'); return; }
      next.models[index] = model;
    }
    if (edit.makeDefault) next.default_model = model.id;
    try {
      if (await publish(next, edit.key === undefined ? {} : { [route.id]: edit.key })) {
        setEdit(undefined); pushToast('模型已保存', 'success');
      }
    } catch (e) { setFormError(`保存失败，输入已保留：${String(e)}`); }
  };
  const discover = () => {
    if (!route || !edit) return;
    const invalid = validateConnection({ ...route, upstream_model: route.upstream_model || 'discovery' }, presetOutput);
    if (invalid) { setFormError(invalid); return; }
    if (route.credential_configured && edit.key === undefined && (route.config.endpoint !== edit.originalEndpoint || route.protocol !== edit.originalProtocol)) {
      setFormError('API 地址或类型已更改，请重新输入密钥，或明确清除原密钥。'); return;
    }
    setSync({ route: structuredClone(route), key: edit.key, sourceRouteId: edit.isNew ? undefined : route.id,
      name: edit.isNew ? edit.model.display_name : '', makeDefault: edit.isNew && edit.makeDefault });
  };
  if (!catalog) return <div role="status" className="flex items-center gap-2">{error || '正在加载模型…'}{error && <Button onClick={() => void load()}>重试</Button>}</div>;
  return <div className="flex min-w-0 flex-col gap-4">
    <div className="flex items-center justify-between gap-3"><p className="text-sm text-muted-foreground">添加模型，或同步同一 API 下的多个模型。</p><Button size="sm" onClick={() => openEditor()} disabled={saving}><Plus data-icon="inline-start" />添加模型</Button></div>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <ul className="flex min-w-0 flex-col divide-y">
      {catalog.models.map(model => {
        const connection = primaryConnection(model);
        return <li key={model.id} className="flex min-w-0 flex-wrap items-center gap-2 py-3">
          <div className="min-w-0 flex-1"><div className="flex items-center gap-2"><span className="truncate text-sm font-medium">{model.display_name}</span>{catalog.default_model === model.id && <Badge variant="secondary">默认</Badge>}</div><p className="truncate text-xs text-muted-foreground">{connection ? `${protocols[connection.protocol]} · ${connection.upstream_model}` : '尚未配置'}</p></div>
          <div className="flex flex-wrap items-center gap-1">
            <Button size="sm" variant="ghost" disabled={saving || catalog.default_model === model.id} onClick={async () => { try { await publish({ ...catalog, default_model: model.id }); } catch (e) { setError(String(e)); } }}>设为默认</Button>
            {connection?.protocol === 'openai' && <Button size="sm" variant="outline" aria-label={`同步 ${model.display_name} 的模型`} disabled={saving} onClick={() => setSync({ route: structuredClone(connection), sourceRouteId: connection.id, makeDefault: false })}>同步模型</Button>}
            <Button size="sm" variant="outline" aria-label={`编辑 ${model.display_name}`} disabled={saving} onClick={() => openEditor(model)}>编辑</Button>
            <Button size="icon" variant="ghost" aria-label={`删除 ${model.display_name}`} disabled={saving || catalog.default_model === model.id} onClick={() => setRemoving(model)}><Trash2 data-icon="inline-start" /></Button>
          </div>
        </li>;
      })}
    </ul>
    <p className="text-xs text-muted-foreground">默认模型用于新聊天。已有聊天可在对话顶部切换模型。</p>
    <Dialog open={!!edit && !sync} onOpenChange={open => { if (!open && !saving) { setEdit(undefined); } }}>
      <DialogContent className="sm:max-w-xl" onInteractOutside={event => event.preventDefault()} onEscapeKeyDown={event => { if (saving) event.preventDefault(); }}>
        <DialogHeader><DialogTitle>{edit?.isNew ? '添加模型' : '编辑模型'}</DialogTitle><DialogDescription>填写服务商提供的 API 地址、密钥和模型 ID。</DialogDescription></DialogHeader>
        {edit && route && <form className="flex min-w-0 flex-col gap-5" onSubmit={e => { e.preventDefault(); void save(); }}>
          <fieldset disabled={saving} className="flex min-w-0 flex-col gap-5">
            <FieldGroup>
              <Field><FieldLabel htmlFor="model-protocol">API 类型</FieldLabel><Select value={route.protocol} onValueChange={value => {
                patch(r => {
                  if (r.config.endpoint === endpoints[r.protocol]) r.config.endpoint = endpoints[value];
                  r.protocol = value; r.config.parameters = {}; r.config.output_limit = null; r.config.remove_parameters = [];
                });
              }}><SelectTrigger id="model-protocol"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{Object.entries(protocols).map(([value, label]) => <SelectItem value={value} key={value}>{label}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>
              <Field><FieldLabel htmlFor="model-endpoint">API 地址</FieldLabel><Input id="model-endpoint" type="url" required value={route.config.endpoint} placeholder={endpoints[route.protocol]} onChange={e => { patch(r => { r.config.endpoint = e.target.value; }); }} /><FieldDescription>填写基础地址（Base URL），例如 {endpoints[route.protocol]}。</FieldDescription></Field>
              <Field><FieldLabel htmlFor="model-key">API 密钥</FieldLabel><Input id="model-key" type="password" autoComplete="new-password" value={edit.key ?? ''} placeholder={route.credential_configured ? '已保存，留空保持原密钥' : '本地免密服务可留空'} onChange={e => { setEdit({ ...edit, key: e.target.value || undefined }); }} />{route.credential_configured && <Button type="button" variant="ghost" size="sm" className="self-start" onClick={() => { setEdit({ ...edit, key: '' }); }}>{edit.key === '' ? '保存时将清除密钥' : '清除已保存的密钥'}</Button>}</Field>
              <Field><FieldLabel htmlFor="model-upstream">模型 ID</FieldLabel><Input id="model-upstream" required value={route.upstream_model} placeholder="例如 gpt-4o" onChange={e => patch(r => { r.upstream_model = e.target.value; })} />
                {route.protocol === 'openai' && <Button type="button" variant="outline" size="sm" className="self-start" disabled={!route.config.endpoint.trim()} onClick={discover}>获取模型列表</Button>}
                {route.protocol === 'openai' && <FieldDescription>获取后可搜索、多选，一次添加多个模型。</FieldDescription>}
              </Field>
              <Field><FieldLabel htmlFor="model-name">显示名称（选填）</FieldLabel><Input id="model-name" value={edit.model.display_name} placeholder="默认使用模型 ID" onChange={e => setEdit({ ...edit, model: { ...edit.model, display_name: e.target.value } })} /></Field>
            </FieldGroup>
            <ModelParameters route={route} patch={patch} presetOutput={presetOutput} />
            <Field orientation="horizontal"><FieldLabel htmlFor="model-default">设为默认模型</FieldLabel><Checkbox id="model-default" checked={edit.makeDefault} disabled={!edit.isNew && catalog.default_model === edit.model.id} onCheckedChange={checked => setEdit({ ...edit, makeDefault: checked === true })} /></Field>
          </fieldset>
          {formError && <Alert variant="destructive"><AlertDescription>{formError}</AlertDescription>{formError.includes('conflict') && <Button type="button" size="sm" variant="outline" className="mt-2" onClick={async () => { if (await load()) setFormError('已加载最新配置，当前输入已保留，请确认后再次保存。'); }}>加载最新配置并保留输入</Button>}</Alert>}
          <DialogFooter><Button type="button" variant="outline" disabled={saving} onClick={() => { setEdit(undefined); }}>取消</Button><Button type="submit" disabled={saving}>{saving && <Spinner />}{saving ? '正在保存…' : '保存模型'}</Button></DialogFooter>
        </form>}
      </DialogContent>
    </Dialog>
    {sync && <ModelSyncDialog draft={sync} catalog={catalog} presetOutput={presetOutput} publish={publish} reload={load}
      onClose={() => setSync(undefined)} onSaved={count => { setSync(undefined); setEdit(undefined); pushToast(`已添加 ${count} 个模型`, 'success'); }} />}
    <AlertDialog open={!!removing} onOpenChange={open => { if (!open) setRemoving(undefined); }}><AlertDialogContent><AlertDialogHeader><AlertDialogTitle>删除模型？</AlertDialogTitle><AlertDialogDescription>删除“{removing?.display_name}”后，使用它的聊天需要重新选择模型。聊天记录会保留。</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel>取消</AlertDialogCancel><AlertDialogAction onClick={async () => { if (!removing) return; try { await publish({ ...catalog, models: catalog.models.filter(m => m.id !== removing.id) }); } catch (e) { setError(String(e)); } }}>删除模型</AlertDialogAction></AlertDialogFooter></AlertDialogContent></AlertDialog>
  </div>;
}
