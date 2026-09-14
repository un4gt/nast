import { useEffect, useState } from 'react';
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuTrigger, DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';
import { MoreVertical, Trash2, MessageSquarePlus } from 'lucide-react';
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { rpc } from '../../rpc';
import { pushToast } from '../../toasts';
import { useStore } from '../../store';

export function ChatActionsMenu({ avatar }: { avatar: string }) {
  const { chatList, activeChatName, openChat, deleteCharacter, newChat, exportChat } = useStore();
  const [creating, setCreating] = useState(false);
  const [greetOpen, setGreetOpen] = useState(false);
  // 修复：点击历史聊天项直接打开所点文件（原先恒选最新聊天）
  void rpc;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" className="size-8">
          <MoreVertical />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuItem onClick={() => setGreetOpen(true)} disabled={creating}>
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
                  onClick={() => {
                    if (c !== activeChatName) void openChat(avatar, c);
                  }}
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
      <GreetingDialog
        avatar={avatar}
        open={greetOpen}
        onOpenChange={setGreetOpen}
        creating={creating}
        setCreating={setCreating}
      />
    </DropdownMenu>
  );
}

/** 开场白选择：first_mes / alternate_greetings / 随机。 */
function GreetingDialog({
  avatar,
  open,
  onOpenChange,
  creating,
  setCreating,
}: {
  avatar: string;
  open: boolean;
  onOpenChange: (v: boolean) => void;
  creating: boolean;
  setCreating: (v: boolean) => void;
}) {
  const { characters, newChat } = useStore();
  const [greetings, setGreetings] = useState<string[]>([]);
  const ch = characters.find((c) => c.avatar === avatar);

  useEffect(() => {
    if (!open) return;
    rpc
      .call<any>('characters.get', { avatar })
      .then((c) => {
        const list = [c.first_mes, ...(c.data?.alternate_greetings ?? [])].filter(Boolean);
        setGreetings(list);
      })
      .catch(() => setGreetings([]));
  }, [open, avatar]);

  const pick = async (idx: number) => {
    setCreating(true);
    try {
      await newChat(avatar, idx);
      pushToast('已创建新聊天', 'success');
      onOpenChange(false);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setCreating(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>新聊天 — 选择开场白</DialogTitle>
          <DialogDescription>{ch?.name}</DialogDescription>
        </DialogHeader>
        <div className="max-h-80 overflow-y-auto flex flex-col gap-2">
          {greetings.map((g, i) => (
            <button
              key={i}
              onClick={() => {
                void pick(i);
              }}
              disabled={creating}
              className="rounded-lg border bg-card px-3 py-2 text-left text-sm hover:bg-accent/50 disabled:opacity-50"
            >
              <span className="line-clamp-3 whitespace-pre-wrap">{g}</span>
            </button>
          ))}
          <Button
            variant="secondary"
            onClick={() => {
              void pick(-1);
            }}
            disabled={creating}
          >
            {creating ? <Spinner /> : null}
            随机开场白
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
