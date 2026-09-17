import { useEffect, useMemo, useRef, useState } from 'react';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { SendHorizontal, Settings2, Square, VolumeX } from 'lucide-react';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Spinner } from '@/components/ui/spinner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { fixMarkdownQuotes } from '@/lib/st-display';
import { useStore } from '../../store';
import { GroupSettingsDialog } from './GroupSettingsDialog';

/** 群聊视图：成员头像/名字、触发单聊、静音标记。 */
export function GroupChatArea() {
  const {
    groups, activeGroupId, characters, messages, generating, sendGroup, stopGeneration,
  } = useStore();
  const [input, setInput] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const group = groups.find((g) => g.id === activeGroupId);
  const members = useMemo(
    () =>
      (group?.members ?? [])
        .map((avatar) => characters.find((c) => c.avatar === avatar))
        .filter(Boolean),
    [group, characters],
  );

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length]);

  if (!group) {
    return <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">未选择群组</div>;
  }

  const doSend = () => {
    const text = input.trim();
    if (!text) return;
    setInput('');
    void sendGroup(text);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 成员行：点击触发单成员回复 */}
      <div className="flex shrink-0 items-center gap-2 border-b px-3 py-1.5">
        <div className="flex flex-1 items-center gap-1.5 overflow-x-auto">
          {members.map((m) => {
            const muted = group.disabled_members.includes(m!.avatar);
            return (
              <Tooltip key={m!.avatar}>
                <TooltipTrigger asChild>
                  <button
                    className="relative shrink-0"
                    disabled={generating}
                    onClick={() => void sendGroup('', m!.avatar)}
                    title={`让 ${m!.name} 回复`}
                  >
                    <Avatar className={'size-7 ' + (muted ? 'opacity-40' : '')}>
                      <img src={m!.avatarUrl} alt={m!.name} className="size-full object-cover" />
                      <AvatarFallback className="bg-primary/20 text-[10px] text-primary">
                        {m!.name.slice(0, 1)}
                      </AvatarFallback>
                    </Avatar>
                    {muted && (
                      <VolumeX className="absolute -bottom-0.5 -right-0.5 size-3 rounded-full bg-background p-0.5 text-muted-foreground" />
                    )}
                  </button>
                </TooltipTrigger>
                <TooltipContent side="bottom">让 {m!.name} 回复</TooltipContent>
              </Tooltip>
            );
          })}
        </div>
        <Button variant="ghost" size="icon" className="size-7" onClick={() => setSettingsOpen(true)} title="群设置">
          <Settings2 />
        </Button>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-4 p-4 pb-2">
          {messages.map((m, i) => (
            <GroupMessage key={i} m={m} />
          ))}
          {generating && (
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <Spinner className="size-3" />
              群成员生成中…
            </div>
          )}
          <div ref={bottomRef} />
        </div>
      </ScrollArea>

      <footer className="shrink-0 border-t bg-background p-3">
        <div className="mx-auto flex max-w-3xl items-end gap-2">
          <Textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                doSend();
              }
            }}
            placeholder="输入消息，群成员将按激活策略回复…（Enter 发送）"
            rows={2}
            disabled={generating}
            className="min-h-0 resize-none"
          />
          {generating ? (
            <Button
              variant="destructive"
              size="icon"
              className="size-10 shrink-0"
              onClick={stopGeneration}
              title="停止生成"
            >
              <Square />
            </Button>
          ) : (
            <Button
              size="icon"
              className="size-10 shrink-0"
              onClick={doSend}
              disabled={!input.trim()}
              title="发送"
            >
              <SendHorizontal />
            </Button>
          )}
        </div>
      </footer>

      <GroupSettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  );
}

function GroupMessage({ m }: { m: any }) {
  const { characters } = useStore();
  const char =
    characters.find((c) => c.avatar === m.original_avatar) ??
    characters.find((c) => c.name === m.name);
  const rendered = useMemo(() => {
    const raw = fixMarkdownQuotes(String(m.extra?.display_text ?? m.mes));
    return DOMPurify.sanitize(marked.parse(raw, { async: false }) as string);
  }, [m.extra?.display_text, m.mes]);

  return (
    <div className={'flex w-full gap-2 ' + (m.is_user ? 'justify-end' : 'justify-start')}>
      {!m.is_user && (
        <Avatar className="mt-1 size-8 shrink-0">
          {char?.avatarUrl ? (
            <img src={char.avatarUrl} alt={m.name} className="size-full object-cover" />
          ) : null}
          <AvatarFallback className="bg-primary/20 text-xs text-primary">
            {String(m.name ?? '').slice(0, 2)}
          </AvatarFallback>
        </Avatar>
      )}
      <div className={'flex max-w-[75%] flex-col gap-1 ' + (m.is_user ? 'items-end' : 'items-start')}>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground/80">{m.name}</span>
          <span className="opacity-60">{formatTime(m.send_date)}</span>
        </div>
        <div
          className={
            'msg-content break-words rounded-2xl px-4 py-2.5 text-sm leading-relaxed ' +
            (m.is_user ? 'rounded-tr-sm bg-primary/25' : 'rounded-tl-sm border bg-card')
          }
          dangerouslySetInnerHTML={{ __html: rendered }}
        />
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
    return new Date(iso).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return '';
  }
}
