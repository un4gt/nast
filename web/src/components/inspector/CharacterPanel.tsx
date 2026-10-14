import { useEffect, useRef, useState } from 'react';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { downloadBase64 } from '@/lib/download';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { Label } from '@/components/ui/label';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { FileImage, FileJson, PenSquare, Plus, Save, UserRound } from 'lucide-react';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';

const EDITABLE_FIELDS = [
  { key: 'name', label: '角色名称', rows: 1 },
  { key: 'description', label: '描述（Description）', rows: 15 },
  { key: 'personality', label: '性格（Personality）', rows: 8 },
  { key: 'scenario', label: '场景（Scenario）', rows: 8 },
  { key: 'first_mes', label: '开场白（First Message）', rows: 12 },
  { key: 'mes_example', label: '对话示例（Examples）', rows: 15 },
  { key: 'system_prompt', label: '系统提示覆盖（System Prompt）', rows: 6 },
  { key: 'post_history_instructions', label: 'PHI / Jailbreak 覆盖', rows: 6 },
  { key: 'creator_notes', label: '作者说明', rows: 6 },
  { key: 'creator', label: '作者', rows: 1 },
  { key: 'character_version', label: '版本', rows: 1 },
] as const;

export function CharacterPanel({ avatar: memberAvatar, onAvatarChange }: { avatar?: string; onAvatarChange?: (avatar: string) => void } = {}) {
  const { characters, activeAvatar: selectedAvatar, loadAll, connected } = useStore();
  const activeAvatar = memberAvatar ?? selectedAvatar;
  const ch = characters.find((c) => c.avatar === activeAvatar);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editorOpen, setEditorOpen] = useState(false);
  const [loadedAvatar, setLoadedAvatar] = useState<string | null>(null);
  const [loadError, setLoadError] = useState('');
  const [loadAttempt, setLoadAttempt] = useState(0);
  const ready = connected && loadedAvatar === activeAvatar && activeAvatar !== null;
  const [greetings, setGreetings] = useState<string[]>([]);
  const [tags, setTags] = useState('');
  const [depthPrompt, setDepthPrompt] = useState({ prompt: '', depth: 4, role: 'system' });
  const renamedDraft = useRef<string | null>(null);

  useEffect(() => {
    if (renamedDraft.current === activeAvatar) { renamedDraft.current = null; return; }
    let cancelled = false;
    setDraft({});
    setDirty(false);
    setLoadedAvatar(null);
    setLoadError('');
    setGreetings([]);
    setTags('');
    setDepthPrompt({ prompt: '', depth: 4, role: 'system' });
    if (!activeAvatar) return;
    rpc
      .call<any>('characters.get', { avatar: activeAvatar })
      .then((c) => {
        if (cancelled) return;
        const d: Record<string, string> = {};
        for (const f of EDITABLE_FIELDS) d[f.key] = c.data?.[f.key] ?? '';
        setDraft(d);
        setGreetings(c.data?.alternate_greetings ?? []);
        setTags((c.data?.tags ?? []).join(', '));
        setDepthPrompt(c.data?.extensions?.depth_prompt ?? { prompt: '', depth: 4, role: 'system' });
        setLoadedAvatar(activeAvatar);
      })
      .catch((e) => { if (!cancelled) setLoadError(e instanceof Error ? e.message : String(e)); });
    return () => { cancelled = true; };
  }, [activeAvatar, loadAttempt]);

  const saveCard = async () => {
    if (!activeAvatar || !ready || saving) return;
    setSaving(true);
    let avatar = activeAvatar;
    try {
      if (draft.name?.trim() && draft.name.trim() !== ch?.name) {
        const renamed = await rpc.call<{ avatar: string }>('characters.rename', { avatar, name: draft.name.trim() });
        avatar = renamed.avatar;
        if (avatar !== activeAvatar) {
          // The backend already moved the chat files. Publish the new identity
          // before loadAll refreshes consumers such as the model selector.
          renamedDraft.current = avatar;
          setLoadedAvatar(avatar);
          if (memberAvatar) onAvatarChange?.(avatar);
          else useStore.setState({ activeAvatar: avatar });
        }
      }
      await rpc.call('characters.edit', { avatar, data: { ...draft, alternate_greetings: greetings,
        tags: tags.split(',').map((tag) => tag.trim()).filter(Boolean), extensions: { depth_prompt: depthPrompt } } });
      await loadAll();
      pushToast('角色卡已保存', 'success');
      setDirty(false);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      if (avatar !== activeAvatar) {
        const prefix = `nast:draft:${activeAvatar}:`;
        try {
          for (const key of Object.keys(localStorage).filter((key) => key.startsWith(prefix))) {
            localStorage.setItem(`nast:draft:${avatar}:${key.slice(prefix.length)}`, localStorage.getItem(key) ?? '');
          }
        } catch { pushToast('角色已重命名，但浏览器未能迁移草稿；原草稿仍保留在本地。', 'error'); }
        await loadAll().catch(() => {});
        await useStore.getState().reloadChat().catch(() => {});
      }
      setSaving(false);
    }
  };

  const exportCard = async (format: 'png' | 'json') => {
    if (!activeAvatar || saving) return;
    setSaving(true);
    try { downloadBase64(await rpc.call('characters.export', { avatar: activeAvatar, format })); }
    catch (e) { pushToast(e instanceof Error ? e.message : String(e), 'error'); }
    finally { setSaving(false); }
  };

  if (!ch) {
    return (
      <Empty className="gap-4 px-2 py-12 md:px-2">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <UserRound />
          </EmptyMedia>
          <EmptyTitle className="text-sm">认识你的角色</EmptyTitle>
          <EmptyDescription className="text-xs">
            选择一个角色后，在这里查看和编辑角色设定。
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  const field = (key: (typeof EDITABLE_FIELDS)[number]['key']) => {
    const f = EDITABLE_FIELDS.find((item) => item.key === key)!;
    const single = f.rows === 1;
    const props = {
      id: `char-field-${f.key}`,
      value: draft[f.key] ?? '',
      disabled: saving || !ready,
      onChange: (e: { target: { value: string } }) => { setDraft((d) => ({ ...d, [f.key]: e.target.value })); setDirty(true); },
    };
    return (
      <div key={f.key} className="flex flex-col gap-1.5">
        <Label htmlFor={props.id} className="text-xs font-medium text-muted-foreground">{f.label}</Label>
        {single ? <Input {...props} /> : <Textarea {...props} rows={Math.min(f.rows, 10)} className="min-h-0 resize-y leading-relaxed" />}
      </div>
    );
  };

  const sections = [
    { id: 'basic', label: '基础信息', hint: '名称、作者与标签' },
    { id: 'persona', label: '角色设定', hint: '描述、性格、场景' },
    { id: 'greeting', label: '开场白', hint: `首条消息与 ${greetings.length} 条备选` },
    { id: 'examples', label: '对话示例', hint: '示范语气与格式' },
    { id: 'prompts', label: '提示覆盖', hint: '系统提示、PHI、深度提示' },
    { id: 'notes', label: '作者说明', hint: '给使用者的备注' },
  ];
  const sectionClass = 'flex scroll-mt-4 flex-col gap-4 rounded-2xl border border-border/70 bg-card/70 p-5';
  const sectionTitle = (id: string) => {
    const sec = sections.find((x) => x.id === id)!;
    return <header><h3 className="font-display text-base font-semibold">{sec.label}</h3><p className="text-xs text-muted-foreground">{sec.hint}</p></header>;
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="relative overflow-hidden rounded-2xl border border-border/70 bg-card/80">
        <div aria-hidden className="h-16 bg-gradient-to-br from-primary/30 via-primary/10 to-transparent" />
        <div className="-mt-9 flex flex-col gap-3 px-4 pb-4">
          <Avatar className="size-16 rounded-2xl ring-4 ring-card">
            <AvatarImage src={ch.avatarUrl} alt={ch.name} className="object-cover" />
            <AvatarFallback className="rounded-2xl">{ch.name.slice(0, 1)}</AvatarFallback>
          </Avatar>
          <div className="min-w-0">
            <p className="truncate font-display text-lg font-semibold leading-tight" title={ch.name}>{ch.name}</p>
            {ch.description && <p className="mt-1 line-clamp-3 text-xs leading-relaxed text-muted-foreground">{ch.description}</p>}
          </div>
          {ch.tags.length > 0 && <div className="flex flex-wrap gap-1">
            {ch.tags.map((t) => <Badge key={t} variant="secondary" className="px-1.5 text-[10px]">{t}</Badge>)}
          </div>}
          {dirty && <p role="status" className="flex items-center gap-1.5 text-xs text-warning"><span className="size-1.5 rounded-full bg-warning" />有未保存的修改</p>}
          <div className="flex gap-2">
            <Button className="flex-1" disabled={!ready} onClick={() => setEditorOpen(true)}><PenSquare data-icon="inline-start" />编辑角色卡</Button>
            {dirty && <Button variant="outline" onClick={() => void saveCard()} disabled={saving}>{saving ? <Spinner /> : null}保存</Button>}
          </div>
        </div>
      </div>

      {loadError && <div role="alert" className="flex flex-col gap-2 text-sm text-destructive">
        <p>角色资料加载失败：{loadError}</p>
        <Button variant="outline" disabled={!connected} onClick={() => setLoadAttempt(value => value + 1)}>重试加载角色</Button>
      </div>}
      {!loadError && loadedAvatar !== activeAvatar && <p role="status" className="flex items-center gap-2 text-sm text-muted-foreground"><Spinner />正在加载角色资料…</p>}

      <div className="flex flex-col gap-2">
        <Label className="text-xs text-muted-foreground">导出</Label>
        <div className="grid grid-cols-2 gap-2">
          <Button variant="outline" size="sm" disabled={saving || dirty} onClick={() => void exportCard('png')}><FileImage data-icon="inline-start" />PNG</Button>
          <Button variant="outline" size="sm" disabled={saving || dirty} onClick={() => void exportCard('json')}><FileJson data-icon="inline-start" />JSON</Button>
        </div>
        {dirty && <p className="text-xs text-muted-foreground">先保存修改，再导出卡片。</p>}
      </div>

      <Dialog open={editorOpen} onOpenChange={setEditorOpen}>
        <DialogContent className="flex h-[min(90dvh,860px)] w-[calc(100%-1.5rem)] max-w-5xl flex-col gap-0 overflow-hidden p-0 sm:p-0">
          <DialogHeader className="flex-row items-center gap-3 space-y-0 border-b border-border/70 px-5 py-4 pr-12 text-left">
            <Avatar className="size-10 rounded-xl">
              <AvatarImage src={ch.avatarUrl} alt="" className="object-cover" />
              <AvatarFallback className="rounded-xl">{ch.name.slice(0, 1)}</AvatarFallback>
            </Avatar>
            <div className="min-w-0 flex-1">
              <DialogTitle className="truncate font-display text-lg">{draft.name || ch.name}</DialogTitle>
              <DialogDescription className="text-xs">编辑角色卡 · 修改在保存前只保存在本页</DialogDescription>
            </div>
          </DialogHeader>
          <div className="grid min-h-0 flex-1 md:grid-cols-[13rem_1fr]">
            <nav aria-label="角色卡分区" className="hidden flex-col gap-0.5 border-r border-border/70 p-3 md:flex">
              {sections.map((sec) => (
                <button key={sec.id} type="button" onClick={() => document.getElementById(`char-sec-${sec.id}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' })}
                  className="flex flex-col items-start rounded-lg px-3 py-2 text-left transition-colors hover:bg-accent">
                  <span className="text-sm font-medium">{sec.label}</span>
                  <span className="text-[11px] text-muted-foreground">{sec.hint}</span>
                </button>
              ))}
            </nav>
            <fieldset disabled={saving || !ready} className="min-h-0 min-w-0 overflow-y-auto p-4 sm:p-6">
              <div className="mx-auto flex max-w-3xl flex-col gap-5">
                <section id="char-sec-basic" className={sectionClass}>
                  {sectionTitle('basic')}
                  {field('name')}
                  <div className="grid gap-4 sm:grid-cols-2">{field('creator')}{field('character_version')}</div>
                  <div className="flex flex-col gap-1.5">
                    <Label htmlFor="char-tags" className="text-xs font-medium text-muted-foreground">标签（逗号分隔）</Label>
                    <Input id="char-tags" aria-label="角色标签" value={tags} disabled={saving} onChange={(event) => { setTags(event.target.value); setDirty(true); }} />
                  </div>
                </section>
                <section id="char-sec-persona" className={sectionClass}>
                  {sectionTitle('persona')}
                  {field('description')}{field('personality')}{field('scenario')}
                </section>
                <section id="char-sec-greeting" className={sectionClass}>
                  {sectionTitle('greeting')}
                  {field('first_mes')}
                  {greetings.map((greeting, index) => <div key={index} className="flex flex-col gap-1.5">
                    <div className="flex items-center justify-between">
                      <Label htmlFor={`char-greet-${index}`} className="text-xs font-medium text-muted-foreground">备选开场白 {index + 1}</Label>
                      <Button variant="ghost" size="sm" className="h-7 text-xs text-muted-foreground" disabled={saving} onClick={() => { setGreetings((values) => values.filter((_, i) => i !== index)); setDirty(true); }}>移除第 {index + 1} 条</Button>
                    </div>
                    <Textarea id={`char-greet-${index}`} aria-label={`备选开场白 ${index + 1}`} rows={4} disabled={saving} value={greeting} onChange={(event) => {
                      setGreetings((values) => values.map((value, i) => i === index ? event.target.value : value)); setDirty(true);
                    }} />
                  </div>)}
                  <Button variant="outline" size="sm" className="self-start" disabled={saving} onClick={() => { setGreetings((values) => [...values, '']); setDirty(true); }}><Plus data-icon="inline-start" />添加开场白</Button>
                </section>
                <section id="char-sec-examples" className={sectionClass}>
                  {sectionTitle('examples')}
                  {field('mes_example')}
                </section>
                <section id="char-sec-prompts" className={sectionClass}>
                  {sectionTitle('prompts')}
                  {field('system_prompt')}{field('post_history_instructions')}
                  <div className="flex flex-col gap-1.5">
                    <Label htmlFor="char-depth-prompt" className="text-xs font-medium text-muted-foreground">深度提示</Label>
                    <Textarea id="char-depth-prompt" aria-label="角色深度提示" rows={4} value={depthPrompt.prompt} disabled={saving}
                      onChange={(event) => { setDepthPrompt((value) => ({ ...value, prompt: event.target.value })); setDirty(true); }} />
                  </div>
                  <div className="grid gap-4 sm:grid-cols-2">
                    <div className="flex flex-col gap-1.5">
                      <Label htmlFor="char-depth" className="text-xs font-medium text-muted-foreground">插入深度</Label>
                      <Input id="char-depth" aria-label="角色提示插入深度" type="number" min={0} value={depthPrompt.depth} disabled={saving}
                        onChange={(event) => { setDepthPrompt((value) => ({ ...value, depth: Number(event.target.value) })); setDirty(true); }} />
                    </div>
                    <div className="flex flex-col gap-1.5">
                      <Label className="text-xs font-medium text-muted-foreground">身份</Label>
                      <Select value={depthPrompt.role} disabled={saving} onValueChange={(role) => { setDepthPrompt((value) => ({ ...value, role })); setDirty(true); }}>
                        <SelectTrigger aria-label="角色深度提示身份"><SelectValue /></SelectTrigger>
                        <SelectContent><SelectGroup>{['system', 'user', 'assistant'].map((role) => <SelectItem key={role} value={role}>{role}</SelectItem>)}</SelectGroup></SelectContent>
                      </Select>
                    </div>
                  </div>
                </section>
                <section id="char-sec-notes" className={sectionClass}>
                  {sectionTitle('notes')}
                  {field('creator_notes')}
                </section>
              </div>
            </fieldset>
          </div>
          <footer className="flex items-center gap-3 border-t border-border/70 bg-card/60 px-5 py-3">
            <p role="status" className={dirty ? 'flex flex-1 items-center gap-1.5 text-xs text-warning' : 'flex-1 text-xs text-muted-foreground'}>
              {dirty ? <><span className="size-1.5 rounded-full bg-warning" />有未保存的修改</> : '所有修改已保存'}
            </p>
            <Button variant="ghost" onClick={() => setEditorOpen(false)}>关闭</Button>
            <Button onClick={() => void saveCard()} disabled={saving || !ready || !dirty}>{saving ? <Spinner /> : <Save data-icon="inline-start" />}保存卡片</Button>
          </footer>
        </DialogContent>
      </Dialog>
    </div>
  );
}
