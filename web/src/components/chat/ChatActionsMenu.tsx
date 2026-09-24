import { useEffect, useState } from 'react';
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuTrigger, DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from '@/components/ui/dialog';
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Spinner } from '@/components/ui/spinner';
import {
  Copy, Download, FileText, MessageSquarePlus, MoreVertical, Pencil, Trash2,
} from 'lucide-react';
import { rpc } from '../../rpc';
import { pushToast } from '../../toasts';
import { useStore } from '../../store';

export function ChatActionsMenu({ avatar }: { avatar: string }) {
  const {
    chatList, activeChatName, openChat, deleteCharacter, newChat, exportChat,
    loadAll,
  } = useStore();
  const [creating, setCreating] = useState(false);
  const [greetOpen, setGreetOpen] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState('');
  const [deleteChatOpen, setDeleteChatOpen] = useState(false);
  const [deleteCharOpen, setDeleteCharOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  const chatName = activeChatName?.replace(/\.jsonl$/, '') ?? '';

  const doRename = async () => {
    if (!activeChatName || !renameValue.trim()) return;
    setBusy(true);
    try {
      const r = await rpc.call<{ name: string }>('chats.rename', {
        avatar,
        original_file: activeChatName,
        renamed_file: renameValue.trim(),
      });
      await openChat(avatar, `${r.name}.jsonl`);
      pushToast('聊天已重命名', 'success');
      setRenameOpen(false);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
    }
  };

  const doDeleteChat = async () => {
    if (!activeChatName) return;
    setBusy(true);
    try {
      await rpc.call('chats.delete', { avatar, file_name: activeChatName });
      const rest = await rpc.call<string[]>('characters.chats', { avatar });
      if (rest.length) {
        await openChat(avatar, rest[rest.length - 1]);
      } else {
        await newChat(avatar, -1);
      }
      pushToast('聊天已删除', 'success');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
      setDeleteChatOpen(false);
    }
  };

  const doDuplicate = async () => {
    setBusy(true);
    try {
      await rpc.call('characters.duplicate', { avatar });
      await loadAll();
      pushToast('角色已复制', 'success');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
    }
  };

  const exportTxt = async () => {
    if (!activeChatName) return;
    const raw = await rpc.call<any[]>('chats.get', { avatar, file_name: activeChatName });
    const lines = raw
      .slice(1)
      .map((m) => `${m.is_user ? 'You' : m.name}: ${m.mes}`)
      .join('\n\n');
    const blob = new Blob([lines], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${chatName}.txt`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" className="size-9 shrink-0" aria-label="聊天操作" title="聊天操作">
          <MoreVertical />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuItem onClick={() => setGreetOpen(true)} disabled={creating}>
          <MessageSquarePlus />
          新聊天
        </DropdownMenuItem>
        {activeChatName && (
          <>
            <DropdownMenuItem
              onClick={() => {
                setRenameValue(chatName);
                setRenameOpen(true);
              }}
            >
              <Pencil />
              重命名聊天
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => void exportChat()}>
              <Download />
              导出聊天（jsonl）
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => void exportTxt()}>
              <FileText />
              导出聊天（txt）
            </DropdownMenuItem>
            <DropdownMenuItem
              className="text-destructive focus:text-destructive"
              onClick={() => setDeleteChatOpen(true)}
            >
              <Trash2 />
              删除聊天
            </DropdownMenuItem>
          </>
        )}
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
                  <span className="truncate">{c.replace(/\.jsonl$/, '')}</span>
                </DropdownMenuItem>
              ))}
            </div>
            <DropdownMenuSeparator />
          </>
        )}
        <DropdownMenuItem onClick={() => void doDuplicate()} disabled={busy}>
          <Copy />
          复制角色
        </DropdownMenuItem>
        <DropdownMenuItem
          className="text-destructive focus:text-destructive"
          onClick={() => setDeleteCharOpen(true)}
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

      <Dialog open={renameOpen} onOpenChange={setRenameOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>重命名聊天</DialogTitle>
            <DialogDescription>{chatName}</DialogDescription>
          </DialogHeader>
          <Input
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void doRename();
            }}
            placeholder="新名称"
          />
          <DialogFooter>
            <Button variant="ghost" onClick={() => setRenameOpen(false)}>取消</Button>
            <Button onClick={() => void doRename()} disabled={busy || !renameValue.trim()}>
              {busy ? <Spinner /> : null}
              重命名
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={deleteChatOpen} onOpenChange={setDeleteChatOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除此聊天？</AlertDialogTitle>
            <AlertDialogDescription>
              「{chatName}」将被永久删除（备份目录可能保留节流副本）。此操作不可撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => void doDeleteChat()}
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={deleteCharOpen} onOpenChange={setDeleteCharOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除角色？</AlertDialogTitle>
            <AlertDialogDescription>
              角色卡与其全部聊天记录将被永久删除。此操作不可撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => {
                setDeleteCharOpen(false);
                void deleteCharacter(avatar);
              }}
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
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
        <div className="flex max-h-80 flex-col gap-2 overflow-y-auto">
          {greetings.map((g, i) => (
            <Button
              key={i}
              variant="outline"
              className="h-auto justify-start whitespace-normal py-2 text-left font-normal"
              onClick={() => void pick(i)}
              disabled={creating}
            >
              <span className="line-clamp-3 whitespace-pre-wrap">{g}</span>
            </Button>
          ))}
          <Button
            variant="secondary"
            onClick={() => void pick(-1)}
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
