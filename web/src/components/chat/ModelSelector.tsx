import { useEffect, useState } from 'react';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { rpc } from '@/rpc';
import { useStore } from '@/store';
import { Catalog, currentConversation } from '@/models';

export function ModelSelector() {
  const { generating, connected, activeAvatar, activeChatName, activeGroupId, groups } = useStore();
  const conversationKey = JSON.stringify(currentConversation());
  const [catalog, setCatalog] = useState<Catalog>();
  const [selected, setSelected] = useState('');
  const [error, setError] = useState('');
  const [phase, setPhase] = useState('');
  const [serverBusy, setServerBusy] = useState(false);
  const [pending, setPending] = useState(false);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let alive = true;
    const conversation = JSON.parse(conversationKey);
    const refresh = async () => {
      if (!conversation || !connected) return;
      try {
        const [next, view] = await Promise.all([rpc.call<Catalog>('model_catalog.get'), rpc.call('conversation_model.get', { conversation })]);
        if (alive) { setCatalog(next); setSelected(view.state.selected_model); setError(view.error ?? ''); }
      } catch (e) { if (alive) setError(String(e)); }
    };
    void refresh();
    if (connected) void rpc.call('generate.status').then(s => { if (alive) setServerBusy(s.running); }).catch(() => {});
    const offStarted = rpc.on('generation_started', () => setServerBusy(true));
    const offEnded = rpc.on('generation_ended', () => { setServerBusy(false); setPhase(''); });
    const offCatalog = rpc.on('model_catalog_changed', () => void refresh());
    const offModel = rpc.on('conversation_model_changed', data => { if (JSON.stringify(data.conversation) === conversationKey) void refresh(); });
    const offRoute = rpc.on('generation_route', data => { if (JSON.stringify(data.conversation) === conversationKey) setPhase(data.phase); });
    const show = () => setOpen(true);
    window.addEventListener('nast:select-model', show);
    return () => { alive = false; offStarted(); offEnded(); offCatalog(); offModel(); offRoute(); window.removeEventListener('nast:select-model', show); };
  }, [conversationKey, connected, activeAvatar, activeChatName, activeGroupId, groups]);
  return <div className="flex min-w-0 flex-wrap items-center gap-2 border-b px-4 py-2 sm:px-6">
    <Select open={open} onOpenChange={setOpen} value={selected} disabled={!connected || generating || serverBusy || pending} onValueChange={async id => {
      setPending(true);
      try { const view = await rpc.call('conversation_model.set', { conversation: currentConversation(), model_id: id }); setSelected(id); setError(view.error ?? ''); }
      catch (e) { setError(String(e)); } finally { setPending(false); }
    }}>
      <SelectTrigger aria-label="会话模型" className="h-8 w-auto min-w-32 max-w-full gap-2 text-xs"><SelectValue placeholder="选择模型">{catalog?.models.find(m => m.id === selected)?.display_name ?? (selected ? '模型已删除，请重新选择' : '选择模型')}</SelectValue></SelectTrigger>
      <SelectContent><SelectGroup>{catalog?.models.map(m => <SelectItem key={m.id} value={m.id}>{m.display_name}{m.routes.some(r => r.enabled) ? '' : ' · 尚未配置'}</SelectItem>)}</SelectGroup></SelectContent>
    </Select>
    <span role="status" className="text-xs text-muted-foreground">{(generating || serverBusy) && (phase === 'retrying' ? '正在重试' : phase === 'fallback' ? '正在切换备用线路' : '')}</span>
    {error && <div role="alert" className="flex w-full flex-wrap items-center gap-2 text-xs text-destructive"><span>{error}</span><Button size="sm" variant="outline" onClick={() => window.dispatchEvent(new CustomEvent('nast:open-model-settings'))}>模型设置</Button></div>}
  </div>;
}
