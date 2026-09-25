import { currentConversation } from '@/models';
import { ModelSelector } from './ModelSelector';
import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { Spinner } from '@/components/ui/spinner';
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from '@/components/ui/empty';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  ChevronLeft,
  ChevronRight,
  RefreshCw,
  Wand2,
  ArrowRightToLine,
  MessageSquarePlus,
} from 'lucide-react';
import { rpc } from '../../rpc';
import { runSlashCommand } from '../../commands';
import { useStore } from '../../store';
import { MessageBubble } from './MessageBubble';
import { StreamingBubble } from './StreamingBubble';
import { ChatTranscript } from './ChatTranscript';
import { ChatComposer } from './ChatComposer';
import { useChatDraft } from '@/hooks/use-chat-draft';

const draftKey = (avatar: string | null, chat: string | null) =>
  `nast:draft:${avatar ?? ''}:${chat ?? ''}`;

export function ChatArea() {
  const {
    activeAvatar,
    activeChatName,
    characters,
    connected,
    newChat,
    messages,
    streamingText,
    streamingReasoning,
    generating,
    send,
    swipe,
    regenerate,
    continueGen,
    impersonate,
    stopGeneration,
    appendStreamToken,
    appendStreamReasoning,
  } = useStore();
  const [input, setInput] = useChatDraft(draftKey(activeAvatar, activeChatName));
  const [tokenStats, setTokenStats] = useState<{ total: number; budget: number } | null>(null);
  const sending = useRef(false);
  const [pending, setPending] = useState(false);
  const [creating, setCreating] = useState(false);
  const character = characters.find((c) => c.avatar === activeAvatar);

  // token 计量（切聊天 / 消息变化时刷新）
  useEffect(() => {
    if (!activeAvatar || !activeChatName) {
      setTokenStats(null);
      return;
    }
    let cancelled = false;
    rpc
      .call<{ total: number; budget: number }>('chats.stats', {
        avatar: activeAvatar,
        file_name: activeChatName,
      })
      .then((s) => {
        if (!cancelled) setTokenStats(s);
      })
      .catch(() => {
        if (!cancelled) setTokenStats(null);
      });
    return () => {
      cancelled = true;
    };
  }, [activeAvatar, activeChatName, messages.length]);

  // P1：接通后端流式事件——逐 token 追加到 streamingText / streamingReasoning
  useEffect(() => {
    const off1 = rpc.on('stream_token_received', (data: { text: string; conversation?: unknown }) => {
      if (data.conversation && JSON.stringify(data.conversation) !== JSON.stringify(currentConversation())) return;
      appendStreamToken(data.text);
    });
    const off2 = rpc.on('stream_reasoning_received', (data: { text: string; conversation?: unknown }) => {
      if (data.conversation && JSON.stringify(data.conversation) !== JSON.stringify(currentConversation())) return;
      appendStreamReasoning(data.text);
    });
    return () => {
      off1();
      off2();
    };
  }, [appendStreamToken, appendStreamReasoning]);

  const lastMsg = messages[messages.length - 1];
  const canSwipe = lastMsg && !lastMsg.is_user;
  const swipeIdx = lastMsg?.swipe_id ?? 0;
  const swipeTotal = lastMsg?.swipes?.length ?? 0;

  const doSend = async () => {
    const text = input.trim();
    if (!text || generating || !connected || !activeChatName || sending.current) return;
    sending.current = true;
    setPending(true);
    try {
      // ST 语义：斜杠命令在发送前拦截——内置命令本地执行、未知命令拦截、
      // 自定义命令展开为文本、插件命令透传
      if (text.startsWith('/')) {
        let commandChangedInput = false;
        const r = await runSlashCommand(text, {
          setInput: (value) => {
            commandChangedInput = true;
            setInput(value);
          },
        });
        if (r.status === 'handled') {
          if (!commandChangedInput) setInput('');
          return;
        }
        if (r.status === 'unknown') return;
        if (r.status === 'send') {
          setInput('');
          const ok = await send(r.text);
          if (!ok) setInput(text); // 展开文本发送失败时回填原命令
          return;
        }
        // passthrough：原样发送（服务端插件命令）
      }
      setInput('');
      const ok = await send(text);
      if (!ok) setInput(text); // 失败回填，避免用户文字丢失
    } catch {
      setInput(text);
    } finally {
      sending.current = false;
      setPending(false);
    }
  };

  const doImpersonate = async () => {
    try {
      const text = await impersonate();
      if (text) setInput(text);
    } catch {
      /* 全局错误提示保留当前草稿。 */
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ModelSelector />
      <ChatTranscript>
        {messages.length === 0 && !generating && (
          <Empty className="py-12">
            <EmptyHeader>
              <EmptyTitle>与 {character?.name} 的故事</EmptyTitle>
              <EmptyDescription>
                {activeChatName
                  ? '写下第一句话，让故事继续。'
                  : '创建一段聊天，从角色的开场白开始。'}
              </EmptyDescription>
            </EmptyHeader>
            {!activeChatName && (
              <EmptyContent>
                <Button
                  disabled={!connected || creating}
                  onClick={async () => {
                    if (!activeAvatar || creating) return;
                    setCreating(true);
                    try {
                      await newChat(activeAvatar);
                    } catch {
                      /* 全局错误提示 */
                    } finally {
                      setCreating(false);
                    }
                  }}
                >
                  {creating ? <Spinner /> : <MessageSquarePlus />}开始聊天
                </Button>
              </EmptyContent>
            )}
          </Empty>
        )}
        {messages.map((m, i) => {
          const isLastAi = i === messages.length - 1 && !m.is_user;
          if (isLastAi && generating && streamingText !== null) return null;
          return <MessageBubble key={i} m={m} index={i} />;
        })}
        {generating && streamingText !== null && (
          <StreamingBubble
            name={character?.name ?? ''}
            text={streamingText}
            reasoning={streamingReasoning}
          />
        )}
      </ChatTranscript>
      <ChatComposer
        input={input}
        onInput={setInput}
        onSend={() => void doSend()}
        onStop={() => void stopGeneration().catch(() => {})}
        generating={generating}
        pending={pending}
        disabled={!connected || !activeChatName}
        placeholder={`回应 ${character?.name ?? '角色'}…`}
        toolbar={
          <>
            {canSwipe && (
              <>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7"
                  disabled={!connected || generating || pending || swipeIdx <= 0}
                  onClick={() => void swipe('left').catch(() => {})}
                  title="上一个 swipe"
                  aria-label="上一个回复版本"
                >
                  <ChevronLeft />
                </Button>
                {swipeTotal > 0 && (
                  <span className="min-w-10 text-center text-xs tabular-nums text-muted-foreground">
                    {swipeIdx + 1}/{swipeTotal}
                  </span>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7"
                  disabled={!connected || generating || pending}
                  onClick={() => void swipe('right').catch(() => {})}
                  title={
                    swipeTotal > 0 && swipeIdx < swipeTotal - 1 ? '下一个 swipe' : '生成新 swipe'
                  }
                  aria-label="下一个回复版本"
                >
                  <ChevronRight />
                </Button>
              </>
            )}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="px-2"
                  disabled={!connected || generating || pending || !canSwipe}
                  onClick={() => void regenerate().catch(() => {})}
                >
                  {generating ? <Spinner /> : <RefreshCw />}
                  重生成
                </Button>
              </TooltipTrigger>
              <TooltipContent>重新生成最后一条回复</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="px-2"
                  disabled={!connected || generating || pending || !canSwipe}
                  onClick={() => void continueGen().catch(() => {})}
                >
                  <ArrowRightToLine />
                  续写
                </Button>
              </TooltipTrigger>
              <TooltipContent>续写最后一条消息</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="px-2"
                  disabled={!connected || !activeChatName || generating || pending}
                  onClick={doImpersonate}
                >
                  <Wand2 />
                  代入
                </Button>
              </TooltipTrigger>
              <TooltipContent>以用户身份生成发言（填入输入框）</TooltipContent>
            </Tooltip>
          </>
        }
        status={
          tokenStats &&
          tokenStats.budget > 0 && (
            <div className="flex items-center gap-2 px-1 text-[11px] text-muted-foreground">
              <span>上下文</span>
              <Progress
                aria-label="上下文用量"
                value={Math.min(100, (tokenStats.total / tokenStats.budget) * 100)}
                className="h-1 flex-1"
              />
              <span className="tabular-nums">
                {tokenStats.total.toLocaleString()} / {tokenStats.budget.toLocaleString()} tokens
              </span>
            </div>
          )
        }
      />
    </div>
  );
}
