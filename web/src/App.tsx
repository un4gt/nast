import { useEffect, useRef, useState } from 'react';
import { rpc } from './rpc';
import { useStore } from './store';
import WorldEditor from './WorldEditor';
import { ToastHost, pushToast } from './toasts';

export default function App() {
  const {
    connected, characters, activeAvatar,
    setConnected, loadCharacters, selectCharacter, importFile,
  } = useStore();
  const [dragOver, setDragOver] = useState(false);
  const [showWorlds, setShowWorlds] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    rpc.connect();
    const onConnect = rpc.on('$connected', () => {
      setConnected(true);
      loadCharacters();
    });
    const onDisconnect = rpc.on('$disconnected', () => setConnected(false));
    const onToken = rpc.on('stream_token_received', (data: { text: string }) => {
      useStore.getState().appendStreamToken(data.text);
    });
    // 全局 RPC 错误 → toast（调用方未 catch 的也至少有提示）
    const onRpcError = rpc.on('$rpc_error', (e: { method: string; message: string }) => {
      pushToast(`${e.message}`, 'error');
    });
    return () => { onConnect(); onDisconnect(); onToken(); onRpcError(); };
  }, []);

  return (
    <div className="flex h-screen">
      <ToastHost />
      {/* 角色侧栏 */}
      <aside className="w-64 shrink-0 border-r border-line bg-surface flex flex-col">
        <header className="px-4 py-3 border-b border-line">
          <h1 className="text-lg font-semibold">nast</h1>
          <p className={`text-xs ${connected ? 'text-emerald-400' : 'text-red-400'}`}>
            {connected ? '已连接' : '连接中…'}
          </p>
        </header>
        <div
          className={`flex-1 overflow-y-auto p-2 space-y-1 ${dragOver ? 'bg-accent/20' : ''}`}
          onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
          onDragLeave={() => setDragOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragOver(false);
            Array.from(e.dataTransfer.files).forEach((f) => importFile(f));
          }}
        >
          {characters.map((c) => (
            <button
              key={c.avatar}
              onClick={() => selectCharacter(c.avatar)}
              className={`w-full text-left px-3 py-2 rounded-lg transition-colors ${
                activeAvatar === c.avatar ? 'bg-raised' : 'hover:bg-raised/60'
              }`}
            >
              <div className="font-medium text-sm">{c.name}</div>
              <div className="text-xs text-muted line-clamp-1">{c.description || '—'}</div>
            </button>
          ))}
          {characters.length === 0 && (
            <div className="text-xs text-muted text-center pt-8 px-4">
              拖入 ST 角色 PNG/JSON 卡片导入
            </div>
          )}
        </div>
        <button
          onClick={() => setShowWorlds(true)}
          className="m-2 mb-0 py-2 rounded-lg bg-raised text-fg text-sm hover:bg-line"
        >
          世界书管理
        </button>
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
        <button
          onClick={() => fileRef.current?.click()}
          className="m-2 py-2 rounded-lg bg-accent text-white text-sm font-medium hover:opacity-90"
        >
          导入角色卡
        </button>
      </aside>

      {/* 世界书编辑器 */}
      {showWorlds && <WorldEditor onClose={() => setShowWorlds(false)} />}

      {/* 聊天区 */}
      <main className="flex-1 flex flex-col min-w-0">
        {activeAvatar ? <ChatView /> : (
          <div className="flex-1 flex items-center justify-center text-muted">
            选择一个角色开始
          </div>
        )}
      </main>
    </div>
  );
}

function ChatView() {
  const {
    messages, activeChatName, streamingText, generating,
    send, swipe, regenerate, impersonate,
  } = useStore();
  const [input, setInput] = useState('');
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length, streamingText]);

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

  // 最后一条消息是否为 AI 消息（决定 swipe/regenerate 可用性）
  const lastMsg = messages[messages.length - 1];
  const canSwipe = lastMsg && !lastMsg.is_user;

  return (
    <>
      <header className="px-4 py-3 border-b border-line text-sm text-muted">
        {activeChatName?.replace(/\.jsonl$/, '')}
      </header>
      <div className="flex-1 overflow-y-auto p-4 space-y-3">
        {messages.map((m, i) => {
          // 流式期间最后一条 AI 消息由 streaming 气泡接管
          const isLastAi = i === messages.length - 1 && !m.is_user;
          const hidden = isLastAi && generating && streamingText !== null;
          if (hidden) return null;
          return (
            <div
              key={i}
              className={`max-w-[75%] rounded-xl px-4 py-2 text-sm leading-relaxed ${
                m.is_user ? 'ml-auto bg-accent/30' : 'bg-raised'
              }`}
            >
              <div className="text-xs text-muted mb-0.5">{m.name}</div>
              <div className="whitespace-pre-wrap">{m.mes}</div>
            </div>
          );
        })}
        {/* 流式气泡 */}
        {generating && streamingText !== null && (
          <div className="max-w-[75%] rounded-xl px-4 py-2 text-sm bg-raised">
            <div className="text-xs text-muted mb-0.5 animate-pulse">生成中…</div>
            <div className="whitespace-pre-wrap">{streamingText}</div>
          </div>
        )}
        <div ref={bottomRef} />
      </div>
      {/* 输入区 */}
      <footer className="border-t border-line p-3 space-y-2">
        <div className="flex gap-2">
          <textarea
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
            className="flex-1 resize-none bg-raised rounded-lg px-3 py-2 text-sm outline-none focus:ring-1 focus:ring-accent disabled:opacity-50"
          />
          <button
            onClick={doSend}
            disabled={generating || !input.trim()}
            className="px-4 rounded-lg bg-accent text-white text-sm font-medium hover:opacity-90 disabled:opacity-40 self-end"
          >
            发送
          </button>
        </div>
        <div className="flex gap-2 text-xs">
          <button
            onClick={swipe}
            disabled={generating || !canSwipe}
            className="px-3 py-1.5 rounded-lg bg-raised hover:bg-line disabled:opacity-40"
            title="重新生成最后一条回复"
          >
            ⟲ Swipe
          </button>
          <button
            onClick={regenerate}
            disabled={generating || !canSwipe}
            className="px-3 py-1.5 rounded-lg bg-raised hover:bg-line disabled:opacity-40"
          >
            ↻ Regenerate
          </button>
          <button
            onClick={doImpersonate}
            disabled={generating}
            className="px-3 py-1.5 rounded-lg bg-raised hover:bg-line disabled:opacity-40"
            title="以用户身份生成一条发言填入输入框"
          >
            ✦ Impersonate
          </button>
        </div>
      </footer>
    </>
  );
}
