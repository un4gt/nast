import { useEffect, useMemo, useRef, useState } from 'react';
import { RefreshCw, SendHorizontal, Settings2, Square, VolumeX } from 'lucide-react';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Spinner } from '@/components/ui/spinner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { MessageBubble } from './MessageBubble';
import { StreamingBubble } from './StreamingBubble';
import { GroupSettingsDialog } from './GroupSettingsDialog';

/** 群聊视图：成员头像/名字、触发单聊、静音标记、流式逐字渲染、消息微操。 */
export function GroupChatArea() {
  const {
    groups, activeGroupId, characters, messages, generating,
    sendGroup, stopGeneration, appendStreamToken, appendStreamReasoning,
    streamingText, streamingReasoning,
  } = useStore();
  const [input, setInput] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [currentMember, setCurrentMember] = useState<string>('');
  const bottomRef = useRef<HTMLDivElement>(null);

  const group = groups.find((g) => g.id === activeGroupId);
  const members = useMemo(
    () =>
      (group?.members ?? [])
        .map((avatar) => characters.find((c) => c.avatar === avatar))
        .filter(Boolean),
    [group, characters],
  );

  // 流式：成员切换（group_member_drafted 携带成员名）→ 重置缓冲；token 事件逐字追加
  useEffect(() => {
    const offDrafted = rpc.on('group_member_drafted', (data: unknown) => {
      const name = typeof data === 'string' ? data : String((data as any)?.name ?? '');
      setCurrentMember(name);
      useStore.setState({ streamingText: '', streamingReasoning: '' });
    });
    const offToken = rpc.on('stream_token_received', (data: { text: string }) => {
      if (useStore.getState().generating) appendStreamToken(data.text);
    });
    const offReasoning = rpc.on('stream_reasoning_received', (data: { text: string }) => {
      if (useStore.getState().generating) appendStreamReasoning(data.text);
    });
    return () => {
      offDrafted();
      offToken();
      offReasoning();
    };
  }, [appendStreamToken, appendStreamReasoning]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length, streamingText]);

  if (!group) {
    return <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">未选择群组</div>;
  }

  const doSend = async () => {
    const text = input.trim();
    if (!text) return;
    setInput('');
    const ok = await sendGroup(text);
    if (!ok) setInput(text); // 失败回填
  };

  /** 重生成：删除最后一条群消息后再触发一次群生成（无新用户消息）。 */
  const doRegenerate = async () => {
    if (generating || messages.length === 0) return;
    try {
      await rpc.call('groups.delete_message', {
        chat_id: group.chat_id,
        index: messages.length - 1,
      });
      await sendGroup('');
    } catch {
      // 全局 toast 已提示
    }
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
                      <AvatarImage src={m!.avatarUrl} alt={m!.name} className="size-full object-cover" />
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
            <MessageBubble key={i} m={m} index={i} group={{ chatId: group.chat_id }} />
          ))}
          {generating &&
            (streamingText !== null ? (
              <StreamingBubble
                name={currentMember || '…'}
                text={streamingText ?? ''}
                reasoning={streamingReasoning}
              />
            ) : (
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <Spinner className="size-3" />
                群成员生成中…
              </div>
            ))}
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
                void doSend();
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
            <>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="outline"
                    size="icon"
                    className="size-10 shrink-0"
                    onClick={() => void doRegenerate()}
                    disabled={messages.length === 0}
                    title="删除最后一条并重新生成"
                  >
                    <RefreshCw />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>重生成最后一条群消息</TooltipContent>
              </Tooltip>
              <Button
                size="icon"
                className="size-10 shrink-0"
                onClick={() => void doSend()}
                disabled={!input.trim()}
                title="发送"
              >
                <SendHorizontal />
              </Button>
            </>
          )}
        </div>
      </footer>

      <GroupSettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  );
}
