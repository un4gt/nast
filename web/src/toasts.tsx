import { useEffect, useState } from 'react';

export interface Toast {
  id: number;
  message: string;
  kind: 'error' | 'info' | 'success';
}

let nextId = 1;
const listeners = new Set<(t: Toast) => void>();

export function pushToast(message: string, kind: Toast['kind'] = 'error') {
  const t: Toast = { id: nextId++, message, kind };
  listeners.forEach((l) => l(t));
}

export function ToastHost() {
  const [toasts, setToasts] = useState<Toast[]>([]);

  useEffect(() => {
    const on = (t: Toast) => {
      setToasts((cur) => [...cur, t]);
      setTimeout(() => {
        setToasts((cur) => cur.filter((x) => x.id !== t.id));
      }, 5000);
    };
    listeners.add(on);
    return () => listeners.delete(on);
  }, []);

  return (
    <div className="fixed bottom-4 right-4 z-[100] space-y-2 max-w-sm">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`rounded-lg px-4 py-3 text-sm shadow-lg border ${
            t.kind === 'error'
              ? 'bg-red-950/90 border-red-700 text-red-100'
              : t.kind === 'success'
                ? 'bg-emerald-950/90 border-emerald-700 text-emerald-100'
                : 'bg-surface border-line text-fg'
          }`}
        >
          <div className="whitespace-pre-wrap break-words">{t.message}</div>
        </div>
      ))}
    </div>
  );
}
