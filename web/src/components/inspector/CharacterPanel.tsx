import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import { Separator } from '@/components/ui/separator';
import { Label } from '@/components/ui/label';
import { rpc } from '../../rpc';
import { useStore } from '../../store';

const LS_KEY = 'nast:persona_draft';

export function CharacterPanel() {
  const { characters, activeAvatar, personaDraft, setPersonaDraft } = useStore();
  const [expanded, setExpanded] = useState(false);
  const ch = characters.find((c) => c.avatar === activeAvatar);
  const [full, setFull] = useState<{ description: string; first_mes: string } | null>(null);

  useEffect(() => {
    const saved = localStorage.getItem(LS_KEY);
    if (saved !== null) setPersonaDraft(saved);
  }, [setPersonaDraft]);

  useEffect(() => {
    setFull(null);
    if (!activeAvatar) return;
    rpc
      .call<any>('characters.get', { avatar: activeAvatar })
      .then((c) => setFull({ description: c.data?.description ?? '', first_mes: c.data?.first_mes ?? '' }))
      .catch(() => setFull(null));
  }, [activeAvatar]);

  if (!ch) {
    return <p className="text-xs text-muted-foreground">未选择角色</p>;
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <Label className="text-xs text-muted-foreground">Overview</Label>
        <div className="text-sm font-medium">{ch.name}</div>
        <div className="flex flex-wrap gap-1">
          {ch.tags.map((t) => (
            <Badge key={t} variant="secondary" className="px-1.5 text-[10px]">
              {t}
            </Badge>
          ))}
        </div>
        {full && (
          <>
            <p
              className={
                'mt-1 whitespace-pre-wrap text-xs leading-relaxed text-muted-foreground ' +
                (expanded ? '' : 'line-clamp-6')
              }
            >
              {full.description}
            </p>
            <Button variant="ghost" size="sm" className="h-6 self-start px-2 text-xs" onClick={() => setExpanded(!expanded)}>
              {expanded ? '收起' : '展开全部'}
            </Button>
          </>
        )}
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
          rows={5}
          className="min-h-0 resize-y text-xs"
        />
        <p className="text-[10px] text-muted-foreground">暂存于浏览器本地，随 generate 参数即时生效</p>
      </div>
    </div>
  );
}
