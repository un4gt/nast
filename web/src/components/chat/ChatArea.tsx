import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Spinner } from '@/components/ui/spinner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  ChevronLeft, ChevronRight, RefreshCw, Wand2, ArrowRightToLine, Square, SendHorizontal,
} from 'lucide-react';
import { useStore } from '../../store';
import { MessageBubble } from './MessageBubble';
import { StreamingBubble } from './StreamingBubble';

export function ChatArea() {
  const {
    messages, streamingText, generating, send, swipe, regenerate, continueGen, impersonate,
    stopGeneration,
  } = useStore();
  const [input, setInput] = useState('');
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length, streamingText]);

  const lastMsg = messages[messages.length - 1];
  const canSwipe = lastMsg && !lastMsg.is_user;
  const swipeIdx = lastMsg?.swipe_id ?? 0;
  const swipeTotal = lastMsg?.swipes?.length ?? 0;

  const doSend = async () => {
    const text = input.trim();
    if (!text) return;
    setInput('');
    await send(text);
  };

  const doImpersonate = async () => {
    const text = await impersonate();
    if (text) setInput(text);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-4 p-4 pb-2">
          {messages.map((m, i) => {
            const isLastAi = i === messages.length - 1 && !m.is_user;
            if (isLastAi && generating && streamingText !== null) return null;
            return <MessageBubble key={i} m={m} />;
          })}
          {generating && streamingText !== null && (
            <StreamingBubble name={messages[messages.length - 1]?.name ?? ''} text={streamingText} />
          )}
          <div ref={bottomRef} />
        </div>
      </ScrollArea>

      <footer className="shrink-0 border-t bg-background p-3">
        <div className="mx-auto flex max-w-3xl flex-col gap-2">
          <div className="flex items-end gap-2">
            <Textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  doSend();
                }
              }}
              placeholder="输入消息…（Enter 发送，Shift+Enter 换行）"
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
          <div className="flex items-center gap-1">
            {canSwipe && (
              <>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7"
                  disabled={generating || swipeIdx <= 0}
                  onClick={() => swipe('left')}
                  title="上一个 swipe"
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
                  disabled={generating}
                  onClick={() => swipe('right')}
                  title={swipeTotal > 0 && swipeIdx < swipeTotal - 1 ? '下一个 swipe' : '生成新 swipe'}
                >
                  <ChevronRight />
                </Button>
              </>
            )}
            <div className="flex-1" />
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="sm" disabled={generating || !canSwipe} onClick={regenerate}>
                  {generating ? <Spinner /> : <RefreshCw />}
                  重生成
                </Button>
              </TooltipTrigger>
              <TooltipContent>重新生成最后一条回复</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="sm" disabled={generating || !canSwipe} onClick={continueGen}>
                  <ArrowRightToLine />
                  续写
                </Button>
              </TooltipTrigger>
              <TooltipContent>续写最后一条消息</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="sm" disabled={generating} onClick={doImpersonate}>
                  <Wand2 />
                  代入
                </Button>
              </TooltipTrigger>
              <TooltipContent>以用户身份生成发言（填入输入框）</TooltipContent>
            </Tooltip>
          </div>
        </div>
      </footer>
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
