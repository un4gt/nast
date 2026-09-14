import { useEffect, useState } from 'react';
import { rpc } from './rpc';

export interface Toast {
  id: number;
  message: string;
  kind: 'error' | 'info' | 'success';
}

type Listener = (t: Toast) => void;
const listeners = new Set<Listener>();
let nextId = 1;

export function pushToast(message: string, kind: Toast['kind'] = 'error') {
  const t: Toast = { id: nextId++, message, kind };
  listeners.forEach((l) => l(t));
}

export function useToasts(): Toast[] {
  const [toasts, setToasts] = useState<Toast[]>([]);
  useEffect(() => {
    const on = (t: Toast) => {
      setToasts((cur) => [...cur, t]);
      setTimeout(() => setToasts((cur) => cur.filter((x) => x.id !== t.id)), 5000);
    };
    listeners.add(on);
    return () => {
      listeners.delete(on);
    };
  }, []);
  return toasts;
}

/** 挂载一次：全局 RPC 错误 → toast。 */
export function useRpcErrorToast() {
  useEffect(() => {
    return rpc.on('$rpc_error', (e: { message: string }) => {
      pushToast(e.message, 'error');
    });
  }, []);
}

export function ToastHost() {
  const toasts = useToasts();
  useRpcErrorToast();

  return (
    <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 max-w-sm">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`rounded-lg border px-4 py-3 text-sm shadow-lg ${
            t.kind === 'error'
              ? 'border-destructive/60 bg-destructive/15 text-destructive'
              : t.kind === 'success'
                ? 'border-emerald-700 bg-emerald-950/60 text-emerald-100'
                : 'border-border bg-card text-foreground'
          }`}
        >
          <div className="whitespace-pre-wrap break-words">{t.message}</div>
        </div>
      ))}
    </div>
  );
}
