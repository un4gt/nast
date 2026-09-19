import { useEffect, useState } from 'react';
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
];
const LOGIC = ['AND_ANY', 'NOT_ALL', 'NOT_ANY', 'AND_ALL'];
const ROLES = ['system', 'user', 'assistant'];

export default function WorldEditor({ onClose }: { onClose: () => void }) {
  const {
    worlds, activeWorld, entries,
    loadWorlds, openWorld, saveWorld, createWorld, deleteWorld,
    updateEntry, addEntry, deleteEntry,
  } = useWorldStore();
  const [newName, setNewName] = useState('');

  useEffect(() => {
    loadWorlds();
  }, []);

  const doSave = async () => {
    try {
      await saveWorld();
      pushToast(`已保存 ${activeWorld}`, 'success');
    } catch (e) {
      pushToast((e as Error).message, 'error');
    }
  };

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-4xl h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>世界书管理</DialogTitle>
          <DialogDescription>
            条目文件与 SillyTavern worlds/*.json 直接兼容
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-wrap items-center gap-2">
          <Select
            value={activeWorld ?? ''}
            onValueChange={(v) => openWorld(v)}
          >
            <SelectTrigger className="w-48">
              <SelectValue placeholder="选择世界书…" />
            </SelectTrigger>
            <SelectContent>
              {worlds.map((w) => (
                <SelectItem key={w} value={w}>
                  {w}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="新书名"
            className="w-36"
          />
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              if (newName.trim()) createWorld(newName.trim());
              setNewName('');
            }}
          >
            <Plus data-icon="inline-start" />
            新建
          </Button>
          {activeWorld && (
            <Button
              variant="destructive"
              size="sm"
              onClick={() => deleteWorld(activeWorld)}
            >
              <Trash2 data-icon="inline-start" />
              删除
            </Button>
          )}
          <div className="flex-1" />
          {activeWorld && (
            <Button size="sm" onClick={doSave}>
              保存
            </Button>
          )}
        </div>

        {activeWorld && (
          <>
            <Separator />
            <div className="flex items-center">
              <Button size="sm" variant="outline" onClick={addEntry}>
                <Plus data-icon="inline-start" />
                添加条目
              </Button>
            </div>
            <ScrollArea className="flex-1 min-h-0 -mx-2 px-2">
              <div className="flex flex-col gap-3 pb-2">
                {Object.entries(entries).map(([uid, e]) => (
                  <EntryCard
                    key={uid}
                    uid={uid}
                    e={e}
                    onChange={(patch) => updateEntry(uid, patch)}
                    onDelete={() => deleteEntry(uid)}
                  />
                ))}
                {Object.keys(entries).length === 0 && (
                  <div className="py-8 text-center text-sm text-muted-foreground">
                    此书暂无条目
                  </div>
                )}
              </div>
            </ScrollArea>
          </>
        )}
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
  const keys = (arr: string[]) => arr.join(', ');
  const parseKeys = (s: string) =>
    s.split(',').map((x) => x.trim()).filter(Boolean);

  return (
    <div className="rounded-lg border bg-card p-3 flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Input
          value={e.comment}
          onChange={(ev) => onChange({ comment: ev.target.value })}
          placeholder="标题/备注"
          className="h-8 flex-1"
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
        <Button variant="ghost" size="icon" className="size-8 text-destructive" onClick={onDelete}>
          <Trash2 />
        </Button>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">主关键词（逗号分隔）</Label>
          <Input
            value={keys(e.key)}
            onChange={(ev) => onChange({ key: parseKeys(ev.target.value) })}
            className="h-8"
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">次级关键词</Label>
          <Input
            value={keys(e.keysecondary)}
            onChange={(ev) => onChange({ keysecondary: parseKeys(ev.target.value) })}
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
            <SelectContent>
              {POSITIONS.map((p, i) => (
                <SelectItem key={i} value={String(i)}>
                  {p}
                </SelectItem>
              ))}
            </SelectContent>
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
                <SelectContent>
                  {ROLES.map((r, i) => (
                    <SelectItem key={i} value={String(i)}>
                      {r}
                    </SelectItem>
                  ))}
                </SelectContent>
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
              <SelectContent>
                {LOGIC.map((l, i) => (
                  <SelectItem key={i} value={String(i)}>
                    {l}
                  </SelectItem>
                ))}
              </SelectContent>
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
    </div>
  );
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
