import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Spinner } from '@/components/ui/spinner';

export function StreamingBubble({ name, text }: { name: string; text: string }) {
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
        <div className="msg-content whitespace-pre-wrap break-words rounded-2xl rounded-tl-sm border bg-card px-4 py-2.5 text-sm leading-relaxed">
          {text || '…'}
        </div>
      </div>
    </div>
  );
}
