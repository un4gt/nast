import { useEffect, useRef, useState } from 'react';
import { ArrowUpRight, Check, ChevronRight, Ellipsis, Layers3, Plus, Search, Sparkles, Unplug } from 'lucide-react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card';
import { DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { channelRoute, newChannel, newModel, protocols, type PublishCatalog } from '@/lib/model-config';
import { cn } from '@/lib/utils';
import type { Catalog, Channel, LogicalModel } from '@/models';
import { rpc } from '@/rpc';
import { useStore } from '@/store';
import { pushToast } from '@/toasts';
import { ChannelEditor, LogicalModelEditor } from './ModelEditors';
import { ModelSyncDialog } from './ModelSyncDialog';

export function ModelsPanel() {
  const [catalog, setCatalog] = useState<Catalog>();
  const [tab, setTab] = useState('models');
  const [query, setQuery] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const [model, setModel] = useState<LogicalModel>();
  const [channel, setChannel] = useState<Channel>();
  const [sync, setSync] = useState<Channel>();
  const [remove, setRemove] = useState<{ kind: 'model' | 'channel'; id: string; name: string }>();
  const connected = useStore(s => s.connected);
  const load = async () => { const value = await rpc.call<Catalog>('model_catalog.get'); setCatalog(value); setError(''); };
  useEffect(() => {
    if (!connected) return;
    let alive = true;
    const refresh = () => rpc.call<Catalog>('model_catalog.get').then(value => { if (alive) { setCatalog(value); setError(''); } }).catch(e => { if (alive) setError(String(e)); });
    void refresh();
    const off = rpc.on('model_catalog_changed', () => void refresh());
    return () => { alive = false; off(); };
  }, [connected]);
  const publish: PublishCatalog = async (next, credentials = {}) => {
    if (lock.current) throw new Error('正在保存，请稍后');
    lock.current = true; setBusy(true);
    try {
      await rpc.call('model_catalog.save', { catalog: next, channel_credentials: credentials });
      await load();
    } catch (e) {
      // Refresh the version without discarding editor drafts.
      await load().catch(() => {});
      throw e;
    } finally { lock.current = false; setBusy(false); }
  };
  const action = async (work: () => Promise<void>) => { try { await work(); setError(''); } catch (e) { setError(String(e)); } };
  if (!catalog) return error ? <Alert variant="destructive"><AlertDescription>{error}</AlertDescription><Button onClick={() => void action(load)}>重新加载</Button></Alert> : <div className="grid gap-4 sm:grid-cols-2"><Skeleton className="h-40" /><Skeleton className="h-40" /></div>;
  const available = (m: LogicalModel) => m.routes.filter(r => r.enabled && catalog.channels.some(c => c.id === r.channel_id && c.enabled));
  const channelCount = (m: LogicalModel) => new Set(available(m).map(r => r.channel_id)).size;
  const defaultModel = catalog.models.find(m => m.id === catalog.default_model);
  const search = query.trim().toLowerCase();
  const models = catalog.models.filter(m => (search || m.id !== catalog.default_model) && [m.display_name, ...m.routes.map(r => r.upstream_model)].some(s => s.toLowerCase().includes(search)));
  const channels = catalog.channels.filter(c => `${c.name} ${c.endpoint}`.toLowerCase().includes(search));
  return <div className="flex flex-col gap-7">
    <div className="flex items-start justify-between gap-4">
      <div className="flex flex-col gap-2"><p className="text-xs tracking-widest text-muted-foreground">连接与选择</p><h3 className="font-display text-2xl font-medium sm:text-3xl">为对话，选一个好搭档。</h3><p className="text-sm leading-relaxed text-muted-foreground">选好模型，安心聊下去。渠道会记住这段对话。</p></div>
      <Sparkles className="mt-2 hidden size-8 shrink-0 text-primary/60 sm:block" aria-hidden="true" />
    </div>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <Tabs value={tab} onValueChange={value => { setTab(value); setQuery(''); }}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <TabsList><TabsTrigger value="models">模型 <span className="ml-2 tabular-nums">{catalog.models.length}</span></TabsTrigger><TabsTrigger value="channels">渠道 <span className="ml-2 tabular-nums">{catalog.channels.length}</span></TabsTrigger></TabsList>
        <Button size="sm" disabled={busy} onClick={() => tab === 'models' ? setModel(newModel()) : setChannel(newChannel())}><Plus data-icon="inline-start" />{tab === 'models' ? '添加模型' : '添加渠道'}</Button>
      </div>
      <TabsContent value="models" className="mt-6 flex flex-col gap-5">
        {defaultModel && !query && <Card className="border-primary/20 bg-accent/40 shadow-none">
          <CardHeader className="pb-3"><CardDescription>新对话的默认选择</CardDescription><CardTitle className="break-all">{defaultModel.display_name}</CardTitle></CardHeader>
          <CardContent className="pb-2"><p className="text-sm text-muted-foreground">{channelCount(defaultModel) ? `${channelCount(defaultModel)} 个可用渠道 · 正常沿用，故障时自动切换` : '关联一个渠道，就可以开始对话。'}</p></CardContent>
          <CardFooter className="pb-4"><Button variant="ghost" size="sm" aria-label={`配置 ${defaultModel.display_name}`} onClick={() => setModel(structuredClone(defaultModel))}>管理渠道<ArrowUpRight data-icon="inline-end" /></Button></CardFooter>
        </Card>}
        <div className="relative"><Search className="pointer-events-none absolute left-3 top-3 size-4 text-muted-foreground" /><Input className="pl-9" aria-label="搜索模型" placeholder="寻找一个模型…" value={query} onChange={e => setQuery(e.target.value)} /></div>
        <div className="grid gap-3 sm:grid-cols-2">
          {models.map(m => {
            const routes = available(m); const names = [...new Set(routes.map(r => catalog.channels.find(c => c.id === r.channel_id)?.name))];
            return <Card key={m.id} className="flex min-w-0 flex-col shadow-none transition-colors hover:border-primary/40">
              <CardHeader className="flex-row items-start justify-between gap-2 p-4 pb-3"><div className="min-w-0 flex-1"><CardTitle className="break-all text-base leading-relaxed">{m.display_name}</CardTitle><CardDescription className="mt-1">{routes.length ? `${channelCount(m)} 个可用渠道` : '尚未连接渠道'}</CardDescription></div>
                <DropdownMenu><DropdownMenuTrigger asChild><Button size="icon-sm" variant="ghost" aria-label={`${m.display_name} 的操作`} disabled={busy}><Ellipsis /></Button></DropdownMenuTrigger><DropdownMenuContent align="end"><DropdownMenuGroup>
                  <DropdownMenuItem onSelect={() => setModel(structuredClone(m))}>编辑模型</DropdownMenuItem>
                  <DropdownMenuItem disabled={m.id === catalog.default_model} onSelect={() => void action(() => publish({ ...catalog, default_model: m.id }))}>设为默认</DropdownMenuItem>
                  <DropdownMenuItem disabled={m.id === catalog.default_model} onSelect={() => setRemove({ kind: 'model', id: m.id, name: m.display_name })}>删除模型</DropdownMenuItem>
                </DropdownMenuGroup></DropdownMenuContent></DropdownMenu>
              </CardHeader>
              <CardContent className="flex flex-1 flex-wrap gap-1.5 px-4 pb-4">{names.slice(0, 2).map(name => <Badge variant="secondary" key={name} className="max-w-full truncate">{name}</Badge>)}{names.length > 2 && <Badge variant="outline">+{names.length - 2}</Badge>}{!routes.length && <Unplug className="size-4 text-muted-foreground" />}</CardContent>
              <CardFooter className="justify-between px-4 pb-3">{m.id === catalog.default_model ? <span className="flex items-center gap-1 text-xs text-primary"><Check className="size-3" />默认</span> : <span className="text-xs text-muted-foreground">{routes.some(r => r.config.supports_images === true) ? '支持图片的渠道' : ''}</span>}<Button size="sm" variant="ghost" aria-label={`配置 ${m.display_name}`} onClick={() => setModel(structuredClone(m))}>配置<ChevronRight data-icon="inline-end" /></Button></CardFooter>
            </Card>;
          })}
        </div>
        {!models.length && (query || !defaultModel) && <Empty><EmptyHeader><EmptyTitle>{query ? '没有找到这个模型' : '给下一段对话选个模型'}</EmptyTitle><EmptyDescription>{query ? '试试模型名称或服务商提供的 ID。' : '先连接渠道，就能获取它提供的模型，也可以手动添加。'}</EmptyDescription></EmptyHeader><EmptyContent><Button variant="outline" onClick={() => { setTab('channels'); setQuery(''); }}>管理渠道</Button></EmptyContent></Empty>}
      </TabsContent>
      <TabsContent value="channels" className="mt-6 flex flex-col gap-5">
        <p className="text-sm leading-relaxed text-muted-foreground">一个渠道，一份连接。它提供的模型可以加入已有模型的备用渠道。</p>
        <Input aria-label="搜索渠道" placeholder="搜索渠道名称或地址…" value={query} onChange={e => setQuery(e.target.value)} />
        <div className="grid gap-4 sm:grid-cols-2">{channels.map(c => <Card key={c.id} className={cn('min-w-0 shadow-none', !c.enabled && 'opacity-60')}>
          <CardHeader className="p-5 pb-3"><div className="flex items-start justify-between gap-3"><CardTitle className="break-all text-lg">{c.name}</CardTitle><Badge variant={c.enabled ? 'secondary' : 'outline'}>{c.enabled ? '已启用' : '已停用'}</Badge></div><CardDescription>{protocols[c.protocol]}</CardDescription></CardHeader>
          <CardContent className="flex flex-col gap-3 px-5 pb-4"><p className="truncate text-xs text-muted-foreground" title={c.endpoint}>{c.endpoint}</p><p className="text-sm">{catalog.models.filter(m => m.routes.some(r => r.channel_id === c.id)).length} 个关联模型</p></CardContent>
          <CardFooter className="flex-wrap justify-between gap-2 px-5 pb-4"><Button size="sm" variant="outline" disabled={busy || c.protocol !== 'openai'} onClick={() => setSync(c)}>获取模型列表</Button><DropdownMenu><DropdownMenuTrigger asChild><Button size="icon-sm" variant="ghost" aria-label={`${c.name} 的操作`} disabled={busy}><Ellipsis /></Button></DropdownMenuTrigger><DropdownMenuContent align="end"><DropdownMenuGroup><DropdownMenuItem onSelect={() => setChannel(structuredClone(c))}>编辑与测试</DropdownMenuItem><DropdownMenuItem onSelect={() => { const m = newModel(); m.routes = [channelRoute(c)]; setModel(m); }}>手动关联模型</DropdownMenuItem><DropdownMenuItem onSelect={() => setRemove({ kind: 'channel', id: c.id, name: c.name })}>删除渠道</DropdownMenuItem></DropdownMenuGroup></DropdownMenuContent></DropdownMenu></CardFooter>
        </Card>)}</div>
        {!channels.length && <Empty><EmptyHeader><Layers3 className="mx-auto size-8 text-muted-foreground" /><EmptyTitle>{query ? '没有匹配的渠道' : '连接你的第一个渠道'}</EmptyTitle><EmptyDescription>填写服务商的 API 地址和密钥，一次连接，供多个模型使用。</EmptyDescription></EmptyHeader><EmptyContent><Button onClick={() => setChannel(newChannel())}>添加渠道</Button></EmptyContent></Empty>}
      </TabsContent>
    </Tabs>
    {model && <LogicalModelEditor key={model.id} model={model} catalog={catalog} publish={publish} onClose={() => setModel(undefined)} />}
    {channel && <ChannelEditor key={channel.id} channel={channel} catalog={catalog} publish={publish} onClose={() => setChannel(undefined)} />}
    {sync && <ModelSyncDialog channel={sync} catalog={catalog} publish={publish} onClose={() => setSync(undefined)} />}
    <AlertDialog open={!!remove} onOpenChange={open => { if (!open && !busy) setRemove(undefined); }}><AlertDialogContent><AlertDialogHeader><AlertDialogTitle>删除“{remove?.name}”？</AlertDialogTitle><AlertDialogDescription>{remove?.kind === 'channel' ? '该渠道及其模型关联将被移除。受影响的会话将在下次请求时尝试其他渠道，聊天记录保留。' : '使用这个模型的会话需要重新选择模型，聊天记录保留。'}</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel disabled={busy}>取消</AlertDialogCancel><AlertDialogAction disabled={busy} onClick={e => { e.preventDefault(); void action(async () => {
      if (!remove) return;
      const next = structuredClone(catalog);
      if (remove.kind === 'model') next.models = next.models.filter(m => m.id !== remove.id);
      else { next.channels = next.channels.filter(c => c.id !== remove.id); next.models.forEach(m => { m.routes = m.routes.filter(r => r.channel_id !== remove.id); }); }
      await publish(next); setRemove(undefined); pushToast('已删除', 'success');
    }); }}>删除</AlertDialogAction></AlertDialogFooter></AlertDialogContent></AlertDialog>
  </div>;
}
