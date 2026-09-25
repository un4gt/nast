// WS RPC 客户端：请求-响应 + 事件订阅
type Pending = { resolve: (v: any) => void; reject: (e: Error) => void; method: string };

type Handler = (data: any) => void;

class RpcClient {
  private ws: WebSocket | null = null;
  private nextId = 1;
  private pending = new Map<string, Pending>();
  private handlers = new Map<string, Set<Handler>>();
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private retries = 0;
  private stopped = false;

  disconnect() {
    this.stopped = true;
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    const ws = this.ws;
    this.ws = null;
    ws?.close();
    for (const pending of this.pending.values()) pending.reject(new Error('连接已关闭'));
    this.pending.clear();
    this.emit('$disconnected', null);
  }

  connect() {
    if (this.ws) return;
    this.stopped = false;
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    this.ws = ws;
    ws.onopen = () => {
      if (this.ws !== ws) return;
      this.retries = 0;
      this.emit('$connected', null);
    };
    ws.onmessage = (ev) => {
      if (this.ws !== ws) return;
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
        this.emit('$rpc_error', { method: p.method, code: msg.error.code, message: msg.error.message, diagnostic: msg.error.diagnostic });
        p.reject(new Error(`${msg.error.code}: ${msg.error.message}`));
      } else p.resolve(msg.result);
    };
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      for (const pending of this.pending.values()) pending.reject(new Error('连接已断开；已提交的生成不会自动重发'));
      this.pending.clear();
      this.emit('$disconnected', null);
      void fetch('/api/auth/session', { cache: 'no-store' }).then(async response => {
        if (response.ok && !(await response.json()).authenticated && !this.stopped) {
          this.disconnect();
          window.dispatchEvent(new Event('nast:auth-required'));
        }
      }).catch(() => {});
      if (!this.retryTimer && !this.stopped) {
        // 指数退避 + 抖动：0.5s 起、上限 10s
        const backoff = Math.min(10000, 500 * 2 ** this.retries);
        const delay = backoff * (0.75 + Math.random() * 0.5);
        this.retries = Math.min(this.retries + 1, 15);
        this.retryTimer = setTimeout(() => {
          this.retryTimer = null;
          if (!this.stopped) this.connect();
        }, delay);
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
