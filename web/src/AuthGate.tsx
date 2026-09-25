import { useEffect, useState, type FormEvent } from 'react';
import { LogIn } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card';
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Spinner } from '@/components/ui/spinner';
import App from './App';
import { rpc } from './rpc';
import { pushToast } from './toasts';

type Session = { enabled: boolean; authenticated: boolean; username: string | null };
export default function AuthGate() {
  const [session, setSession] = useState<Session | null>(null);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(true);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setChecking(true);
    void fetch('/api/auth/session', { cache: 'no-store', signal: controller.signal })
      .then(async response => {
        if (!response.ok) throw new Error('无法连接服务，请稍后重试');
        setSession(await response.json()); setError('');
      }).catch(e => { if (!controller.signal.aborted) setError(e instanceof Error ? e.message : '连接失败'); })
      .finally(() => { if (!controller.signal.aborted) setChecking(false); });
    const expired = () => { rpc.disconnect(); window.location.reload(); };
    window.addEventListener('nast:auth-required', expired);
    return () => { controller.abort(); window.removeEventListener('nast:auth-required', expired); };
  }, [retry]);

  async function login(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    setBusy(true); setError('');
    try {
      const response = await fetch('/api/auth/login', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-NAST-Request': '1' }, body: JSON.stringify({ username, password }) });
      if (!response.ok) {
        const detail = await response.json().catch(() => ({}));
        throw new Error(detail.error || '登录失败，请稍后重试');
      }
      setPassword(''); setRetry(value => value + 1);
    } catch (e) { setError(e instanceof Error ? e.message : '无法连接服务'); }
    finally { setBusy(false); }
  }
  async function logout() {
    try {
      const response = await fetch('/api/auth/logout', { method: 'POST', headers: { 'X-NAST-Request': '1' } });
      if (!response.ok && response.status !== 401) throw new Error('退出失败，请重试');
      rpc.disconnect(); window.location.reload();
    } catch (e) { pushToast(e instanceof Error ? e.message : '退出失败，请重试', 'error'); }
  }

  if (session?.authenticated) return <App onLogout={session.enabled ? () => void logout() : undefined} />;
  return (
    <main className="flex min-h-dvh items-center justify-center bg-background p-5">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle><h1>登录 NAST</h1></CardTitle>
          <CardDescription>登录后继续你的角色与对话。</CardDescription>
        </CardHeader>
        {checking ? <CardContent><p role="status" className="flex items-center gap-2 text-sm text-muted-foreground"><Spinner />正在连接…</p></CardContent> :
          <form onSubmit={event => void login(event)}>
            <CardContent>
              <FieldGroup>
                {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
                {session && <>
                  <Field data-invalid={!!error} data-disabled={busy}>
                    <FieldLabel htmlFor="username">用户名</FieldLabel>
                    <Input id="username" name="username" autoComplete="username" autoFocus required maxLength={256} value={username} onChange={e => setUsername(e.target.value)} disabled={busy} aria-invalid={!!error} />
                  </Field>
                  <Field data-invalid={!!error} data-disabled={busy}>
                    <FieldLabel htmlFor="password">密码</FieldLabel>
                    <Input id="password" name="password" type="password" autoComplete="current-password" required maxLength={1024} value={password} onChange={e => setPassword(e.target.value)} disabled={busy} aria-invalid={!!error} />
                  </Field>
                </>}
              </FieldGroup>
            </CardContent>
            <CardFooter>
              {session ? <Button type="submit" className="w-full" disabled={busy || !username || !password}>
                {busy ? <Spinner /> : <LogIn data-icon="inline-start" />}{busy ? '正在登录…' : '登录'}
              </Button> : <Button type="button" className="w-full" onClick={() => setRetry(value => value + 1)}>重新连接</Button>}
            </CardFooter>
          </form>}
      </Card>
    </main>
  );
}
