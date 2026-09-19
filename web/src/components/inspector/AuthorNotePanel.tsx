import { useEffect, useState } from 'react';
import { Check, Loader2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { SliderField } from '../settings/fields';
import { useStore } from '../../store';

/** AN 位置（AN.js extension_floating_position） */
const NOTE_POSITIONS = [
  { value: '2', label: '主提示之前' },
  { value: '0', label: '主提示之后' },
  { value: '1', label: '聊天内 @ 深度' },
];

const NOTE_ROLES = [
  { value: '0', label: 'System' },
  { value: '1', label: 'User' },
  { value: '2', label: 'Assistant' },
];

interface NoteDraft {
  prompt: string;
  interval: number;
  depth: number;
  position: number;
  role: number;
}

const DEFAULT_NOTE: NoteDraft = { prompt: '', interval: 1, depth: 4, position: 1, role: 0 };

export function AuthorNotePanel() {
  const { chatMetadata, activeChatName, setNote } = useStore();
  const [draft, setDraft] = useState<NoteDraft>(DEFAULT_NOTE);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (activeChatName && chatMetadata) {
      setDraft({
        prompt: chatMetadata.note_prompt ?? '',
        interval: chatMetadata.note_interval ?? 1,
        depth: chatMetadata.note_depth ?? 4,
        position: chatMetadata.note_position ?? 1,
        role: chatMetadata.note_role ?? 0,
      });
    } else if (!activeChatName) {
      setDraft(DEFAULT_NOTE);
    }
  }, [activeChatName, chatMetadata]);

  if (!activeChatName) {
    return <p className="text-xs text-muted-foreground">未打开聊天</p>;
  }

  const save = async (clear = false) => {
    setSaving(true);
    try {
      await setNote(
        clear
          ? { prompt: null }
          : {
              prompt: draft.prompt.trim(),
              interval: draft.interval,
              depth: draft.depth,
              position: draft.position,
              role: draft.role,
            },
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="mb-1 text-sm font-semibold">Author's Note</h3>
        <p className="text-xs text-muted-foreground">
          按聊天保存在服务端（chat_metadata.note_*），interval 控制按用户消息数取模插入。
        </p>
      </div>

      <div className="flex flex-col gap-2">
        <Label className="text-xs text-muted-foreground">内容</Label>
        <Textarea
          value={draft.prompt}
          onChange={(e) => setDraft((d) => ({ ...d, prompt: e.target.value }))}
          placeholder="注入到提示的指导文本…"
          rows={12}
          className="min-h-0 resize-y text-xs"
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>插入位置</Label>
        <Select
          value={String(draft.position)}
          onValueChange={(v) => setDraft((d) => ({ ...d, position: Number(v) }))}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {NOTE_POSITIONS.map((p) => (
              <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {draft.position === 1 && (
        <>
          <SliderField
            label="注入深度（距末尾消息数）"
            value={draft.depth}
            min={0}
            max={16}
            step={1}
            onChange={(v) => setDraft((d) => ({ ...d, depth: v }))}
          />
          <div className="flex flex-col gap-1.5">
            <Label>注入角色</Label>
            <Select
              value={String(draft.role)}
              onValueChange={(v) => setDraft((d) => ({ ...d, role: Number(v) }))}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {NOTE_ROLES.map((r) => (
                  <SelectItem key={r.value} value={r.value}>{r.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </>
      )}

      <div className="flex flex-col gap-1.5">
        <Label>插入频率（每 N 条用户消息）</Label>
        <Input
          type="number"
          min={1}
          max={99}
          value={draft.interval}
          onChange={(e) => setDraft((d) => ({ ...d, interval: Math.max(1, Number(e.target.value) || 1) }))}
        />
      </div>

      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" disabled={saving} onClick={() => void save(true)}>
          <X className="size-3.5" />
          清除
        </Button>
        <Button size="sm" disabled={saving} onClick={() => void save()}>
          {saving ? <Loader2 className="size-3.5 animate-spin" /> : <Check className="size-3.5" />}
          保存到本聊天
        </Button>
      </div>
    </div>
  );
}
