import { useMemo, useState } from 'react';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { BrainCog, Check, Copy, Pencil, Trash2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import {
  Collapsible, CollapsibleContent, CollapsibleTrigger,
} from '@/components/ui/collapsible';
import { fixMarkdownQuotes } from '@/lib/st-display';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';

/**
 * 单聊与群聊共用的消息气泡。
 * group 模式：编辑/删除走 groups.* RPC（按 original_avatar 解析头像）。
 */
export function MessageBubble({
  m, index, group,
}: { m: any; index: number; group?: { chatId: string } }) {
  const { characters, deleteMessage, editMessage, reloadChat } = useStore();
  const char = group
    ? characters.find((c) => c.avatar === m.original_avatar) ??
      characters.find((c) => c.name === m.name)
    : characters.find((c) => c.name === m.name);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(m.mes);
  // display_text 优先（后端正则 display pass 产物），否则 markdown 渲染 mes
  const rendered = useMemo(() => {
    const raw = fixMarkdownQuotes(String(m.extra?.display_text ?? m.mes));
    const html = marked.parse(raw, { async: false }) as string;
    return DOMPurify.sanitize(html);
  }, [m.extra?.display_text, m.mes]);

  const reasoning = typeof m.extra?.reasoning === 'string' ? m.extra.reasoning : '';

  const saveEdit = async () => {
    if (group) {
      await rpc.call('groups.update_message', {
        chat_id: group.chatId,
        index,
        text: draft,
      });
      await reloadChat();
    } else {
      await editMessage(index, draft);
    }
    setEditing(false);
  };

  const copyMessage = async () => {
    try {
      await navigator.clipboard.writeText(String(m.mes));
      pushToast('已复制', 'success');
    } catch {
      pushToast('复制失败', 'error');
    }
  };

  return (
    <div className={'group flex w-full gap-2 ' + (m.is_user ? 'justify-end' : 'justify-start')}>
      {!m.is_user && (
        <Avatar className="mt-1 size-8 shrink-0">
          {char?.avatarUrl ? (
            <AvatarImage src={char.avatarUrl} alt={m.name} className="size-full object-cover" />
          ) : null}
          <AvatarFallback className="bg-primary/20 text-xs text-primary">
            {m.name.slice(0, 2)}
          </AvatarFallback>
        </Avatar>
      )}
      <div className={'flex max-w-[75%] flex-col gap-1 ' + (m.is_user ? 'items-end' : 'items-start')}>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground/80">{m.name}</span>
          <span className="opacity-60">{formatTime(m.send_date)}</span>
        </div>

        {reasoning && !editing && (
          <Collapsible className="w-full">
            <CollapsibleTrigger className="flex items-center gap-1.5 rounded-md border bg-muted/40 px-2 py-1 text-[11px] text-muted-foreground hover:bg-muted">
              <BrainCog className="size-3" />
              思考过程
              {m.extra?.reasoning_duration ? (
                <span className="tabular-nums opacity-70">
                  {(Number(m.extra.reasoning_duration) / 1000).toFixed(1)}s
                </span>
              ) : null}
            </CollapsibleTrigger>
            <CollapsibleContent>
              <pre className="mt-1 max-h-48 w-full overflow-auto whitespace-pre-wrap break-words rounded-md border bg-muted/20 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
                {reasoning}
              </pre>
            </CollapsibleContent>
          </Collapsible>
        )}

        <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
          <Button
            variant="ghost"
            size="icon"
            className="size-6 text-muted-foreground hover:text-foreground"
            onClick={() => void copyMessage()}
            title="复制"
          >
            <Copy className="size-3" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-6 text-muted-foreground hover:text-foreground"
            onClick={() => {
              setDraft(m.mes);
              setEditing(true);
            }}
            title="编辑（双击消息亦可）"
          >
            <Pencil className="size-3" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-6 text-muted-foreground hover:text-destructive"
            onClick={() =>
              group
                ? void rpc
                    .call('groups.delete_message', { chat_id: group.chatId, index })
                    .then(() => reloadChat())
                : void deleteMessage(index)
            }
            title="删除此消息"
          >
            <Trash2 className="size-3" />
          </Button>
        </div>
        {editing ? (
          <div className="flex w-full flex-col gap-2">
            <Textarea value={draft} onChange={(e) => setDraft(e.target.value)} rows={4} className="bg-card" />
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
                <X className="size-3.5" />
                取消
              </Button>
              <Button size="sm" onClick={saveEdit}>
                <Check className="size-3.5" />
                保存
              </Button>
            </div>
          </div>
        ) : (
          <div
            onDoubleClick={() => {
              setDraft(m.mes);
              setEditing(true);
            }}
            className={
              'msg-content break-words rounded-2xl px-4 py-2.5 text-sm leading-relaxed ' +
              (m.is_user ? 'rounded-tr-sm bg-primary/25' : 'rounded-tl-sm border bg-card')
            }
            dangerouslySetInnerHTML={{ __html: rendered }}
          />
        )}
      </div>
      {m.is_user && (
        <Avatar className="mt-1 size-8 shrink-0">
          <AvatarFallback className="bg-secondary text-xs text-secondary-foreground">我</AvatarFallback>
        </Avatar>
      )}
    </div>
  );
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return '';
  }
}
