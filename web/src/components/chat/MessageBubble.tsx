import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { rpc } from '../../rpc';
import { useStore } from '../../store';

export function MessageBubble({ m }: { m: any }) {
  const { activeAvatar, activeChatName, reloadChat } = useStore();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(m.mes);

  const saveEdit = async () => {
    if (!activeAvatar || !activeChatName) return;
    const raw = await rpc.call<any[]>('chats.get', { avatar: activeAvatar, file_name: activeChatName });
    const idx = raw.findIndex((x, i) => i > 0 && x.mes === m.mes && x.is_user === m.is_user);
    if (idx > 0) {
      raw[idx].mes = draft;
      await rpc.call('chats.save', { avatar: activeAvatar, file_name: activeChatName, chat: raw });
      await reloadChat();
    }
    setEditing(false);
  };

  return (
    <div className={'group flex w-full gap-2 ' + (m.is_user ? 'justify-end' : 'justify-start')}>
      {!m.is_user && (
        <Avatar className="mt-1 size-8 shrink-0">
          <AvatarFallback className="bg-primary/20 text-xs text-primary">
            {m.name.slice(0, 2)}
          </AvatarFallback>
        </Avatar>
      )}
      <div className={'flex max-w-[75%] flex-col gap-1 ' + (m.is_user ? 'items-end' : 'items-start')}>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium text-foreground/80">{m.name}</span>
          <span className="opacity-60">{formatTime(m.send_date)}</span>
        </div>
        {editing ? (
          <div className="flex w-full flex-col gap-2">
            <Textarea value={draft} onChange={(e) => setDraft(e.target.value)} rows={4} className="bg-card" />
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
                取消
              </Button>
              <Button size="sm" onClick={saveEdit}>
                保存
              </Button>
            </div>
          </div>
        ) : (
          <div
            onDoubleClick={() => {
              setDraft(m.mes);
              setEditing(true);
            }}
            className={
              'msg-content whitespace-pre-wrap break-words rounded-2xl px-4 py-2.5 text-sm leading-relaxed ' +
              (m.is_user ? 'rounded-tr-sm bg-primary/25' : 'rounded-tl-sm border bg-card')
            }
          >
            {m.mes}
          </div>
        )}
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
