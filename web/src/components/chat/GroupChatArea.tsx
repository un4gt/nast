import { useEffect, useMemo, useRef, useState } from 'react';
import { RefreshCw, Settings2, VolumeX } from 'lucide-react';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty';
import { Spinner } from '@/components/ui/spinner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { MessageBubble } from './MessageBubble';
import { StreamingBubble } from './StreamingBubble';
import { GroupSettingsDialog } from './GroupSettingsDialog';
import { GroupSessionTools } from './GroupSessionTools';
import { ChatComposer } from './ChatComposer';
import { ChatTranscript } from './ChatTranscript';
import { useChatDraft } from '@/hooks/use-chat-draft';
import { cn } from '@/lib/utils';
import { runSlashCommand } from '@/commands';

/** 群聊视图：成员头像/名字、触发单聊、静音标记、流式逐字渲染、消息微操。 */
export function GroupChatArea() {
  const {
    groups,
    activeGroupId,
    characters,
    messages,
    generating,
    connected,
    sendGroup,
    stopGeneration,
    appendStreamToken,
    appendStreamReasoning,
    streamingText,
    streamingReasoning,
  } = useStore();
  const [input, setInput] = useChatDraft(`nast:draft:group:${activeGroupId ?? ''}`);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [currentMember, setCurrentMember] = useState<string>('');
  const sending = useRef(false);
  const [pending, setPending] = useState(false);
  const [autoMode, setAutoMode] = useState(false);

  const group = groups.find((g) => g.id === activeGroupId);
  useEffect(() => { setAutoMode(false); }, [activeGroupId]);
  useEffect(() => {
    if (!autoMode || generating || pending || !connected || !group) return;
    const timer = window.setTimeout(() => {
      void sendGroup('').then((ok) => { if (!ok) setAutoMode(false); });
    }, Math.max(1, group.auto_mode_delay ?? 5) * 1000);
    return () => window.clearTimeout(timer);
  }, [autoMode, generating, pending, connected, group, sendGroup]);
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

  if (!group) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
        未选择群组
      </div>
    );
  }

  const doSend = async () => {
    const text = input.trim();
    if (!text || !connected || generating || sending.current) return;
    sending.current = true;
    setPending(true);
    setInput('');
    try {
      if (/^\/(speak|tts-stop)(?:\s|$)/i.test(text)) {
        await runSlashCommand(text);
        return;
      }
      const ok = await sendGroup(text);
      if (!ok) setInput(text);
    } catch {
      setInput(text);
    } finally {
      sending.current = false;
      setPending(false);
    }
  };

  /** 服务端按 gen_id 重生成整轮，避免混入上一轮的成员回答。 */
  const doRegenerate = async () => {
    if (!connected || generating || sending.current || messages.length === 0) return;
    sending.current = true;
    setPending(true);
    try {
      await sendGroup('', undefined, 'regenerate');
    } catch {
      // 全局 toast 已提示
    } finally {
      sending.current = false;
      setPending(false);
    }
  };

  const navigateCandidate = async (direction: 'left' | 'right') => {
    if (pending || generating || !group) return;
    setPending(true);
    try {
      await rpc.call('groups.swipe', { id: group.id, chat_id: group.chat_id, direction });
      await useStore.getState().reloadChat();
    } catch { /* RPC displays the error; keep the current candidate. */ }
    finally { setPending(false); }
  };
  const last = messages[messages.length - 1];
  const candidate = last?.swipe_id ?? 0;
  const candidates = last?.swipes?.length ?? 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 成员行：点击触发单成员回复 */}
      <div className="flex shrink-0 items-center gap-2 border-b px-4 py-3 sm:px-6">
        <div className="flex flex-1 items-center gap-1.5 overflow-x-auto">
          {members.map((m) => {
            const muted = group.disabled_members.includes(m!.avatar);
            return (
              <Tooltip key={m!.avatar}>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    className="relative h-auto shrink-0 gap-2 px-2 py-1.5"
                    disabled={!connected || generating || pending}
                    onClick={() => void sendGroup('', m!.avatar)}
                    title={`让 ${m!.name} 回复`}
                  >
                    <Avatar className={cn('size-7', muted && 'opacity-40')}>
                      <AvatarImage
                        src={m!.avatarUrl}
                        alt={m!.name}
                        className="size-full object-cover"
                      />
                      <AvatarFallback className="bg-primary/20 text-[10px] text-primary">
                        {m!.name.slice(0, 1)}
                      </AvatarFallback>
                    </Avatar>
                    <span className="text-xs">{m!.name}</span>
                    {muted && (
                      <VolumeX className="absolute -bottom-0.5 -right-0.5 size-3 rounded-full bg-background p-0.5 text-muted-foreground" />
                    )}
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom">让 {m!.name} 回复</TooltipContent>
              </Tooltip>
            );
          })}
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="size-9 shrink-0"
          onClick={() => setSettingsOpen(true)}
          title="群设置"
          aria-label="群设置"
        >
          <Settings2 />
        </Button>
      </div>

      <GroupSessionTools />
      <ChatTranscript>
        {messages.length === 0 && !generating && (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>让大家聊起来</EmptyTitle>
              <EmptyDescription>
                发送第一条消息，或点击上方成员，让指定角色先开口。
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
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
      </ChatTranscript>
      <ChatComposer
        input={input}
        onInput={setInput}
        onSend={() => void doSend()}
        onStop={() => { setAutoMode(false); void stopGeneration().catch(() => {}); }}
        generating={generating}
        pending={pending}
        disabled={!connected}
        placeholder="对大家说点什么…"
        toolbar={
          <div className="flex flex-wrap items-center gap-1">
          {candidates > 1 && <>
            <Button variant="ghost" size="sm" aria-label="上一个候选回复" disabled={!connected || generating || pending || candidate === 0}
              onClick={() => void navigateCandidate('left')}>上一条</Button>
            <span className="text-xs tabular-nums text-muted-foreground" aria-live="polite">{candidate + 1} / {candidates}</span>
            <Button variant="ghost" size="sm" aria-label="下一个候选回复" disabled={!connected || generating || pending || candidate >= candidates - 1}
              onClick={() => void navigateCandidate('right')}>下一条</Button>
          </>}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                disabled={!connected || generating || pending || messages.length === 0}
                onClick={() => void doRegenerate()}
              >
                <RefreshCw data-icon="inline-start" />
                重新生成
              </Button>
            </TooltipTrigger>
            <TooltipContent>重生成最后一轮成员回复</TooltipContent>
          </Tooltip>
          <Button variant="ghost" size="sm" disabled={!connected || generating || pending || !messages.length || messages[messages.length - 1]?.is_user}
            onClick={() => void sendGroup('', undefined, 'continue')}>续写</Button>
          <Button variant="ghost" size="sm" disabled={!connected || generating || pending || !messages.length || messages[messages.length - 1]?.is_user}
            onClick={() => void sendGroup('', undefined, 'swipe')}>新候选回复</Button>
          <Button variant="ghost" size="sm" aria-pressed={autoMode} disabled={!connected} onClick={() => setAutoMode((value) => !value)}>
            {autoMode ? '停止自动对话' : '自动对话'}
          </Button>
          </div>
        }
      />

      <GroupSettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  );
}
