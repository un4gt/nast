import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { downloadBase64 } from '@/lib/download';
import { rpc } from '@/rpc';
import { useStore } from '@/store';
import { pushToast } from '@/toasts';

export function GroupSessionTools() {
  const { groups, activeGroupId, generating, connected, chatMetadata, openGroup } = useStore();
  const group = groups.find((item) => item.id === activeGroupId);
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('');
  const [worlds, setWorlds] = useState<string[]>([]);
  const fileInput = useRef<HTMLInputElement>(null);
  useEffect(() => { setName(group?.chat_id ?? ''); }, [group?.chat_id]);
  useEffect(() => { void rpc.call<string[]>('worlds.list', {}).then(setWorlds).catch(() => {}); }, [activeGroupId]);
  if (!group) return null;
  const disabled = busy || generating || !connected;
  const operate = async (method: string, params: Record<string, unknown> = {}) => {
    if (disabled) return;
    setBusy(true);
    try {
      const result = await rpc.call<any>(method, { id: group.id, chat_id: group.chat_id, ...params });
      if (method === 'groups.export_chat') downloadBase64(result);
      else await openGroup(group.id);
      if (method === 'groups.delete_chat') pushToast('聊天已删除，可从服务端 backups 目录恢复', 'success');
    } catch (e) { pushToast(e instanceof Error ? e.message : String(e), 'error'); }
    finally { setBusy(false); }
  };
  return <details className="shrink-0 border-b px-4 py-2 sm:px-6">
    <summary className="cursor-pointer text-xs text-muted-foreground">群会话与世界书 · {group.chats.length} 个会话</summary>
    <FieldGroup className="mt-3 gap-3">
      <Field>
        <FieldLabel htmlFor="group-session">当前会话</FieldLabel>
        <Select value={group.chat_id} disabled={disabled} onValueChange={(chat_id) => void operate('groups.open_chat', { chat_id })}>
          <SelectTrigger id="group-session"><SelectValue /></SelectTrigger>
          <SelectContent><SelectGroup>{group.chats.map((id) => <SelectItem key={id} value={id}>{id}</SelectItem>)}</SelectGroup></SelectContent>
        </Select>
      </Field>
      <div className="flex flex-wrap items-end gap-2">
        <Field className="min-w-0 flex-1">
          <FieldLabel htmlFor="group-session-name">会话名称</FieldLabel>
          <Input id="group-session-name" value={name} disabled={disabled} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Button variant="outline" size="sm" disabled={disabled || !name.trim() || name === group.chat_id} onClick={() => void operate('groups.rename_chat', { name: name.trim() })}>重命名</Button>
      </div>
      <Field>
        <FieldLabel htmlFor="group-world">本会话世界书</FieldLabel>
        <Select value={chatMetadata?.world_info || '__none__'} disabled={disabled} onValueChange={(world) => void operate('groups.set_world', { world: world === '__none__' ? null : world })}>
          <SelectTrigger id="group-world"><SelectValue /></SelectTrigger>
          <SelectContent><SelectGroup><SelectItem value="__none__">不关联</SelectItem>{worlds.map((world) => <SelectItem key={world} value={world}>{world}</SelectItem>)}</SelectGroup></SelectContent>
        </Select>
      </Field>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={disabled} onClick={() => void operate('groups.new_chat')}>新会话</Button>
        <Button variant="outline" size="sm" disabled={disabled} onClick={() => fileInput.current?.click()}>导入 JSONL</Button>
        <Button variant="outline" size="sm" disabled={disabled} onClick={() => void operate('groups.export_chat')}>导出 JSONL</Button>
        <Button variant="ghost" size="sm" disabled={disabled} onClick={() => {
          if (window.confirm('删除当前群会话？文件会备份到 backups 目录。')) void operate('groups.delete_chat');
        }}>删除会话</Button>
      </div>
      <input ref={fileInput} type="file" accept=".jsonl" className="hidden" aria-label="导入群会话" onChange={(event) => {
        const file = event.target.files?.[0]; event.target.value = '';
        if (file) void file.text().then((text) => operate('groups.import_chat', { text })).catch((e) => pushToast(e.message, 'error'));
      }} />
    </FieldGroup>
  </details>;
}
