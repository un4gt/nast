import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Catalog, Route } from '@/models';
import { rpc } from '@/rpc';
import { pushToast } from '@/toasts';

const endpoints: Record<string, string> = { openai: 'https://api.openai.com/v1', anthropic: 'https://api.anthropic.com/v1', gemini: 'https://generativelanguage.googleapis.com/v1beta' };
const newRoute = (): Route => ({ id: crypto.randomUUID(), provider: '', protocol: 'openai', upstream_model: '', priority: 0, enabled: true, config: { endpoint: endpoints.openai, connect_timeout_secs: 10, first_token_timeout_secs: 60, idle_timeout_secs: 60, headers: {}, parameters: {}, remove_parameters: [] } });

export function ModelsPanel() {
  const [catalog, setCatalog] = useState<Catalog>();
  const [credentials, setCredentials] = useState<Record<string,string>>({});
  const [jsonDrafts, setJsonDrafts] = useState<Record<string,string>>({});
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [discovered, setDiscovered] = useState<Record<string,string[]>>({});
  const [fetching, setFetching] = useState('');
  const load = async () => { try { setCatalog(await rpc.call<Catalog>('model_catalog.get')); setCredentials({}); setJsonDrafts({}); setDirty(false); setError(''); } catch(e) { setError(String(e)); } };
  useEffect(() => { void load(); }, []);
  const change = (fn: (draft: Catalog) => void) => { setCatalog(previous => { if (!previous) return previous; const next = structuredClone(previous); fn(next); return next; }); setDirty(true); };
  const save = async () => {
    if (!catalog) return;
    setSaving(true); setError('');
    try {
      const draft = structuredClone(catalog);
      for (const model of draft.models) for (const route of model.routes) for (const key of ['headers','parameters','remove_parameters'] as const) {
        const raw = jsonDrafts[`${route.id}:${key}`];
        if (raw !== undefined) (route.config as any)[key] = JSON.parse(raw);
      }
      const result = await rpc.call<{version:number}>('model_catalog.save', { catalog: draft, credentials });
      setCatalog({ ...draft, version: result.version, models: draft.models.map(m => ({ ...m, routes: m.routes.map(r => ({ ...r, credential_configured: r.id in credentials ? !!credentials[r.id] : r.credential_configured })) })) });
      setCredentials({}); setJsonDrafts({}); setDirty(false); pushToast('模型目录已保存', 'success');
    } catch(e) { setError(`保存失败，草稿已保留：${String(e)}`); } finally { setSaving(false); }
  };
  if (!catalog) return <div role="status">{error || '正在读取模型目录…'}{error && <Button onClick={() => void load()}>重新加载</Button>}</div>;
  return <div className="flex flex-col gap-4">
    <p className="text-xs text-muted-foreground">聊天时选择模型。每个模型可配置多条线路，按优先级尝试；成功后当前会话继续使用该线路。默认模型只影响新聊天。</p>
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    <fieldset disabled={saving} className="flex min-w-0 flex-col gap-4">
    <Accordion type="multiple" className="min-w-0">
      {catalog.models.map((model, mi) => <AccordionItem value={model.id} key={model.id}>
        <AccordionTrigger><span className="min-w-0 truncate">{model.display_name || '未命名模型'}{catalog.default_model === model.id ? ' · 默认' : ''}</span></AccordionTrigger>
        <AccordionContent className="flex flex-col gap-4">
          <FieldGroup><Field><FieldLabel htmlFor={`name-${model.id}`}>显示名称</FieldLabel><Input id={`name-${model.id}`} value={model.display_name} onChange={e => change(c => { c.models[mi].display_name = e.target.value; })}/></Field></FieldGroup>
          <p className="break-all text-xs text-muted-foreground">模型 ID：{model.id}</p>
          <div className="flex flex-wrap gap-2"><Button size="sm" variant="outline" disabled={catalog.default_model === model.id} onClick={() => change(c => { c.default_model = model.id; })}>设为默认</Button><Button size="sm" variant="ghost" disabled={catalog.default_model === model.id} onClick={() => change(c => { c.models.splice(mi,1); })}>删除模型</Button></div>
          <Accordion type="multiple">
          {model.routes.map((route, ri) => {
            const patch = (fn: (r: Route) => void) => change(c => fn(c.models[mi].routes[ri]));
            return <AccordionItem key={route.id} value={route.id}>
              <AccordionTrigger><span className="truncate">{route.provider || route.protocol} · {route.upstream_model || '待配置'}{!route.enabled ? ' · 已停用' : ''}</span></AccordionTrigger>
              <AccordionContent className="flex flex-col gap-4">
                <FieldGroup>
                  <Field orientation="horizontal"><Switch id={`enabled-${route.id}`} checked={route.enabled} onCheckedChange={enabled => patch(r => {r.enabled=enabled;})}/><FieldLabel htmlFor={`enabled-${route.id}`}>启用线路</FieldLabel></Field>
                  <Field><FieldLabel>协议</FieldLabel><Select value={route.protocol} onValueChange={protocol => patch(r => { if (r.config.endpoint === endpoints[r.protocol]) r.config.endpoint = endpoints[protocol]; r.protocol=protocol; })}><SelectTrigger aria-label="协议"><SelectValue/></SelectTrigger><SelectContent><SelectGroup><SelectItem value="openai">OpenAI 兼容</SelectItem><SelectItem value="anthropic">Anthropic</SelectItem><SelectItem value="gemini">Gemini</SelectItem></SelectGroup></SelectContent></Select></Field>
                  <Field><FieldLabel htmlFor={`provider-${route.id}`}>线路名称</FieldLabel><Input id={`provider-${route.id}`} value={route.provider} placeholder="例如 OpenRouter" onChange={e => patch(r => {r.provider=e.target.value;})}/></Field>
                  <Field><FieldLabel htmlFor={`endpoint-${route.id}`}>端点（含 /v1 或 /v1beta）</FieldLabel><Input id={`endpoint-${route.id}`} value={route.config.endpoint} onChange={e => patch(r => {r.config.endpoint=e.target.value;})}/></Field>
                  <Field><FieldLabel htmlFor={`key-${route.id}`}>API 密钥</FieldLabel><Input id={`key-${route.id}`} type="password" autoComplete="new-password" value={credentials[route.id] ?? ''} placeholder={route.credential_configured ? '已配置 · 输入以替换' : '未配置'} onChange={e => {setCredentials(c => ({...c,[route.id]:e.target.value}));setDirty(true);}}/><Button variant="ghost" size="sm" onClick={() => {setCredentials(c => ({...c,[route.id]:''}));setDirty(true);}}>清除密钥（保存后生效）</Button></Field>
                  <Field><FieldLabel htmlFor={`upstream-${route.id}`}>上游模型 ID</FieldLabel><Input id={`upstream-${route.id}`} list={`discovered-${route.id}`} value={route.upstream_model} onChange={e => patch(r => {r.upstream_model=e.target.value;})}/><datalist id={`discovered-${route.id}`}>{discovered[route.id]?.map(id => <option value={id} key={id}/>)}</datalist></Field>
                  {route.protocol === 'openai' && <Button size="sm" variant="outline" disabled={dirty || !!fetching} onClick={async () => {setFetching(route.id);try { const result = await rpc.call<{data:string[]}>('models.list',{route_id:route.id});setDiscovered(d=>({...d,[route.id]:result.data}));pushToast(`已获取 ${result.data.length} 个模型，可在上方输入框选择`,'success'); } catch(e) {setError(String(e));} finally {setFetching('');}}}>{fetching === route.id ? '正在获取…' : dirty ? '保存后可获取上游列表' : '获取上游模型列表'}</Button>}
                  <Field><FieldLabel htmlFor={`priority-${route.id}`}>优先级（越大越优先）</FieldLabel><Input id={`priority-${route.id}`} type="number" value={route.priority} onChange={e => patch(r => {r.priority=Number(e.target.value);})}/></Field>
                </FieldGroup>
                <Accordion type="single" collapsible><AccordionItem value="advanced"><AccordionTrigger>高级配置</AccordionTrigger><AccordionContent><FieldGroup>
                  {([['context_limit','上下文限制'],['output_limit','输出限制'],['connect_timeout_secs','连接超时（秒）'],['first_token_timeout_secs','首段内容等待（秒）'],['idle_timeout_secs','流式空闲超时（秒）']] as const).map(([key,label]) => <Field key={key}><FieldLabel htmlFor={`${key}-${route.id}`}>{label}</FieldLabel><Input id={`${key}-${route.id}`} type="number" min={1} value={route.config[key] ?? ''} onChange={e => patch(r => {(r.config as any)[key]=e.target.value === '' ? null : Number(e.target.value);})}/></Field>)}
                  {([['headers','附加请求头（JSON 对象）'],['parameters','请求参数覆盖（JSON 对象）'],['remove_parameters','移除参数（JSON 字符串数组）']] as const).map(([key,label]) => <Field key={key}><FieldLabel htmlFor={`${key}-${route.id}`}>{label}</FieldLabel><Textarea id={`${key}-${route.id}`} className="font-mono text-xs" value={jsonDrafts[`${route.id}:${key}`] ?? JSON.stringify(route.config[key],null,2)} onChange={e => {setJsonDrafts(d=>({...d,[`${route.id}:${key}`]:e.target.value}));setDirty(true);}}/></Field>)}
                </FieldGroup></AccordionContent></AccordionItem></Accordion>
                <Button size="sm" variant="ghost" onClick={() => change(c => {c.models[mi].routes.splice(ri,1);})}>删除线路</Button>
              </AccordionContent>
            </AccordionItem>;
          })}</Accordion>
          <Button variant="outline" onClick={() => change(c => {c.models[mi].routes.push(newRoute());})}>添加线路</Button>
        </AccordionContent>
      </AccordionItem>)}
    </Accordion>
    <Button variant="outline" onClick={() => change(c => { const id=crypto.randomUUID(); c.models.push({id,display_name:'新模型',routes:[newRoute()]}); })}>添加模型</Button>
    <div className="flex flex-wrap gap-2"><Button disabled={!dirty} onClick={() => void save()}>{saving ? '正在保存…' : '保存模型目录'}</Button><Button variant="ghost" onClick={() => void load()}>放弃草稿并重新加载</Button></div>
    </fieldset>
  </div>;
}
