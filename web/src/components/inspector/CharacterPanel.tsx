import { useEffect, useState } from 'react';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { Separator } from '@/components/ui/separator';
import { Label } from '@/components/ui/label';
import {
  Accordion, AccordionContent, AccordionItem, AccordionTrigger,
} from '@/components/ui/accordion';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';

const LS_KEY = 'nast:persona_draft';

const EDITABLE_FIELDS = [
  { key: 'description', label: '描述（Description）', rows: 6 },
  { key: 'personality', label: '性格（Personality）', rows: 3 },
  { key: 'scenario', label: '场景（Scenario）', rows: 3 },
  { key: 'first_mes', label: '开场白（First Message）', rows: 4 },
  { key: 'mes_example', label: '对话示例（Examples）', rows: 6 },
  { key: 'system_prompt', label: '系统提示覆盖（System Prompt）', rows: 3 },
  { key: 'post_history_instructions', label: 'PHI / Jailbreak 覆盖', rows: 3 },
] as const;

export function CharacterPanel() {
  const { characters, activeAvatar, personaDraft, setPersonaDraft, loadAll } = useStore();
  const ch = characters.find((c) => c.avatar === activeAvatar);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const saved = localStorage.getItem(LS_KEY);
    if (saved !== null) setPersonaDraft(saved);
  }, [setPersonaDraft]);

  useEffect(() => {
    setDraft({});
    setDirty(false);
    if (!activeAvatar) return;
    rpc
      .call<any>('characters.get', { avatar: activeAvatar })
      .then((c) => {
        const d: Record<string, string> = {};
        for (const f of EDITABLE_FIELDS) d[f.key] = c.data?.[f.key] ?? '';
        setDraft(d);
      })
      .catch(() => setDraft({}));
  }, [activeAvatar]);

  const saveCard = async () => {
    if (!activeAvatar) return;
    setSaving(true);
    try {
      await rpc.call('characters.edit', { avatar: activeAvatar, data: draft });
      await loadAll();
      pushToast('角色卡已保存', 'success');
      setDirty(false);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setSaving(false);
    }
  };

  if (!ch) {
    return <p className="text-xs text-muted-foreground">未选择角色</p>;
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <Label className="text-xs text-muted-foreground">Overview</Label>
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium">{ch.name}</span>
          {dirty && (
            <Button size="sm" className="h-6 px-2 text-xs" onClick={() => void saveCard()} disabled={saving}>
              {saving ? <Spinner /> : null}
              保存卡片
            </Button>
          )}
        </div>
        <div className="flex flex-wrap gap-1">
          {ch.tags.map((t) => (
            <Badge key={t} variant="secondary" className="px-1.5 text-[10px]">
              {t}
            </Badge>
          ))}
        </div>
      </div>

      <Separator />

      <div className="flex flex-col gap-2">
        <Label className="text-xs text-muted-foreground">Persona（你的角色设定）</Label>
        <Textarea
          value={personaDraft}
          onChange={(e) => {
            setPersonaDraft(e.target.value);
            localStorage.setItem(LS_KEY, e.target.value);
          }}
          placeholder="描述你是谁…随每次生成发送"
          rows={4}
          className="min-h-0 resize-y text-xs"
        />
        <p className="text-[10px] text-muted-foreground">暂存于浏览器本地，随 generate 参数即时生效</p>
      </div>

      <Separator />

      <div>
        <Label className="mb-2 block text-xs text-muted-foreground">卡片编辑</Label>
        <Accordion type="multiple" className="w-full">
          {EDITABLE_FIELDS.map((f) => (
            <AccordionItem key={f.key} value={f.key} className="border-b-0">
              <AccordionTrigger className="py-2 text-xs hover:no-underline">
                {f.label}
              </AccordionTrigger>
              <AccordionContent>
                <textarea
                  value={draft[f.key] ?? ''}
                  onChange={(e) => {
                    setDraft((d) => ({ ...d, [f.key]: e.target.value }));
                    setDirty(true);
                  }}
                  rows={f.rows}
                  className="w-full resize-y rounded-md border border-input bg-background px-2.5 py-1.5 text-xs shadow-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
                />
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      </div>
    </div>
  );
}
