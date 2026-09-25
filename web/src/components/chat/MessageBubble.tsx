import { useEffect, useMemo, useState } from 'react';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { BrainCog, Check, Copy, Pencil, Trash2, X, Volume2, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { Spinner } from '@/components/ui/spinner';
import { cn } from '@/lib/utils';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { fixMarkdownQuotes } from '@/lib/st-display';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';
import { readTtsSettings } from '@/tts/config';
import { ttsPlayer, useTtsPlayback } from '@/tts/player';

/**
 * 单聊与群聊共用的消息气泡。
 * group 模式：编辑/删除走 groups.* RPC（按 original_avatar 解析头像）。
 */
export function MessageBubble({
  m,
  index,
  group,
}: {
  m: any;
  index: number;
  group?: { chatId: string };
}) {
  const { characters, deleteMessage, editMessage, reloadChat, generating, connected } = useStore();
  const tts = readTtsSettings(useStore((s) => s.settings));
  const playback = useTtsPlayback();
  const speaking = playback.messageId === index && !['idle', 'error'].includes(playback.status);
  const char = group
    ? (characters.find((c) => c.avatar === m.original_avatar) ??
      characters.find((c) => c.name === m.name))
    : characters.find((c) => c.name === m.name);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(m.mes);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1800);
    return () => window.clearTimeout(timer);
  }, [copied]);
  // display_text 优先（后端正则 display pass 产物），否则 markdown 渲染 mes
  const rendered = useMemo(() => {
    const raw = fixMarkdownQuotes(String(m.extra?.display_text ?? m.mes));
    const html = marked.parse(raw, { async: false }) as string;
    return DOMPurify.sanitize(html);
  }, [m.extra?.display_text, m.mes]);

  const reasoning = typeof m.extra?.reasoning === 'string' ? m.extra.reasoning : '';

  const saveEdit = async () => {
    if (saving) return;
    setSaving(true);
    try {
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
    } catch {
      /* 失败保持编辑状态及草稿；RPC 全局提示错误。 */
    } finally {
      setSaving(false);
    }
  };

  const copyMessage = async () => {
    try {
      await navigator.clipboard.writeText(String(m.mes));
      setCopied(true);
      pushToast('已复制', 'success');
    } catch {
      pushToast('复制失败', 'error');
    }
  };

  return (
    <article
      className={cn(
        'group flex w-full min-w-0 gap-2.5 sm:gap-3',
        m.is_user ? 'justify-end' : 'justify-start',
      )}
      aria-label={`${m.name} 的消息`}
    >
      {!m.is_user && (
        <Avatar className="mt-1 size-8 shrink-0 rounded-xl sm:size-9">
          {char?.avatarUrl ? (
            <AvatarImage src={char.avatarUrl} alt={m.name} className="size-full object-cover" />
          ) : null}
          <AvatarFallback className="rounded-xl bg-accent text-xs text-primary">
            {m.name.slice(0, 2)}
          </AvatarFallback>
        </Avatar>
      )}
      <div
        className={cn(
          'flex min-w-0 max-w-[calc(100%-3rem)] flex-col gap-2 sm:max-w-[88%]',
          m.is_user ? 'items-end' : 'items-start',
          editing && 'w-full',
        )}
      >
        <div className="flex max-w-full flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          <span className="max-w-full truncate font-medium text-foreground">{m.name}</span>
          <time className="text-[11px]" dateTime={m.send_date}>
            {formatTime(m.send_date)}
          </time>
        </div>

        {m.extra?.nast_model?.status === 'incomplete' && <p role="status" className="text-xs text-destructive">未完成 · 已保留收到的内容</p>}
        {m.extra?.nast_model && <details className="text-xs text-muted-foreground"><summary className="cursor-pointer">模型调用信息</summary><p className="break-all">模型：{m.extra.nast_model.logical_model} · 路由：{m.extra.nast_model.route} · 上游：{m.extra.model} · {m.extra.nast_model.status === 'complete' ? '已完成' : '未完成'}</p></details>}
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

        <div className="message-tools order-3 -my-1">
          {tts.enabled && (
            <Button variant="ghost" size="icon" className="size-8" disabled={editing}
              title={speaking ? '停止朗读' : '朗读此消息'} aria-label={speaking ? '停止朗读此消息' : '朗读此消息'}
              onClick={() => speaking ? ttsPlayer.cancel() : ttsPlayer.speakMessage(m, index, { manual: true })}>
              {speaking ? <Square /> : <Volume2 />}
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            className="size-8"
            onClick={() => void copyMessage()}
            title={copied ? '已复制' : '复制'}
            aria-label={copied ? '已复制消息' : '复制消息'}
          >
            {copied ? <Check /> : <Copy />}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-8"
            disabled={!connected || generating || saving}
            onClick={() => {
              setDraft(m.mes);
              setEditing(true);
            }}
            title="编辑（双击消息亦可）"
            aria-label="编辑消息"
          >
            <Pencil />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-8"
            disabled={!connected || generating || saving}
            onClick={() =>
              group
                ? void rpc
                    .call('groups.delete_message', { chat_id: group.chatId, index })
                    .then(() => reloadChat())
                    .catch(() => {})
                : void deleteMessage(index).catch(() => {})
            }
            title="删除此消息"
            aria-label="删除消息"
          >
            <Trash2 />
          </Button>
        </div>
        {editing ? (
          <div className="flex w-full flex-col gap-2">
            <Textarea
              autoFocus
              aria-label="编辑消息内容"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              rows={4}
              disabled={saving}
              onKeyDown={(e) => {
                if (e.nativeEvent.isComposing) return;
                if (e.key === 'Escape' && !saving) setEditing(false);
                if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
                  e.preventDefault();
                  void saveEdit();
                }
              }}
            />
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" disabled={saving} onClick={() => setEditing(false)}>
                <X data-icon="inline-start" />
                取消
              </Button>
              <Button size="sm" disabled={saving} onClick={saveEdit}>
                {saving ? <Spinner /> : <Check data-icon="inline-start" />}
                {saving ? '保存中' : '保存'}
              </Button>
            </div>
          </div>
        ) : (
          <div
            onDoubleClick={() => {
              if (!connected || generating) return;
              setDraft(m.mes);
              setEditing(true);
            }}
            className="msg-content message-surface"
            data-user={m.is_user}
            dangerouslySetInnerHTML={{ __html: rendered }}
          />
        )}
      </div>
      {m.is_user && (
        <Avatar className="mt-1 hidden size-8 shrink-0 sm:flex">
          <AvatarFallback className="bg-secondary text-xs text-secondary-foreground">
            我
          </AvatarFallback>
        </Avatar>
      )}
    </article>
  );
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return '';
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
