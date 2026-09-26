import { useEffect, useMemo, useState } from 'react';
import { v4 as uuidv4 } from 'uuid';
import { Pencil, Plus, Play, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
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
import { ScrollArea } from '@/components/ui/scroll-area';
import { SwitchField } from '../fields';
import { pushToast } from '@/toasts';

/** ST 1.18 regex_placement 实值。 */
const PLACEMENTS = [
  { value: 1, label: '用户输入' },
  { value: 2, label: 'AI 输出' },
  { value: 3, label: '斜杠命令' },
  { value: 5, label: '世界书' },
  { value: 6, label: '推理' },
];

interface RegexScript {
  id: string;
  script_name: string;
  find_regex: string;
  replace_string: string;
  trim_strings: string[];
  placement: number[];
  disabled: boolean;
  markdown_only: boolean;
  prompt_only: boolean;
  run_on_edit: boolean;
  substitute_regex: number;
  min_depth: number | null;
  max_depth: number | null;
}

function newScript(): RegexScript {
  return {
    id: uuidv4(),
    script_name: '新脚本',
    find_regex: 'pattern',
    replace_string: '',
    trim_strings: [],
    placement: [2],
    disabled: false,
    markdown_only: false,
    prompt_only: false,
    run_on_edit: true,
    substitute_regex: 0,
    min_depth: null,
    max_depth: null,
  };
}

/** JS 侧试运行（与后端 regex crate 行为接近；不支持 look-around 时会报错）。 */
function tryRun(script: RegexScript, sample: string): string {
  let out = sample;
  try {
    const re = new RegExp(script.find_regex, 'g');
    out = out.replace(re, script.replace_string);
    for (const t of script.trim_strings) out = out.split(t).join('');
  } catch (e) {
    return `正则错误：${e instanceof Error ? e.message : String(e)}`;
  }
  return out;
}

export function RegexPanel({
  draft,
  patch,
}: {
  draft: any;
  patch: (path: string, v: unknown) => void;
}) {
  const scripts: RegexScript[] = useMemo(
    () =>
      (Array.isArray(draft.extension_settings?.regex) ? draft.extension_settings.regex : [])
        .map((s: any) => ({ trim_strings: [], placement: [], ...s })),
    [draft.extension_settings?.regex],
  );
  const [editingId, setEditingId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const setScripts = (next: RegexScript[]) => {
    patch('extension_settings.regex', next);
  };

  const editing = scripts.find((s) => s.id === editingId) ?? null;

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="mb-1 text-sm font-semibold">正则脚本（Regex）</h3>
        <p className="text-xs text-muted-foreground">
          全局脚本（extension_settings.regex，与 ST 互通）。作用域链：全局 → 角色内嵌 → 聊天级。
        </p>
      </div>

      <div className="flex items-center justify-between">
        <Label>脚本列表</Label>
        <Button
          size="sm"
          variant="secondary"
          onClick={() => {
            const s = newScript();
            setScripts([...scripts, s]);
            setEditingId(s.id);
          }}
        >
          <Plus className="size-3.5" />
          新建脚本
        </Button>
      </div>
      <ScrollArea className="max-h-64 rounded-md border p-1">
        <div className="flex flex-col gap-1">
          {scripts.map((s) => (
            <div key={s.id} className="flex items-center gap-2 rounded px-2 py-1 hover:bg-accent/50">
              <Switch
                checked={!s.disabled}
                onCheckedChange={(v) =>
                  setScripts(scripts.map((x) => (x.id === s.id ? { ...x, disabled: !v } : x)))
                }
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="truncate text-xs">{s.script_name || '(untitled)'}</span>
                  {s.markdown_only && <Badge variant="secondary" className="px-1 text-[9px]">仅显示</Badge>}
                  {s.prompt_only && <Badge variant="secondary" className="px-1 text-[9px]">仅提示</Badge>}
                </div>
                <p className="truncate text-[10px] text-muted-foreground">
                  {s.find_regex} → {s.replace_string || '(删除)'}
                </p>
              </div>
              <Button variant="ghost" size="icon" className="size-6" onClick={() => setEditingId(s.id)} title="编辑">
                <Pencil className="size-3" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-6 text-muted-foreground hover:text-destructive"
                onClick={() => setScripts(scripts.filter((x) => x.id !== s.id))}
                title="删除"
              >
                <Trash2 className="size-3" />
              </Button>
            </div>
          ))}
          {scripts.length === 0 && (
            <p className="px-2 py-3 text-center text-xs text-muted-foreground">
              暂无全局脚本。
            </p>
          )}
        </div>
      </ScrollArea>

      <RegexEditDialog
        script={editing}
        open={!!editing}
        onOpenChange={(v) => {
          if (!v) setEditingId(null);
        }}
        onSave={(next) => setScripts(scripts.map((x) => (x.id === next.id ? next : x)))}
      />
      {void creating}
    </div>
  );
}

function RegexEditDialog({
  script,
  open,
  onOpenChange,
  onSave,
}: {
  script: RegexScript | null;
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onSave: (s: RegexScript) => void;
}) {
  const [draft, setDraft] = useState<RegexScript | null>(script);
  const [sample, setSample] = useState('');
  useEffect(() => setDraft(script), [script]);
  const result = useMemo(() => draft ? tryRun(draft, sample) : '', [draft, sample]);
  if (!draft) return null;
  const set = (p: Partial<RegexScript>) => setDraft({ ...draft, ...p });

  const togglePlacement = (v: number, on: boolean) => {
    const next = on ? [...draft.placement, v] : draft.placement.filter((p) => p !== v);
    set({ placement: next });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[88vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>编辑正则脚本</DialogTitle>
          <DialogDescription>findRegex 为裸正则（无斜杠，大小写不敏感请用 (?i)）；替换支持 $1、{'{{match}}'}。</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label>脚本名</Label>
            <Input value={draft.script_name} onChange={(e) => set({ script_name: e.target.value })} />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>查找（正则）</Label>
            <Input
              value={draft.find_regex}
              onChange={(e) => set({ find_regex: e.target.value })}
              className="font-mono text-xs"
              placeholder="pattern"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>替换为（空 = 删除匹配）</Label>
            <Input
              value={draft.replace_string}
              onChange={(e) => set({ replace_string: e.target.value })}
              className="font-mono text-xs"
              placeholder="$1 / {{match}}"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>作用位置</Label>
            <div className="flex flex-wrap gap-x-4 gap-y-1.5">
              {PLACEMENTS.map((p) => (
                <label key={p.value} className="flex cursor-pointer items-center gap-1.5 text-xs">
                  <Checkbox
                    checked={draft.placement.includes(p.value)}
                    onCheckedChange={(v) => togglePlacement(p.value, v === true)}
                  />
                  {p.label}
                </label>
              ))}
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>剔除子串（逗号分隔，捕获组过滤）</Label>
            <Input
              value={draft.trim_strings.join(',')}
              onChange={(e) =>
                set({ trim_strings: e.target.value.split(',').map((t) => t.trim()).filter(Boolean) })
              }
              className="font-mono text-xs"
            />
          </div>
          <SwitchField
            label="仅显示（markdownOnly）"
            checked={draft.markdown_only}
            onChange={(v) => set({ markdown_only: v })}
          />
          <SwitchField
            label="仅提示（promptOnly）"
            checked={draft.prompt_only}
            onChange={(v) => set({ prompt_only: v })}
          />
          <SwitchField
            label="编辑消息时也运行"
            checked={draft.run_on_edit}
            onChange={(v) => set({ run_on_edit: v })}
          />
          <div className="flex flex-col gap-1.5">
            <Label>宏替换（substituteRegex）</Label>
            <Select
              value={String(draft.substitute_regex ?? 0)}
              onValueChange={(v) => set({ substitute_regex: Number(v) })}
            >
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="0">无</SelectItem>
                <SelectItem value="1">RAW（宏全替换）</SelectItem>
                <SelectItem value="2">ESCAPED（字面转义）</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div className="flex flex-col gap-1.5">
              <Label>最小深度（-1 不限）</Label>
              <Input
                type="number"
                value={draft.min_depth ?? -1}
                onChange={(e) => set({ min_depth: Number(e.target.value) })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>最大深度（0 不限）</Label>
              <Input
                type="number"
                value={draft.max_depth ?? 0}
                onChange={(e) => set({ max_depth: Number(e.target.value) })}
              />
            </div>
          </div>

          {/* 试运行 */}
          <div className="flex flex-col gap-1.5 rounded-md border bg-muted/30 p-2">
            <Label className="flex items-center gap-1.5">
              <Play className="size-3" />
              试运行
            </Label>
            <Textarea
              value={sample}
              onChange={(e) => setSample(e.target.value)}
              rows={3}
              placeholder="粘贴样本文本…"
              className="min-h-0 resize-y text-xs"
            />
            {sample && (
              <pre className="max-h-24 overflow-auto whitespace-pre-wrap break-words rounded bg-background p-2 text-[11px]">
                {result}
              </pre>
            )}
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>取消</Button>
          <Button
            onClick={() => {
              onSave(draft);
              pushToast('脚本已更新（记得保存设置）', 'success');
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
