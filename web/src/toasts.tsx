import { useEffect } from 'react';
import { toast } from 'sonner';
import { rpc } from './rpc';

export type ToastKind = 'error' | 'info' | 'success';

/** 全局 toast（sonner）。 */
export function pushToast(message: string, kind: ToastKind = 'error') {
  if (kind === 'success') toast.success(message);
  else if (kind === 'error') toast.error(message);
  else toast(message);
}

/** 挂载一次：全局 RPC 错误 → toast。 */
export function useRpcErrorToast() {
  useEffect(() => {
    return rpc.on('$rpc_error', (e: { message: string }) => {
      pushToast(e.message, 'error');
    });
  }, []);
}
