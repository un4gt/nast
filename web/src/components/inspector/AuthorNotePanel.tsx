import { useEffect, useState } from 'react';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { useStore } from '../../store';
import { SliderField } from '../settings/fields';

const LS_KEY = 'nast:an_draft';

export function AuthorNotePanel() {
  const [draft, setDraft] = useState<{ prompt: string; depth: number }>({ prompt: '', depth: 4 });
  const { setAnDraft } = useStore();

  useEffect(() => {
    const saved = localStorage.getItem(LS_KEY);
    if (saved) {
      try {
        setDraft(JSON.parse(saved));
      } catch {
        // ignore
      }
    }
  }, []);

  const update = (next: { prompt: string; depth: number }) => {
    setDraft(next);
    localStorage.setItem(LS_KEY, JSON.stringify(next));
    setAnDraft(next);
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <Label className="text-xs text-muted-foreground">Author's Note 内容</Label>
        <Textarea
          value={draft.prompt}
          onChange={(e) => update({ ...draft, prompt: e.target.value })}
          placeholder="注入到聊天深度的指导文本…"
          rows={6}
          className="min-h-0 resize-y text-xs"
        />
      </div>
      <SliderField
        label="注入深度（距末尾消息数）"
        value={draft.depth}
        min={0}
        max={16}
        step={1}
        onChange={(v) => update({ ...draft, depth: v })}
      />
      <p className="text-[10px] text-muted-foreground">
        暂存于浏览器本地，随每次 generate 以 depth 注入即时生效
      </p>
    </div>
  );
}
