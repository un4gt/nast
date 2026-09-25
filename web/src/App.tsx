import { useEffect, useRef, useState } from 'react';
import { SidebarProvider, SidebarInset, SidebarTrigger } from '@/components/ui/sidebar';
import { Badge } from '@/components/ui/badge';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Toaster } from '@/components/ui/sonner';
import { LeftSidebar } from './components/layout/LeftSidebar';
import { RightInspector, InspectorMobileSheet } from './components/inspector/RightInspector';
import { ChatArea } from './components/chat/ChatArea';
import { GroupChatArea } from './components/chat/GroupChatArea';
import { ChatActionsMenu } from './components/chat/ChatActionsMenu';
import { Button } from '@/components/ui/button';
import { PanelRight, Upload, Users } from 'lucide-react';
import { useRpcErrorToast } from './toasts';
import { rpc } from './rpc';
import { useStore } from './store';
import WorldEditor from './WorldEditor';
import { SettingsSheet } from './components/settings/SettingsSheet';
import { WelcomeScreen } from './components/layout/WelcomeScreen';
import { TtsControls } from './components/chat/TtsControls';
import { useAutoTts } from './tts/use-auto-tts';

export default function App({ onLogout }: { onLogout?: () => void }) {
  const {
    characters,
    groups,
    activeGroupId,
    activeAvatar,
    activeChatName,
    connected,
    generating,
    setConnected,
    loadAll,
    importFile,
  } = useStore();
  const [dragOver, setDragOver] = useState(false);
  const dragDepth = useRef(0);
  const [showWorlds, setShowWorlds] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [settingsSection, setSettingsSection] = useState<string>();
  useEffect(() => {
    const open = () => { setSettingsSection('connection'); setShowSettings(true); };
    window.addEventListener('nast:open-model-settings', open);
    return () => window.removeEventListener('nast:open-model-settings', open);
  }, []);
  const [showInspectorSheet, setShowInspectorSheet] = useState(false);
  const [inspectorCollapsed, setInspectorCollapsed] = useState(() => {
    return localStorage.getItem('nast:inspector_collapsed') === '1';
  });

  useRpcErrorToast();
  useAutoTts();

  useEffect(() => {
    // 外观设置恢复（localStorage → CSS 变量）
    const fs = localStorage.getItem('nast:font_scale');
    if (fs)
      document.documentElement.style.setProperty('--chat-font-scale', String(Number(fs) / 100));
    const cw = localStorage.getItem('nast:chat_width');
    if (cw) document.documentElement.style.setProperty('--chat-width', `${cw}%`);
  }, []);

  useEffect(() => {
    rpc.connect();
    const onConnect = rpc.on('$connected', () => {
      setConnected(true);
      void loadAll().catch(() => {});
      // 断线重连：若服务端生成仍在进行，恢复流式气泡
      rpc
        .call<{
          running: boolean;
          text: string;
          reasoning?: string;
          info?: { avatar: string; chat_file: string; is_group: boolean } | null;
        }>('generate.status', {})
        .then((st) => {
          if (st.running && !useStore.getState().generating) {
            useStore.setState({
              generating: true,
              streamingText: st.text ?? '',
              streamingReasoning: st.reasoning ?? '',
            });
            // 生成实际由旧连接发起；轮询直至结束
            const poll = setInterval(() => {
              rpc
                .call<{ running: boolean; text: string; reasoning?: string }>('generate.status', {})
                .then((s) => {
                  if (!s.running) {
                    clearInterval(poll);
                    useStore.setState({
                      generating: false,
                      streamingText: null,
                      streamingReasoning: null,
                    });
                    void useStore.getState().reloadChat();
                    const info = st.info;
                    if (info?.is_group) {
                      const g = useStore.getState().groups.find((x) => x.id === info.avatar);
                      if (g) void useStore.getState().openGroup(g.id);
                    }
                  } else {
                    useStore.setState({ streamingText: s.text ?? '', streamingReasoning: s.reasoning ?? '' });
                  }
                })
                .catch(() => {});
            }, 1000);
          }
        })
        .catch(() => {});
    });
    const onDisconnect = rpc.on('$disconnected', () => setConnected(false));
    return () => {
      onConnect();
      onDisconnect();
      rpc.disconnect();
    };
  }, []);

  const toggleInspector = () => {
    setInspectorCollapsed((v) => {
      localStorage.setItem('nast:inspector_collapsed', v ? '0' : '1');
      return !v;
    });
  };

  const activeChar = characters.find((c) => c.avatar === activeAvatar);
  const activeGroup = groups.find((g) => g.id === activeGroupId);

  return (
    <SidebarProvider
      className="app-shell"
      style={{ '--sidebar-width': '17rem' } as React.CSSProperties}
    >
      <LeftSidebar onOpenSettings={() => setShowSettings(true)} onLogout={onLogout} />

      <SidebarInset className="min-w-0 overflow-hidden">
        <div
          className="relative flex h-full min-h-0 flex-col"
          onDragEnter={(e) => {
            if (!e.dataTransfer.types.includes('Files')) return;
            e.preventDefault();
            dragDepth.current += 1;
            setDragOver(true);
          }}
          onDragOver={(e) => {
            if (!e.dataTransfer.types.includes('Files')) return;
            e.preventDefault();
          }}
          onDragLeave={() => {
            dragDepth.current = Math.max(0, dragDepth.current - 1);
            if (!dragDepth.current) setDragOver(false);
          }}
          onDrop={(e) => {
            e.preventDefault();
            dragDepth.current = 0;
            setDragOver(false);
            Array.from(e.dataTransfer.files).forEach((f) => void importFile(f).catch(() => {}));
          }}
        >
          <header className="flex h-16 shrink-0 items-center gap-3 border-b bg-card/60 px-3 sm:px-5">
            <SidebarTrigger className="size-9 shrink-0" title="切换侧栏（Ctrl / ⌘ + B）" />
            {(activeChar || activeGroup) && (
              <Avatar className="hidden size-9 shrink-0 sm:flex">
                {activeChar && <AvatarImage src={activeChar.avatarUrl} alt={activeChar.name} />}
                <AvatarFallback>
                  {activeGroup ? <Users className="size-4" /> : activeChar?.name.slice(0, 1)}
                </AvatarFallback>
              </Avatar>
            )}
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <h1 className="truncate text-sm font-semibold">
                {activeGroup?.name ?? activeChar?.name ?? '对话空间'}
              </h1>
              <p
                className="truncate text-xs text-muted-foreground"
                title={activeChatName?.replace(/\.jsonl$/, '')}
              >
                {activeGroup
                  ? `${activeGroup.members.length} 名成员 · 群组对话`
                  : activeChar
                    ? (activeChatName?.replace(/\.jsonl$/, '') ?? '开始一段新的对话')
                    : '让每一个角色，都有故事可讲'}
              </p>
            </div>
            <div className="hidden items-center gap-1.5 lg:flex">
              {activeChar?.tags.slice(0, 2).map((t) => (
                <Badge key={t} variant="secondary" className="max-w-24 truncate">
                  {t}
                </Badge>
              ))}
            </div>
            <span
              className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground"
              role="status"
              title={connected ? '已连接到服务端' : '连接断开，正在自动重连'}
            >
              <span className="connection-dot" data-connected={connected} />
              <span className="hidden sm:inline">
                {connected ? (generating ? '正在生成' : '已连接') : '重连中'}
              </span>
              <span className="sr-only sm:hidden">{connected ? '已连接' : '正在重连'}</span>
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="size-9 shrink-0 xl:hidden"
              onClick={() => setShowInspectorSheet(true)}
              title="对话详情"
              aria-label="打开对话详情"
            >
              <PanelRight />
            </Button>
            {!activeGroup && activeAvatar && <ChatActionsMenu avatar={activeAvatar ?? ''} />}
          </header>

          <TtsControls onSettings={() => { setSettingsSection('tts'); setShowSettings(true); }} />

          {activeGroup ? (
            <GroupChatArea key={activeGroupId} />
          ) : activeAvatar ? (
            <ChatArea key={`${activeAvatar}:${activeChatName}`} />
          ) : (
            <WelcomeScreen onOpenSettings={() => setShowSettings(true)} />
          )}
          {dragOver && (
            <div className="pointer-events-none absolute inset-2 z-20 flex flex-col items-center justify-center gap-3 rounded-2xl border-2 border-dashed border-primary bg-background/95">
              <Upload className="size-9 text-primary" />
              <p className="font-medium">松开，导入角色卡</p>
              <p className="text-sm text-muted-foreground">支持 SillyTavern PNG / JSON 格式</p>
            </div>
          )}
        </div>
      </SidebarInset>

      <RightInspector
        collapsed={inspectorCollapsed}
        onToggleCollapse={toggleInspector}
        onOpenWorldEditor={() => setShowWorlds(true)}
      />

      {showWorlds && <WorldEditor onClose={() => setShowWorlds(false)} />}
      <SettingsSheet open={showSettings} initialSection={settingsSection} onOpenChange={(open) => { setShowSettings(open); if (!open) setSettingsSection(undefined); }} />
      <InspectorMobileSheet
        open={showInspectorSheet}
        onOpenChange={setShowInspectorSheet}
        onOpenWorldEditor={() => {
          setShowInspectorSheet(false);
          setShowWorlds(true);
        }}
      />
      <Toaster
        position="top-right"
        offset={76}
        mobileOffset={{ top: 76, right: 12, left: 12 }}
        richColors
        closeButton
      />
    </SidebarProvider>
  );
}
