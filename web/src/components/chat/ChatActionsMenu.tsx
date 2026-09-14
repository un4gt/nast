import { useState } from 'react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuTrigger, DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';
import { MoreVertical, Trash2, MessageSquarePlus } from 'lucide-react';
import { rpc } from '../../rpc';
import { pushToast } from '../../toasts';
import { useStore } from '../../store';

export function ChatActionsMenu({ avatar }: { avatar: string }) {
  const { chatList, activeChatName, selectCharacter, deleteCharacter } = useStore();
  const [creating, setCreating] = useState(false);

  const newChat = async () => {
    setCreating(true);
    try {
      const newFile = Date.now() + '.jsonl';
      const cur = await rpc.call<any[]>('chats.get', { avatar, file_name: activeChatName });
      await rpc.call('chats.save', { avatar, file_name: newFile, chat: [cur[0]] });
      await selectCharacter(avatar);
      pushToast('已创建新聊天', 'success');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setCreating(false);
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" className="size-8">
          <MoreVertical />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuItem onClick={() => { void newChat(); }} disabled={creating}>
          <MessageSquarePlus />
          新聊天
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        {chatList.length > 0 && (
          <>
            <div className="px-2 py-1.5 text-xs text-muted-foreground">历史聊天</div>
            <div className="max-h-48 overflow-y-auto">
              {chatList.map((c) => (
                <DropdownMenuItem
                  key={c}
                  onClick={() => selectCharacter(avatar)}
                  className={c === activeChatName ? 'bg-accent' : ''}
                >
                  <MessageSquarePlus />
                  <span className="truncate">{c.replace(/.json$/, '')}</span>
                </DropdownMenuItem>
              ))}
            </div>
            <DropdownMenuSeparator />
          </>
        )}
        <DropdownMenuItem
          className="text-destructive focus:text-destructive"
          onClick={() => {
            if (activeChatName) void deleteCharacter(avatar);
          }}
        >
          <Trash2 />
          删除角色
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
