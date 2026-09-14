// WS RPC 客户端：请求-响应 + 事件订阅
type Pending = { resolve: (v: any) => void; reject: (e: Error) => void; method: string };

type Handler = (data: any) => void;

class RpcClient {
  private ws: WebSocket | null = null;
  private nextId = 1;
  private pending = new Map<string, Pending>();
  private handlers = new Map<string, Set<Handler>>();
  private retryTimer: ReturnType<typeof setTimeout> | null = null;

  connect() {
    if (this.ws) return;
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    this.ws = ws;
    ws.onopen = () => {
      this.emit('$connected', null);
    };
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data as string);
      if (msg.event) {
        this.emit(msg.event, msg.data);
        return;
      }
      const p = this.pending.get(msg.id);
      if (!p) return;
      this.pending.delete(msg.id);
      if (msg.error) {
        // 所有 RPC 错误统一走 rpc_error 事件（全局 toast），调用方仍可 catch 覆盖
        this.emit('$rpc_error', { method: p.method, code: msg.error.code, message: msg.error.message });
        p.reject(new Error(`${msg.error.code}: ${msg.error.message}`));
      } else p.resolve(msg.result);
    };
    ws.onclose = () => {
      this.ws = null;
      this.emit('$disconnected', null);
      if (!this.retryTimer) {
        this.retryTimer = setTimeout(() => {
          this.retryTimer = null;
          this.connect();
        }, 1500);
      }
    };
  }

  private emit(event: string, data: any) {
    this.handlers.get(event)?.forEach((h) => h(data));
  }

  on(event: string, handler: Handler): () => void {
    if (!this.handlers.has(event)) this.handlers.set(event, new Set());
    this.handlers.get(event)!.add(handler);
    return () => this.handlers.get(event)?.delete(handler);
  }

  call<T = any>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error('not connected'));
        return;
      }
      const id = String(this.nextId++);
      this.pending.set(id, { resolve, reject, method });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
}

export const rpc = new RpcClient();
