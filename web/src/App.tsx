import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Badge } from '@/components/ui/badge';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Empty, EmptyHeader, EmptyTitle, EmptyDescription } from '@/components/ui/empty';
import { Spinner } from '@/components/ui/spinner';
import {
  Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription,
} from '@/components/ui/sheet';
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuTrigger, DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { ToastHost, pushToast } from './toasts';
import { rpc } from './rpc';
import { useStore } from './store';
import WorldEditor from './WorldEditor';
import { SettingsDialog } from './SettingsDialog';
import {
  MoreVertical, SendHorizontal, ChevronLeft, ChevronRight, RefreshCw,
  Wand2, ArrowRightToLine, Square, BookOpen, Settings, Upload, Trash2,
  MessageSquarePlus, PanelLeft,
} from 'lucide-react';

export default function App() {
  const { connected, characters, activeAvatar, setConnected, loadAll, selectCharacter, importFile } =
    useStore();
  const [dragOver, setDragOver] = useState(false);
  const [showWorlds, setShowWorlds] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showChars, setShowChars] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    rpc.connect();
    const onConnect = rpc.on('$connected', () => {
      setConnected(true);
      loadAll();
    });
    const onDisconnect = () => setConnected(false);
    return () => {
      onConnect();
      onDisconnect();
    };
  }, []);

  const activeChar = characters.find((c) => c.avatar === activeAvatar);

  return (
    <TooltipProvider delayDuration={300}>
      <div
        className="flex h-screen flex-col bg-background"
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragOver(false);
          Array.from(e.dataTransfer.files).forEach((f) => importFile(f));
        }}
      >
        <header className="flex h-10 shrink-0 items-center gap-1 border-b bg-background/80 px-2 backdrop-blur">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="size-7" onClick={() => setShowChars(true)}>
                <PanelLeft />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">角色面板</TooltipContent>
          </Tooltip>
          {activeChar ? (
            <>
              <Separator orientation="vertical" className="h-4" />
              <span className="text-sm font-medium">{activeChar.name}</span>
              {activeChar.tags.slice(0, 2).map((t) => (
                <Badge key={t} variant="secondary" className="hidden px-1.5 text-[10px] md:inline-flex">
                  {t}
                </Badge>
              ))}
            </>
          ) : (
            <span className="text-sm text-muted-foreground">nast</span>
          )}
          <div className="flex-1" />
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="size-7" onClick={() => setShowWorlds(true)}>
                <BookOpen />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">世界书</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="size-7" onClick={() => setShowSettings(true)}>
                <Settings />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">设置</TooltipContent>
          </Tooltip>
          {activeAvatar && <ChatActionsMenu avatar={activeAvatar} />}
        </header>

        {activeAvatar ? (
          <ChatView />
        ) : (
          <div className="flex flex-1 items-center justify-center">
            <Empty className={dragOver ? 'rounded-xl ring-2 ring-primary/50' : ''}>
              <EmptyHeader>
                <EmptyTitle>nast</EmptyTitle>
                <EmptyDescription>
                  点左上角选择角色，或直接把 SillyTavern 角色 PNG / JSON 卡片拖进窗口
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          </div>
        )}
      </div>

      <Sheet open={showChars} onOpenChange={setShowChars}>
        <SheetContent side="left" className="w-72 p-0">
          <SheetHeader className="border-b px-4 py-3">
            <SheetTitle className="text-base">角色</SheetTitle>
            <SheetDescription className="text-xs">
              {connected ? '已连接' : '连接中…'}
            </SheetDescription>
          </SheetHeader>
          <ScrollArea className="h-[calc(100vh-5rem)]">
            <div className="flex flex-col gap-0.5 p-2">
              {characters.map((c) => (
                <button
                  key={c.avatar}
                  onClick={() => {
                    selectCharacter(c.avatar);
                    setShowChars(false);
                  }}
                  className={
                    'flex items-center gap-2.5 rounded-lg px-2 py-2 text-left transition-colors hover:bg-accent ' +
                    (activeAvatar === c.avatar ? 'bg-accent' : '')
                  }
                >
                  <Avatar className="size-9">
                    <AvatarFallback className="bg-primary/20 text-xs text-primary">
                      {c.name.slice(0, 2)}
                    </AvatarFallback>
                  </Avatar>
                  <span className="truncate text-sm">{c.name}</span>
                </button>
              ))}
              {characters.length === 0 && (
                <div className="px-2 py-8 text-center text-xs text-muted-foreground">
                  拖入 PNG / JSON 卡片导入
                </div>
              )}
            </div>
          </ScrollArea>
          <div className="absolute inset-x-0 bottom-0 border-t p-2">
            <Button variant="outline" className="w-full" onClick={() => fileRef.current?.click()}>
              <Upload data-icon="inline-start" />
              导入角色卡
            </Button>
          </div>
        </SheetContent>
      </Sheet>

      {showWorlds && <WorldEditor onClose={() => setShowWorlds(false)} />}
      <SettingsDialog open={showSettings} onOpenChange={setShowSettings} />
      <ToastHost />

      <input
        ref={fileRef}
        type="file"
        accept=".png,.json"
        multiple
        hidden
        onChange={(e) => {
          Array.from(e.target.files ?? []).forEach((f) => importFile(f));
          e.target.value = '';
        }}
      />
    </TooltipProvider>
  );
}

function ChatActionsMenu({ avatar }: { avatar: string }) {
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


function ChatView() {
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
                  size="sm"
                  disabled={generating || swipeIdx <= 0}
                  onClick={() => swipe('left')}
                >
                  <ChevronLeft data-icon="inline-start" />
                  {swipeTotal > 0 && (swipeIdx + 1) + '/' + swipeTotal}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={generating}
                  onClick={() => swipe('right')}
                >
                  {swipeTotal > 0 && (swipeIdx + 1) + '/' + swipeTotal}
                  <ChevronRight data-icon="inline-end" />
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


function MessageBubble({ m }: { m: any }) {
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

function StreamingBubble({ name, text }: { name: string; text: string }) {
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
