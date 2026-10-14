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
import { BookOpen, Download, Plus, Save, Search, Trash2, Upload } from 'lucide-react';
import { Spinner } from '@/components/ui/spinner';
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

  const importInput = <input ref={upload} type="file" accept=".json" className="hidden" aria-label="导入世界书" onChange={(event) => {
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
  }} />;
  const entryCount = Object.keys(entries).length;

  return (
    <Dialog open onOpenChange={(open) => { if (!open) close(); }}>
      <DialogContent className="flex h-[min(92dvh,900px)] w-[calc(100%-1.5rem)] max-w-6xl flex-col gap-0 overflow-hidden p-0 sm:p-0" aria-busy={busy}>
        <DialogHeader className="flex-row flex-wrap items-center gap-x-4 gap-y-2 space-y-0 border-b border-border/70 px-5 py-4 pr-12 text-left">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-accent text-primary"><BookOpen className="size-5" /></div>
          <div className="min-w-0 flex-1">
            <DialogTitle className="font-display text-lg">世界书</DialogTitle>
            <DialogDescription className="text-xs">切换书籍或关闭时自动保存；导入与导出使用兼容 ST 的 JSON。</DialogDescription>
          </div>
          {activeWorld && <div className="flex flex-wrap items-center gap-1.5">
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => void work(async () => {
              await flush(); const book = await rpc.call('worlds.get', { name: activeWorld });
              downloadBlob(new Blob([JSON.stringify(book, null, 2)], { type: 'application/json' }), activeWorld + '.json');
            })}><Download data-icon="inline-start" />导出</Button>
            <Button variant="ghost" size="sm" className="text-destructive hover:text-destructive" disabled={busy} onClick={() => {
              if (window.confirm('删除“' + activeWorld + '”？角色中的关联不会自动替换。')) void work(async () => {
                await deleteWorld(activeWorld); markDirty(false);
              });
            }}><Trash2 data-icon="inline-start" />删除书籍</Button>
            <Button size="sm" disabled={busy || !dirty} onClick={() => void work(async () => { await flush(); pushToast('世界书已保存', 'success'); })}>
              {busy ? <Spinner /> : <Save data-icon="inline-start" />}{busy ? '处理中…' : dirty ? '保存修改' : '已保存'}
            </Button>
          </div>}
        </DialogHeader>

        <div className="grid min-h-0 flex-1 grid-rows-[auto_1fr] md:grid-cols-[18rem_1fr] md:grid-rows-1">
          <aside className="flex min-h-0 flex-col gap-3 border-b border-border/70 bg-muted/30 p-3 md:border-r md:border-b-0">
            <Select value={activeWorld ?? ''} onValueChange={(name) => void work(async () => { await flush(); await openWorld(name); })} disabled={busy}>
              <SelectTrigger className="w-full bg-card/70" aria-label="选择世界书"><SelectValue placeholder="选择世界书…" /></SelectTrigger>
              <SelectContent><SelectGroup>{worlds.map((name) => <SelectItem key={name} value={name}>{name}</SelectItem>)}</SelectGroup></SelectContent>
            </Select>
            <div className="flex gap-1.5">
              <Input value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="新书名称" aria-label="新书名称" className="h-control-sm min-w-0 flex-1 text-sm" disabled={busy} />
              <Button variant="secondary" size="sm" disabled={busy || !newName.trim()} onClick={() => void work(async () => {
                await flush(); await createWorld(newName.trim()); setNewName('');
              })}><Plus data-icon="inline-start" />新建</Button>
              <Button variant="outline" size="icon-sm" disabled={busy} onClick={() => upload.current?.click()} aria-label="导入世界书" title="导入"><Upload /></Button>
              {importInput}
            </div>
            {activeWorld && <>
              <Separator className="bg-border/70" />
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索标题、关键词、内容…" aria-label="搜索世界书条目" className="h-control-sm pl-8 text-sm" />
              </div>
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs text-muted-foreground">{filtered.length === entryCount ? `${entryCount} 条设定` : `${filtered.length} / ${entryCount} 条`}</span>
                <Button size="sm" variant="ghost" className="h-7" disabled={busy} onClick={() => {
                  const before = new Set(Object.keys(entries)); addEntry(); markDirty(true);
                  setSelected(Object.keys(useWorldStore.getState().entries).find((key) => !before.has(key)) ?? '');
                }}><Plus data-icon="inline-start" />添加条目</Button>
              </div>
              <div className="-mx-1 flex max-h-40 min-h-0 flex-col gap-0.5 overflow-y-auto px-1 md:max-h-none md:flex-1" aria-label="世界书条目列表">
                {filtered.map(([uid, entry]) => <button type="button" key={uid} aria-pressed={selected === uid} disabled={busy}
                  onClick={() => setSelected(uid)} title={entry.comment || entry.key.join(', ')}
                  className={'flex w-full flex-col items-start gap-0.5 rounded-lg px-3 py-2 text-left transition-colors ' + (selected === uid ? 'bg-card shadow-sm ring-1 ring-primary/25' : 'hover:bg-card/60')}>
                  <span className="flex w-full items-center gap-2">
                    <span className={'size-1.5 shrink-0 rounded-full ' + (entry.disable ? 'bg-muted-foreground/40' : entry.constant ? 'bg-primary' : 'bg-success')} aria-hidden />
                    <span className={'truncate text-sm font-medium ' + (entry.disable ? 'text-muted-foreground line-through' : '')}>{entry.disable ? '已禁用 · ' : ''}{entry.comment || entry.key.join(', ') || '未命名条目'}</span>
                    {entry.constant && <span className="ml-auto shrink-0 rounded bg-primary/12 px-1.5 text-[10px] text-primary">常驻</span>}
                  </span>
                  <span className="w-full truncate pl-3.5 text-[11px] text-muted-foreground">{entry.key.length ? entry.key.join(' · ') : '无关键词'}</span>
                </button>)}
                {filtered.length === 0 && <p className="px-3 py-6 text-center text-xs text-muted-foreground">{entryCount ? '没有匹配的条目' : '这本书还是空的'}</p>}
              </div>
            </>}
          </aside>

          <section className="min-h-0 min-w-0">
            {activeWorld ? <ScrollArea className="h-full">
              <fieldset disabled={busy} className="mx-auto min-w-0 max-w-3xl p-4 sm:p-6">
                {selectedEntry ? <EntryCard key={activeWorld + '/' + selected} uid={selected} e={selectedEntry}
                  onChange={(patch) => { updateEntry(selected, patch); markDirty(true); }}
                  onDelete={() => { if (!busy) { deleteEntry(selected); markDirty(true); } }} /> :
                  <WorldEmpty title="还没有选中条目" text="点击“添加条目”写下第一条设定，或从左侧选择一条。" />}
              </fieldset>
            </ScrollArea> : <WorldEmpty title="打开一本世界书" text="在左侧选择、新建或导入一本世界书，让角色记住这个世界的地名、规则与历史。" />}
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function WorldEmpty({ title, text }: { title: string; text: string }) {
  return <div className="flex h-full min-h-64 flex-col items-center justify-center gap-3 p-8 text-center">
    <div className="welcome-mark size-14"><BookOpen className="size-6" strokeWidth={1.5} /></div>
    <p className="font-display text-lg font-semibold">{title}</p>
    <p className="max-w-sm text-sm text-muted-foreground">{text}</p>
  </div>;
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
    <div className="flex min-w-0 animate-rise-in flex-col gap-5">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={e.comment}
          onChange={(ev) => onChange({ comment: ev.target.value })}
          placeholder="标题/备注"
          aria-label="条目标题"
          className="h-control-lg min-w-0 basis-full border-transparent bg-transparent px-0 font-display text-xl font-semibold shadow-none focus-visible:border-transparent focus-visible:ring-0 sm:flex-1 sm:basis-auto"
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
        aria-label="条目内容"
        rows={10}
        className="min-h-0 resize-y rounded-xl bg-card/70 p-4 leading-relaxed"
      />
      <div className="flex flex-wrap items-end gap-3 rounded-xl border border-border/70 bg-card/50 p-4">
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
  return <details className="rounded-xl border border-border/70 bg-card/50 p-4">
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
