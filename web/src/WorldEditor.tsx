import { useEffect, useRef, useState } from 'react';
import { rpc } from './rpc';
import { downloadBlob } from '@/lib/download';
import { Field, FieldGroup, FieldLabel, FieldDescription } from '@/components/ui/field';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Switch } from '@/components/ui/switch';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Plus, Trash2 } from 'lucide-react';
import { useWorldStore, type WIEntry } from './worldStore';
import { pushToast } from './toasts';

const POSITIONS = [
  '角色描述前',
  '角色描述后',
  'AN 顶部',
  'AN 底部',
  '@D 深度',
  '示例前',
  '示例后',
  '自定义出口',
];
const LOGIC = ['AND_ANY', 'NOT_ALL', 'NOT_ANY', 'AND_ALL'];
const ROLES = ['system', 'user', 'assistant'];

export default function WorldEditor({ onClose }: { onClose: () => void }) {
  const { worlds, activeWorld, entries, loadWorlds, openWorld, saveWorld, createWorld, deleteWorld,
    updateEntry, addEntry, deleteEntry } = useWorldStore();
  const [newName, setNewName] = useState('');
  const [selected, setSelected] = useState('');
  const [search, setSearch] = useState('');
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const dirtyRef = useRef(false);
  const markDirty = (value: boolean) => { dirtyRef.current = value; setDirty(value); };
  const upload = useRef<HTMLInputElement>(null);

  useEffect(() => { void loadWorlds().catch((e) => pushToast(e.message, 'error')); }, [loadWorlds]);
  useEffect(() => {
    if (!entries[selected]) setSelected(Object.keys(entries)[0] ?? '');
  }, [entries, selected]);
  const work = async (action: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    try { await action(); }
    catch (e) { pushToast(e instanceof Error ? e.message : String(e), 'error'); }
    finally { setBusy(false); }
  };
  const flush = async () => {
    (document.activeElement as HTMLElement | null)?.blur();
    if (dirtyRef.current) { await saveWorld(); markDirty(false); }
  };
  const close = () => void work(async () => { await flush(); onClose(); });
  const filtered = Object.entries(entries).filter(([, e]) =>
    [e.comment, e.content, ...e.key].join(' ').toLocaleLowerCase().includes(search.toLocaleLowerCase()));
  const selectedEntry = entries[selected];

  return (
    <Dialog open onOpenChange={(open) => { if (!open) close(); }}>
      <DialogContent className="flex h-[88dvh] max-w-5xl flex-col overflow-hidden" aria-busy={busy}>
        <DialogHeader>
          <DialogTitle>世界书管理</DialogTitle>
          <DialogDescription>选择一条设定编辑。切换书籍或关闭时自动保存，导入与导出使用兼容 ST 的 JSON。</DialogDescription>
        </DialogHeader>
        <div className="flex flex-wrap items-center gap-2">
          <Select value={activeWorld ?? ''} onValueChange={(name) => void work(async () => { await flush(); await openWorld(name); })} disabled={busy}>
            <SelectTrigger className="w-48" aria-label="选择世界书"><SelectValue placeholder="选择世界书…" /></SelectTrigger>
            <SelectContent><SelectGroup>{worlds.map((name) => <SelectItem key={name} value={name}>{name}</SelectItem>)}</SelectGroup></SelectContent>
          </Select>
          <Input value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="新书名称" aria-label="新书名称" className="w-36" disabled={busy} />
          <Button variant="secondary" size="sm" disabled={busy || !newName.trim()} onClick={() => void work(async () => {
            await flush(); await createWorld(newName.trim()); setNewName('');
          })}><Plus data-icon="inline-start" />新建</Button>
          <Button variant="outline" size="sm" disabled={busy} onClick={() => upload.current?.click()}>导入</Button>
          <input ref={upload} type="file" accept=".json" className="hidden" aria-label="导入世界书" onChange={(event) => {
            const file = event.target.files?.[0]; event.target.value = '';
            if (!file) return;
            void work(async () => {
              const name = newName.trim() || file.name.replace(/\.json$/i, '');
              if ((await rpc.call<string[]>('worlds.list', {})).includes(name)) throw new Error('已有同名书。请填写新书名称后重新导入。');
              const book = JSON.parse(await file.text());
              if (!book.entries || Array.isArray(book.entries)) throw new Error('请选择 ST 世界书 JSON；卡内书请在角色面板导入。');
              await flush(); await rpc.call('worlds.save', { name, book });
              await loadWorlds(); await openWorld(name); setNewName(''); pushToast('世界书已导入', 'success');
            });
          }} />
          {activeWorld && <>
            <Button variant="outline" size="sm" disabled={busy} onClick={() => void work(async () => {
              await flush(); const book = await rpc.call('worlds.get', { name: activeWorld });
              downloadBlob(new Blob([JSON.stringify(book, null, 2)], { type: 'application/json' }), activeWorld + '.json');
            })}>导出</Button>
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => {
              if (window.confirm('删除“' + activeWorld + '”？角色中的关联不会自动替换。')) void work(async () => {
                await deleteWorld(activeWorld); markDirty(false);
              });
            }}>删除书籍</Button>
            <Button size="sm" disabled={busy || !dirty} onClick={() => void work(async () => { await flush(); pushToast('世界书已保存', 'success'); })}>
              {busy ? '处理中…' : dirty ? '保存修改' : '已保存'}
            </Button>
          </>}
        </div>
        <Separator />
        {activeWorld ? <div className="flex min-h-0 flex-1 flex-col gap-4 sm:flex-row">
          <div className="flex shrink-0 flex-col gap-2 sm:w-52">
            <Input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索条目…" aria-label="搜索世界书条目" />
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs text-muted-foreground">{filtered.length} 条设定</span>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => {
                const before = new Set(Object.keys(entries)); addEntry(); markDirty(true);
                setSelected(Object.keys(useWorldStore.getState().entries).find((key) => !before.has(key)) ?? '');
              }}><Plus data-icon="inline-start" />添加</Button>
            </div>
            <div className="flex max-h-28 gap-1 overflow-auto sm:max-h-none sm:flex-1 sm:flex-col" aria-label="世界书条目列表">
              {filtered.map(([uid, entry]) => <Button key={uid} variant={selected === uid ? 'secondary' : 'ghost'}
                className="shrink-0 justify-start sm:w-full" aria-pressed={selected === uid} disabled={busy}
                onClick={() => setSelected(uid)} title={entry.comment || entry.key.join(', ')}>
                <span className="truncate">{entry.disable ? '已禁用 · ' : ''}{entry.comment || entry.key.join(', ') || '未命名条目'}</span>
              </Button>)}
            </div>
          </div>
          <ScrollArea className="min-h-0 min-w-0 flex-1">
            <fieldset disabled={busy} className="min-w-0 pr-3">
              {selectedEntry ? <EntryCard key={activeWorld + '/' + selected} uid={selected} e={selectedEntry}
                onChange={(patch) => { updateEntry(selected, patch); markDirty(true); }}
                onDelete={() => { if (!busy) { deleteEntry(selected); markDirty(true); } }} /> :
                <p className="py-8 text-center text-sm text-muted-foreground">还没有条目，点击“添加”写下第一条设定。</p>}
            </fieldset>
          </ScrollArea>
        </div> : <p className="py-12 text-center text-sm text-muted-foreground">选择、新建或导入一本世界书。</p>}
      </DialogContent>
    </Dialog>
  );
}

function EntryCard({
  uid,
  e,
  onChange,
  onDelete,
}: {
  uid: string;
  e: WIEntry;
  onChange: (patch: Partial<WIEntry>) => void;
  onDelete: () => void;
}) {
  const posNum = typeof e.position === 'number' ? e.position : 0;

  return (
    <div className="flex min-w-0 flex-col gap-3 rounded-xl border bg-card p-3 sm:p-4">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={e.comment}
          onChange={(ev) => onChange({ comment: ev.target.value })}
          placeholder="标题/备注"
          className="h-9 min-w-0 basis-full sm:flex-1 sm:basis-auto"
        />
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Switch
            checked={e.disable}
            onCheckedChange={(v) => onChange({ disable: v })}
            aria-label="禁用条目"
          />
          禁用
        </div>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Switch
            checked={e.constant}
            onCheckedChange={(v) => onChange({ constant: v })}
            aria-label="常驻"
          />
          常驻
        </div>
        <Button variant="ghost" size="icon" className="ml-auto size-8 text-destructive" aria-label="删除世界书条目" onClick={onDelete}>
          <Trash2 />
        </Button>
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">主关键词（逗号分隔）</Label>
          <KeywordInput
            value={e.key}
            label="主关键词"
            onChange={(key) => onChange({ key })}
            className="h-8"
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">次级关键词</Label>
          <KeywordInput
            value={e.keysecondary}
            label="次级关键词"
            onChange={(keysecondary) => onChange({ keysecondary })}
            className="h-8"
          />
        </div>
      </div>
      <Textarea
        value={e.content}
        onChange={(ev) => onChange({ content: ev.target.value })}
        placeholder="内容（支持宏）"
        rows={8}
        className="min-h-0 resize-y"
      />
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">位置</Label>
          <Select
            value={String(posNum)}
            onValueChange={(v) => onChange({ position: Number(v) })}
          >
            <SelectTrigger className="h-8 w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent><SelectGroup>
              {POSITIONS.map((p, i) => (
                <SelectItem key={i} value={String(i)}>
                  {p}
                </SelectItem>
              ))}
            </SelectGroup></SelectContent>
          </Select>
        </div>
        {posNum === 4 && (
          <>
            <div className="flex flex-col gap-1">
              <Label className="text-xs text-muted-foreground">深度</Label>
              <Input
                type="number"
                value={e.depth}
                onChange={(ev) => onChange({ depth: Number(ev.target.value) })}
                className="h-8 w-20"
              />
            </div>
            <div className="flex flex-col gap-1">
              <Label className="text-xs text-muted-foreground">角色</Label>
              <Select
                value={String(e.role ?? 0)}
                onValueChange={(v) => onChange({ role: Number(v) })}
              >
                <SelectTrigger className="h-8 w-24">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent><SelectGroup>
                  {ROLES.map((r, i) => (
                    <SelectItem key={i} value={String(i)}>
                      {r}
                    </SelectItem>
                  ))}
                </SelectGroup></SelectContent>
              </Select>
            </div>
          </>
        )}
        {e.selective && e.keysecondary.length > 0 && (
          <div className="flex flex-col gap-1">
            <Label className="text-xs text-muted-foreground">次级逻辑</Label>
            <Select
              value={String(e.selectiveLogic)}
              onValueChange={(v) => onChange({ selectiveLogic: Number(v) })}
            >
              <SelectTrigger className="h-8 w-28">
                <SelectValue />
              </SelectTrigger>
              <SelectContent><SelectGroup>
                {LOGIC.map((l, i) => (
                  <SelectItem key={i} value={String(i)}>
                    {l}
                  </SelectItem>
                ))}
              </SelectGroup></SelectContent>
            </Select>
          </div>
        )}
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">顺序</Label>
          <Input
            type="number"
            value={e.order}
            onChange={(ev) => onChange({ order: Number(ev.target.value) })}
            className="h-8 w-20"
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">概率 %</Label>
          <Input
            type="number"
            value={e.probability}
            onChange={(ev) => onChange({ probability: Number(ev.target.value) })}
            className="h-8 w-20"
          />
        </div>
        <NumField label="驻留" value={e.sticky} onChange={(v) => onChange({ sticky: v })} />
        <NumField label="冷却" value={e.cooldown} onChange={(v) => onChange({ cooldown: v })} />
        <NumField label="延迟" value={e.delay} onChange={(v) => onChange({ delay: v })} />
        <div className="flex items-center gap-1.5 pb-1.5 text-xs text-muted-foreground">
          <Switch
            checked={e.preventRecursion}
            onCheckedChange={(v) => onChange({ preventRecursion: v })}
            aria-label="阻止递归"
          />
          阻止递归
        </div>
        <div className="flex items-center gap-1.5 pb-1.5 text-xs text-muted-foreground">
          <Switch
            checked={e.ignoreBudget}
            onCheckedChange={(v) => onChange({ ignoreBudget: v })}
            aria-label="忽略预算"
          />
          忽略预算
        </div>
      </div>
      <AdvancedEntry e={e} onChange={onChange} />
    </div>
  );
}

function AdvancedEntry({ e, onChange }: { e: WIEntry; onChange: (patch: Partial<WIEntry>) => void }) {
  const flags = [
    ['selective', '启用次级关键词'], ['useProbability', '启用概率'],
    ['excludeRecursion', '不由递归激活'], ['groupOverride', '组内优先'],
    ['matchPersonaDescription', '扫描用户设定'], ['matchCharacterDescription', '扫描角色描述'],
    ['matchCharacterPersonality', '扫描角色性格'], ['matchCharacterDepthPrompt', '扫描角色深度提示'],
    ['matchScenario', '扫描场景'], ['matchCreatorNotes', '扫描作者说明'],
  ];
  return <details className="border-t pt-3">
    <summary className="cursor-pointer text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">高级激活规则</summary>
    <FieldGroup className="mt-4 gap-4">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field><FieldLabel>扫描深度</FieldLabel><Input type="number" min={0} max={1000} aria-label="扫描深度" placeholder="跟随全局"
          value={e.scanDepth == null ? '' : Number(e.scanDepth)} onChange={(event) => onChange({ scanDepth: event.target.value === '' ? null : Number(event.target.value) })} /></Field>
        <Field><FieldLabel>递归起始层级</FieldLabel><Input type="number" min={0} aria-label="递归起始层级" value={Number(e.delayUntilRecursion)}
          onChange={(event) => onChange({ delayUntilRecursion: Number(event.target.value) })} /><FieldDescription>0 表示无需等待递归。</FieldDescription></Field>
        <Field><FieldLabel>互斥组</FieldLabel><Input aria-label="互斥组" value={e.group} onChange={(event) => onChange({ group: event.target.value })} /></Field>
        <Field><FieldLabel>组内权重</FieldLabel><Input type="number" min={0} aria-label="组内权重" value={e.groupWeight}
          onChange={(event) => onChange({ groupWeight: Number(event.target.value) })} /></Field>
        {([['caseSensitive', '区分大小写'], ['matchWholeWords', '全词匹配'], ['useGroupScoring', '组内关键词计分']] as const).map(([key, label]) =>
          <Field key={key}><FieldLabel>{label}</FieldLabel><Select value={e[key] == null ? 'inherit' : String(e[key])}
            onValueChange={(value) => onChange({ [key]: value === 'inherit' ? null : value === 'true' })}>
            <SelectTrigger aria-label={label}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>
              <SelectItem value="inherit">跟随全局</SelectItem><SelectItem value="true">启用</SelectItem><SelectItem value="false">关闭</SelectItem>
            </SelectGroup></SelectContent></Select></Field>)}
        {e.position === 7 && <Field><FieldLabel>出口名称</FieldLabel><Input aria-label="出口名称" value={String(e.outletName ?? '')}
          onChange={(event) => onChange({ outletName: event.target.value })} /><FieldDescription>在提示词中使用 {'{{outlet::名称}}'} 引用。</FieldDescription></Field>}
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">{flags.map(([key, label]) =>
        <label key={key} className="flex items-center gap-2 text-xs"><Switch aria-label={label} checked={Boolean(e[key])}
          onCheckedChange={(value) => onChange({ [key]: value })} />{label}</label>)}</div>
      <Field><FieldLabel>生成类型</FieldLabel><FieldDescription>不勾选时适用于所有类型。</FieldDescription>
        <div className="flex flex-wrap gap-3">{['normal', 'regenerate', 'swipe', 'continue', 'quiet', 'impersonate'].map((trigger) => {
          const triggers = Array.isArray(e.triggers) ? e.triggers as string[] : [];
          return <label key={trigger} className="flex items-center gap-1.5 text-xs"><input type="checkbox" checked={triggers.includes(trigger)}
            onChange={(event) => onChange({ triggers: event.target.checked ? [...triggers, trigger] : triggers.filter((value) => value !== trigger) })} />{trigger}</label>;
        })}</div>
      </Field>
      <FieldDescription>向量检索与 STScript 自动化字段会保留，当前不执行。</FieldDescription>
    </FieldGroup>
  </details>;
}

function NumField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex flex-col gap-1">
      <Label className="text-xs text-muted-foreground">{label}</Label>
      <Input
        type="number"
        value={value}
        onChange={(ev) => onChange(Number(ev.target.value))}
        className="h-8 w-20"
      />
    </div>
  );
}


function KeywordInput({ value, label, onChange, className }: { value: string[]; label: string; onChange: (value: string[]) => void; className?: string }) {
  const [text, setText] = useState(value.join(', '));
  return <Input className={className} aria-label={label} value={text} onChange={(event) => setText(event.target.value)}
    onBlur={() => onChange(text.split(',').map((item) => item.trim()).filter(Boolean))} />;
}
