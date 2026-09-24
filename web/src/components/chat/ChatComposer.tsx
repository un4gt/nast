import { useLayoutEffect, useRef, type ReactNode } from 'react';
import { ArrowUp, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';

export function ChatComposer({
  input,
  onInput,
  onSend,
  onStop,
  generating,
  disabled,
  pending = false,
  placeholder,
  toolbar,
  status,
}: {
  input: string;
  onInput: (value: string) => void;
  onSend: () => void;
  onStop: () => void;
  generating: boolean;
  disabled?: boolean;
  pending?: boolean;
  placeholder?: string;
  toolbar?: ReactNode;
  status?: ReactNode;
}) {
  const textarea = useRef<HTMLTextAreaElement>(null);
  const wasGenerating = useRef(generating);
  useLayoutEffect(() => {
    if (!textarea.current) return;
    textarea.current.style.height = 'auto';
    textarea.current.style.height = `${Math.min(textarea.current.scrollHeight, 180)}px`;
  }, [input]);
  useLayoutEffect(() => {
    if (
      wasGenerating.current &&
      !generating &&
      !disabled &&
      document.activeElement === document.body
    )
      textarea.current?.focus();
    wasGenerating.current = generating;
  }, [generating, disabled]);

  return (
    <footer className="safe-bottom shrink-0 px-3 pt-2 sm:px-6">
      <div className="chat-column flex flex-col gap-2">
        <div className="composer-surface">
          <Textarea
            variant="composer"
            ref={textarea}
            value={input}
            onChange={(e) => onInput(e.target.value)}
            onKeyDown={(e) => {
              if (
                e.key === 'Enter' &&
                !e.shiftKey &&
                !e.nativeEvent.isComposing &&
                e.keyCode !== 229
              ) {
                e.preventDefault();
                if (!generating && !disabled && !pending && input.trim()) onSend();
              }
            }}
            aria-label="输入消息"
            placeholder={placeholder ?? '写下你的回应…'}
            rows={2}
            disabled={disabled || generating || pending}
          />
          <div className="mt-2 flex flex-wrap items-center gap-1">
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-0.5">{toolbar}</div>
            {generating ? (
              <Button
                variant="destructive"
                size="icon"
                className="size-9 shrink-0 rounded-xl"
                onClick={onStop}
                aria-label="停止生成"
                title="停止生成"
              >
                <Square />
              </Button>
            ) : (
              <Button
                size="icon"
                className="size-9 shrink-0 rounded-xl"
                onClick={onSend}
                disabled={disabled || pending || !input.trim()}
                aria-label="发送消息"
                title="发送（Enter）"
              >
                <ArrowUp />
              </Button>
            )}
          </div>
        </div>
        <div className="flex min-h-5 items-center justify-between gap-2 px-1 text-[11px] text-muted-foreground">
          <span role="status">
            {generating
              ? '正在回复，可随时停止'
              : disabled
                ? '连接就绪并开始聊天后即可发送'
                : input
                  ? '草稿已保存'
                  : '慢慢聊，故事还很长'}
          </span>
          <span className="hidden shrink-0 sm:inline">Enter 发送 · Shift + Enter 换行</span>
        </div>
        {status}
      </div>
    </footer>
  );
}
