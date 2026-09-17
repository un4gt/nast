import { useMemo, useState } from 'react';
import { BrainCog } from 'lucide-react';
import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Spinner } from '@/components/ui/spinner';
import {
  Collapsible, CollapsibleContent, CollapsibleTrigger,
} from '@/components/ui/collapsible';
import { fixMarkdownQuotes } from '@/lib/st-display';

export function StreamingBubble({
  name, text, reasoning,
}: { name: string; text: string; reasoning?: string | null }) {
  const [reasonOpen, setReasonOpen] = useState(true);
  const rendered = useMemo(() => {
    if (!text) return '';
    const html = marked.parse(fixMarkdownQuotes(text), { async: false }) as string;
    return DOMPurify.sanitize(html);
  }, [text]);
  return (
    <div className="flex w-full justify-start gap-2">
      <Avatar className="mt-1 size-8 shrink-0">
        <AvatarFallback className="bg-primary/20 text-xs text-primary">
          {name.slice(0, 2) || '…'}
        </AvatarFallback>
      </Avatar>
      <div className="flex max-w-[75%] flex-col items-start gap-1">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground/80">{name}</span>
          <Spinner className="size-3" />
        </div>
        {reasoning ? (
          <Collapsible open={reasonOpen} onOpenChange={setReasonOpen} className="w-full">
            <CollapsibleTrigger className="flex items-center gap-1.5 rounded-md border bg-muted/40 px-2 py-1 text-[11px] text-muted-foreground hover:bg-muted">
              <BrainCog className="size-3" />
              思考中…
            </CollapsibleTrigger>
            <CollapsibleContent>
              <pre className="mt-1 max-h-36 w-full overflow-auto whitespace-pre-wrap break-words rounded-md border bg-muted/20 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
                {reasoning}
              </pre>
            </CollapsibleContent>
          </Collapsible>
        ) : null}
        <div
          className="msg-content break-words rounded-2xl rounded-tl-sm border bg-card px-4 py-2.5 text-sm leading-relaxed"
          dangerouslySetInnerHTML={{ __html: rendered || '…' }}
        />
      </div>
    </div>
  );
}
