import { useEffect, useMemo, useState } from 'react';
import {
  DndContext, closestCenter, KeyboardSensor, PointerSensor, useSensor, useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { GripVertical, Pencil, Plus, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import { SliderField, SwitchField } from '../fields';

/** 12 个 marker（PromptManager.js markerPrompts）。 */
const MARKERS = new Set([
  'main', 'nsfw', 'dialogueExamples', 'jailbreak', 'chatHistory', 'worldInfoAfter',
  'worldInfoBefore', 'enhanceDefinitions', 'charDescription', 'charPersonality',
  'scenario', 'personaDescription',
]);

const CC_DUMMY_ID = 100001;

interface PromptEntry {
  identifier: string;
  name: string;
  role: string;
  content: string;
  marker: boolean;
  injection_position: number;
  injection_depth: number;
  injection_order: number;
  forbid_overrides: boolean;
  [k: string]: unknown;
}

interface OrderRow {
  identifier: string;
  enabled: boolean;
}

function SortableRow({
  row, prompt, onToggle, onEdit, onDelete,
}: {
  row: OrderRow;
  prompt?: PromptEntry;
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: row.identifier,
  });
  const isMarker = prompt?.marker || MARKERS.has(row.identifier);
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={
        'flex items-center gap-2 rounded-md border bg-card px-2 py-1.5 ' +
        (isDragging ? 'opacity-50' : '')
      }
    >
      <button className="cursor-grab touch-none text-muted-foreground" {...attributes} {...listeners}>
        <GripVertical className="size-3.5" />
      </button>
      <Switch checked={row.enabled} onCheckedChange={onToggle} className="scale-90" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="truncate text-xs">{prompt?.name ?? row.identifier}</span>
          {isMarker && <Badge variant="outline" className="px-1 text-[9px]">marker</Badge>}
        </div>
        {!isMarker && prompt?.content && (
          <p className="truncate text-[10px] text-muted-foreground">{prompt.content}</p>
        )}
      </div>
      {!isMarker && (
        <>
          <Button variant="ghost" size="icon" className="size-6" onClick={onEdit} title="编辑">
            <Pencil className="size-3" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-6 text-muted-foreground hover:text-destructive"
            onClick={onDelete}
            title="删除"
          >
            <Trash2 className="size-3" />
          </Button>
        </>
      )}
      {isMarker && (
        <Button variant="ghost" size="icon" className="size-6" onClick={onEdit} title="查看">
          <Pencil className="size-3 opacity-50" />
        </Button>
      )}
    </div>
  );
}

export function PromptManagerPanel({
  oai,
  patchOai,
}: {
  oai: any;
  patchOai: (k: string, v: unknown) => void;
}) {
  const prompts: PromptEntry[] = oai.prompts ?? [];
  const orderRows: OrderRow[] = useMemo(
    () =>
      (oai.prompt_order ?? []).find((r: any) => r.character_id === CC_DUMMY_ID)?.order ?? [],
    [oai.prompt_order],
  );
  const [editing, setEditing] = useState<string | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const promptMap = useMemo(() => {
    const m = new Map<string, PromptEntry>();
    prompts.forEach((p) => m.set(p.identifier, p));
    return m;
  }, [prompts]);

  const setOrder = (rows: OrderRow[]) => {
    const po = Array.isArray(oai.prompt_order) ? [...oai.prompt_order] : [];
    const idx = po.findIndex((r: any) => r.character_id === CC_DUMMY_ID);
    if (idx >= 0) po[idx] = { ...po[idx], order: rows };
    else po.push({ character_id: CC_DUMMY_ID, order: rows });
    patchOai('prompt_order', po);
  };

  const setPrompts = (next: PromptEntry[]) => patchOai('prompts', next);

  const onDragEnd = (e: DragEndEvent) => {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    const oldIdx = orderRows.findIndex((r) => r.identifier === active.id);
    const newIdx = orderRows.findIndex((r) => r.identifier === over.id);
    if (oldIdx < 0 || newIdx < 0) return;
    const next = [...orderRows];
    next.splice(newIdx, 0, ...next.splice(oldIdx, 1));
    setOrder(next);
  };

  const addPrompt = () => {
    const id = `custom-${Date.now().toString(36)}-${Math.floor(Math.random() * 1e4).toString(36)}`;
    setPrompts([
      ...prompts,
      {
        identifier: id,
        name: '新提示词',
        role: 'system',
        content: '',
        marker: false,
        injection_position: 0,
        injection_depth: 4,
        injection_order: 100,
        forbid_overrides: false,
      },
    ]);
    setOrder([...orderRows, { identifier: id, enabled: true }]);
    setEditing(id);
  };

  const deletePrompt = (id: string) => {
    setPrompts(prompts.filter((p) => p.identifier !== id));
    setOrder(orderRows.filter((r) => r.identifier !== id));
  };

  const editingPrompt = prompts.find((p) => p.identifier === editing);
  useEffect(() => {
    if (editing && !editingPrompt) setEditing(null);
  }, [editing, editingPrompt]);

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="mb-1 text-sm font-semibold">Prompt Manager</h3>
        <p className="text-xs text-muted-foreground">
          全局提示词顺序（prompt_order #100001）。拖拽排序；marker 为动态注入位。
          随「保存」写入 settings.json。
        </p>
      </div>

      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
        <SortableContext
          items={orderRows.map((r) => r.identifier)}
          strategy={verticalListSortingStrategy}
        >
          <div className="flex flex-col gap-1.5">
            {orderRows.map((row) => (
              <SortableRow
                key={row.identifier}
                row={row}
                prompt={promptMap.get(row.identifier)}
                onToggle={() =>
                  setOrder(
                    orderRows.map((r) =>
                      r.identifier === row.identifier ? { ...r, enabled: !r.enabled } : r,
                    ),
                  )
                }
                onEdit={() => setEditing(row.identifier)}
                onDelete={() => deletePrompt(row.identifier)}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>

      <Button variant="secondary" size="sm" className="self-start" onClick={addPrompt}>
        <Plus className="size-3.5" />
        新增提示词
      </Button>

      <PromptEditDialog
        prompt={editingPrompt ?? null}
        onChange={(next) => setPrompts(prompts.map((p) => (p.identifier === next.identifier ? next : p)))}
        open={!!editingPrompt}
        onOpenChange={(v) => {
          if (!v) setEditing(null);
        }}
      />
    </div>
  );
}

function PromptEditDialog({
  prompt,
  onChange,
  open,
  onOpenChange,
}: {
  prompt: PromptEntry | null;
  onChange: (p: PromptEntry) => void;
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const [draft, setDraft] = useState<PromptEntry | null>(prompt);
  useEffect(() => setDraft(prompt), [prompt]);
  if (!draft) return null;
  const isMarker = draft.marker || MARKERS.has(draft.identifier);
  const set = (patch: Partial<PromptEntry>) => setDraft({ ...draft, ...patch });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{isMarker ? '查看 Marker' : '编辑提示词'}</DialogTitle>
          <DialogDescription>
            {isMarker
              ? 'marker 的内容由拼装引擎动态填充；可改名称。'
              : '自定义提示词：内容、角色与注入位置。'}
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label>名称</Label>
            <Input value={draft.name} onChange={(e) => set({ name: e.target.value })} />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>角色</Label>
            <Select value={draft.role || 'system'} onValueChange={(v) => set({ role: v })}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="system">system</SelectItem>
                <SelectItem value="user">user</SelectItem>
                <SelectItem value="assistant">assistant</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {!isMarker && (
            <>
              <div className="flex flex-col gap-1.5">
                <Label>内容</Label>
                <Textarea
                  value={draft.content}
                  onChange={(e) => set({ content: e.target.value })}
                  rows={6}
                  className="min-h-0 resize-y text-xs"
                  placeholder="支持 {{char}}/{{user}} 宏…"
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>注入位置</Label>
                <Select
                  value={String(draft.injection_position ?? 0)}
                  onValueChange={(v) => set({ injection_position: Number(v) })}
                >
                  <SelectTrigger><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">相对（按顺序放置）</SelectItem>
                    <SelectItem value="1">绝对（聊天内深度）</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              {Number(draft.injection_position ?? 0) === 1 && (
                <>
                  <SliderField
                    label="注入深度"
                    value={draft.injection_depth ?? 4}
                    min={0}
                    max={16}
                    step={1}
                    onChange={(v) => set({ injection_depth: v })}
                  />
                  <div className="flex flex-col gap-1.5">
                    <Label>注入角色</Label>
                    <Select
                      value={String(draft.injection_role ?? 0)}
                      onValueChange={(v) => set({ injection_role: Number(v) })}
                    >
                      <SelectTrigger><SelectValue /></SelectTrigger>
                      <SelectContent>
                        <SelectItem value="0">system</SelectItem>
                        <SelectItem value="1">user</SelectItem>
                        <SelectItem value="2">assistant</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </>
              )}
              <div className="flex flex-col gap-1.5">
                <Label>注入顺序（injection_order）</Label>
                <Input
                  type="number"
                  value={draft.injection_order ?? 100}
                  onChange={(e) => set({ injection_order: Number(e.target.value) || 100 })}
                />
              </div>
              <SwitchField
                label="禁止角色卡覆盖"
                checked={!!draft.forbid_overrides}
                onChange={(v) => set({ forbid_overrides: v })}
              />
            </>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>取消</Button>
          <Button
            onClick={() => {
              onChange(draft);
              onOpenChange(false);
            }}
          >
            确定
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
