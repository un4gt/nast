import { useEffect, useRef, useState } from 'react';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { downloadBase64 } from '@/lib/download';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { Separator } from '@/components/ui/separator';
import { Label } from '@/components/ui/label';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { UserRound } from 'lucide-react';
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion';
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

export function CharacterPanel() {
  const { characters, activeAvatar, loadAll } = useStore();
  const ch = characters.find((c) => c.avatar === activeAvatar);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [greetings, setGreetings] = useState<string[]>([]);
  const [tags, setTags] = useState('');
  const [depthPrompt, setDepthPrompt] = useState({ prompt: '', depth: 4, role: 'system' });
  const renamedDraft = useRef<string | null>(null);

  useEffect(() => {
    if (renamedDraft.current === activeAvatar) { renamedDraft.current = null; return; }
    let cancelled = false;
    setDraft({});
    setDirty(false);
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
      })
      .catch((e) => { if (!cancelled) pushToast(e.message, 'error'); });
    return () => { cancelled = true; };
  }, [activeAvatar]);

  const saveCard = async () => {
    if (!activeAvatar || saving) return;
    setSaving(true);
    let avatar = activeAvatar;
    try {
      if (draft.name?.trim() && draft.name.trim() !== ch?.name) {
        const renamed = await rpc.call<{ avatar: string }>('characters.rename', { avatar, name: draft.name.trim() });
        avatar = renamed.avatar;
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
        renamedDraft.current = avatar;
        useStore.setState({ activeAvatar: avatar });
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

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
        <div className="flex items-center gap-3">
          <Avatar className="size-12 shrink-0 rounded-xl">
            <AvatarImage src={ch.avatarUrl} alt={ch.name} />
            <AvatarFallback className="rounded-xl">{ch.name.slice(0, 1)}</AvatarFallback>
          </Avatar>
          <div className="min-w-0">
            <p className="truncate text-sm font-semibold" title={ch.name}>
              {ch.name}
            </p>
            <p className="mt-1 text-xs text-muted-foreground">角色设定</p>
          </div>
        </div>
        {dirty && (
          <Button
            size="sm"
            className="h-6 px-2 text-xs"
            onClick={() => void saveCard()}
            disabled={saving}
          >
            {saving ? <Spinner /> : null}
            保存卡片
          </Button>
        )}
        <div className="flex flex-wrap gap-1">
          {ch.tags.map((t) => (
            <Badge key={t} variant="secondary" className="px-1.5 text-[10px]">
              {t}
            </Badge>
          ))}
        </div>
      </div>

      <Separator />

      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={saving || dirty} onClick={() => void exportCard('png')}>导出 PNG</Button>
        <Button variant="outline" size="sm" disabled={saving || dirty} onClick={() => void exportCard('json')}>导出 JSON</Button>
      </div>
      {dirty && <p className="text-xs text-muted-foreground">先保存修改，再导出卡片。</p>}

      <div>
        <Label className="mb-2 block text-xs text-muted-foreground">卡片编辑</Label>
        <Accordion type="multiple" defaultValue={['description']} className="w-full">
          {EDITABLE_FIELDS.map((f) => (
            <AccordionItem key={f.key} value={f.key} className="border-b-0">
              <AccordionTrigger className="py-2 text-xs hover:no-underline">
                {f.label}
              </AccordionTrigger>
              <AccordionContent>
                <Textarea
                  value={draft[f.key] ?? ''}
                  onChange={(e) => {
                    setDraft((d) => ({ ...d, [f.key]: e.target.value }));
                    setDirty(true);
                  }}
                  rows={f.rows}
                  aria-label={f.label}
                  disabled={saving}
                  className="min-h-0 resize-y text-xs"
                />
              </AccordionContent>
            </AccordionItem>
          ))}
          <AccordionItem value="alternate-greetings" className="border-b-0">
            <AccordionTrigger className="py-2 text-xs">备选开场白 · {greetings.length}</AccordionTrigger>
            <AccordionContent className="flex flex-col gap-3">
              {greetings.map((greeting, index) => <div key={index} className="flex flex-col gap-1">
                <Textarea aria-label={`备选开场白 ${index + 1}`} rows={4} disabled={saving} value={greeting} onChange={(event) => {
                  setGreetings((values) => values.map((value, i) => i === index ? event.target.value : value)); setDirty(true);
                }} />
                <Button variant="ghost" size="sm" className="self-end" disabled={saving} onClick={() => {
                  setGreetings((values) => values.filter((_, i) => i !== index)); setDirty(true);
                }}>移除第 {index + 1} 条</Button>
              </div>)}
              <Button variant="outline" size="sm" disabled={saving} onClick={() => { setGreetings((values) => [...values, '']); setDirty(true); }}>添加开场白</Button>
            </AccordionContent>
          </AccordionItem>
          <AccordionItem value="depth-prompt" className="border-b-0">
            <AccordionTrigger className="py-2 text-xs">深度提示与标签</AccordionTrigger>
            <AccordionContent className="flex flex-col gap-3">
              <label className="text-xs">标签（逗号分隔）<Input aria-label="角色标签" value={tags} disabled={saving}
                onChange={(event) => { setTags(event.target.value); setDirty(true); }} /></label>
              <label className="text-xs">深度提示<Textarea aria-label="角色深度提示" rows={5} value={depthPrompt.prompt} disabled={saving}
                onChange={(event) => { setDepthPrompt((value) => ({ ...value, prompt: event.target.value })); setDirty(true); }} /></label>
              <label className="text-xs">插入深度<Input aria-label="角色提示插入深度" type="number" min={0} value={depthPrompt.depth} disabled={saving}
                onChange={(event) => { setDepthPrompt((value) => ({ ...value, depth: Number(event.target.value) })); setDirty(true); }} /></label>
              <Select value={depthPrompt.role} disabled={saving} onValueChange={(role) => { setDepthPrompt((value) => ({ ...value, role })); setDirty(true); }}>
                <SelectTrigger aria-label="角色深度提示身份"><SelectValue /></SelectTrigger>
                <SelectContent><SelectGroup>{['system', 'user', 'assistant'].map((role) => <SelectItem key={role} value={role}>{role}</SelectItem>)}</SelectGroup></SelectContent>
              </Select>
            </AccordionContent>
          </AccordionItem>
        </Accordion>
      </div>
    </div>
  );
}
