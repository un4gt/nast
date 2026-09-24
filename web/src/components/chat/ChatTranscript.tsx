import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { ArrowDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';

/** 沿用本地 Radix 滚动区；阅读历史时让出滚动控制权。 */
export function ChatTranscript({ children }: { children: ReactNode }) {
  const viewport = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const [showLatest, setShowLatest] = useState(false);

  useLayoutEffect(() => {
    const element = viewport.current;
    if (element && following.current) element.scrollTop = element.scrollHeight;
  }, [children]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScrollArea
        className="chat-transcript min-h-0 flex-1"
        viewportRef={viewport}
        viewportProps={{
          tabIndex: 0,
          role: 'region',
          'aria-label': '聊天消息',
          onScroll: (event) => {
            const element = event.currentTarget;
            following.current =
              element.scrollHeight - element.scrollTop - element.clientHeight < 64;
            setShowLatest(!following.current);
          },
        }}
      >
        <div className="chat-column flex min-w-0 flex-col gap-5 px-4 py-6 sm:px-7">{children}</div>
      </ScrollArea>
      {showLatest && (
        <div className="pointer-events-none absolute inset-x-0 bottom-4 flex justify-center">
          <Button
            variant="outline"
            size="sm"
            className="pointer-events-auto rounded-full shadow-md"
            onClick={() => {
              following.current = true;
              viewport.current?.scrollTo({
                top: viewport.current.scrollHeight,
                behavior: 'instant',
              });
              setShowLatest(false);
            }}
          >
            <ArrowDown data-icon="inline-start" />
            回到最新消息
          </Button>
        </div>
      )}
    </div>
  );
}
