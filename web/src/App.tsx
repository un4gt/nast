import { useEffect, useState } from 'react';
import { SidebarProvider, SidebarInset } from '@/components/ui/sidebar';
import { Badge } from '@/components/ui/badge';
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty';
import { LeftSidebar } from './components/layout/LeftSidebar';
import { RightInspector } from './components/inspector/RightInspector';
import { ChatArea } from './components/chat/ChatArea';
import { ChatActionsMenu } from './components/chat/ChatActionsMenu';
import { ToastHost } from './toasts';
import { rpc } from './rpc';
import { useStore } from './store';
import WorldEditor from './WorldEditor';
import { SettingsDialog } from './SettingsDialog';

export default function App() {
  const { connected, characters, activeAvatar, activeChatName, setConnected, loadAll, importFile } =
    useStore();
  const [dragOver, setDragOver] = useState(false);
  const [showWorlds, setShowWorlds] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [inspectorCollapsed, setInspectorCollapsed] = useState(() => {
    return localStorage.getItem('nast:inspector_collapsed') === '1';
  });

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

  const toggleInspector = () => {
    setInspectorCollapsed((v) => {
      localStorage.setItem('nast:inspector_collapsed', v ? '0' : '1');
      return !v;
    });
  };

  const activeChar = characters.find((c) => c.avatar === activeAvatar);

  return (
    <SidebarProvider>
      <LeftSidebar onOpenSettings={() => setShowSettings(true)} />

      <SidebarInset>
        <div
          className="flex h-full flex-col"
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
          {/* 极简聊天头 */}
          <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3">
            {activeChar ? (
              <>
                <span className="text-sm font-medium">{activeChar.name}</span>
                {activeChatName && (
                  <span className="truncate text-[10px] text-muted-foreground">
                    {activeChatName.replace(/.jsonl$/, '')}
                  </span>
                )}
                {activeChar.tags.slice(0, 2).map((t) => (
                  <Badge key={t} variant="secondary" className="hidden px-1.5 text-[10px] md:inline-flex">
                    {t}
                  </Badge>
                ))}
                <div className="flex-1" />
                <ChatActionsMenu avatar={activeAvatar ?? ""} />
              </>
            ) : (
              <span className="text-sm text-muted-foreground">nast</span>
            )}
          </div>

          {activeAvatar ? (
            <ChatArea />
          ) : (
            <div className={'flex flex-1 items-center justify-center ' + (dragOver ? 'ring-2 ring-inset ring-primary/50' : '')}>
              <Empty>
                <EmptyHeader>
                  <EmptyTitle>nast</EmptyTitle>
                  <EmptyDescription>
                    点左侧选择角色，或直接把 SillyTavern 角色 PNG / JSON 卡片拖进窗口
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            </div>
          )}
        </div>
      </SidebarInset>

      <RightInspector collapsed={inspectorCollapsed} onToggleCollapse={toggleInspector} onOpenWorldEditor={() => setShowWorlds(true)} />

      {showWorlds && <WorldEditor onClose={() => setShowWorlds(false)} />}
      <SettingsDialog open={showSettings} onOpenChange={setShowSettings} />
      <ToastHost />
      <span className="hidden">{connected}</span>
    </SidebarProvider>
  );
}
