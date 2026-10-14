import { useEffect, useId, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Separator } from '@/components/ui/separator';
import { GroupSettingsDialog } from '../chat/GroupSettingsDialog';
import { CharacterPanel } from './CharacterPanel';
import { rpc } from '@/rpc';
import { useStore } from '@/store';
import { pushToast } from '@/toasts';

export function GroupPanel() {
  const fieldId = useId();
  const { groups, activeGroupId, characters, settings, chatMetadata, generating, connected, saveGroup, setChatPersona, reloadChat } = useStore();
  const group = groups.find(g => g.id === activeGroupId);
  const [name, setName] = useState('');
  const [avatar, setAvatar] = useState('');
  const [tags, setTags] = useState('');
  const [scenario, setScenario] = useState('');
  const [examples, setExamples] = useState('');
  const [member, setMember] = useState('');
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setName(group?.name ?? ''); setAvatar(String(group?.avatar_url ?? '')); setTags(Array.isArray(group?.tags) ? group.tags.join(', ') : ''); }, [group]);
  useEffect(() => { setScenario(String(chatMetadata?.scenario ?? '')); setExamples(String(chatMetadata?.mes_example ?? '')); }, [group?.chat_id, chatMetadata]);
  useEffect(() => { if (!group?.members.includes(member)) setMember(''); }, [group?.members, member]);
  if (!group) return null;
  const disabled = busy || generating || !connected;
  const personas = ((settings?.power_user as any)?.personas ?? {}) as Record<string, string>;
  const save = async () => {
    setBusy(true);
    try {
      await saveGroup({ ...group, name: name.trim() || group.name, avatar_url: avatar, tags: tags.split(',').map(t => t.trim()).filter(Boolean) });
      await rpc.call('groups.set_metadata', { id: group.id, chat_id: group.chat_id, metadata: { scenario, mes_example: examples } });
      await reloadChat(); pushToast('群详情已保存', 'success');
    } finally { setBusy(false); }
  };
  return <div className="flex flex-col gap-4">
    <FieldGroup>
      <Field><FieldLabel htmlFor={fieldId + "-group-detail-name"}>群名称</FieldLabel><Input id={fieldId + "-group-detail-name"} value={name} disabled={disabled} onChange={e => setName(e.target.value)} /></Field>
      <Field><FieldLabel htmlFor={fieldId + "-group-avatar"}>群头像 URL</FieldLabel><Input id={fieldId + "-group-avatar"} value={avatar} disabled={disabled} onChange={e => setAvatar(e.target.value)} /></Field>
      <Field><FieldLabel htmlFor={fieldId + "-group-tags"}>群标签（逗号分隔）</FieldLabel><Input id={fieldId + "-group-tags"} value={tags} disabled={disabled} onChange={e => setTags(e.target.value)} /></Field>
      <Field orientation="horizontal"><FieldLabel htmlFor={fieldId + "-group-favorite"}>收藏群组</FieldLabel><Switch id={fieldId + "-group-favorite"} checked={group.fav} disabled={disabled} onCheckedChange={fav => void saveGroup({ ...group, fav })} /></Field>
      <Field><FieldLabel htmlFor={fieldId + "-group-persona"}>本会话 Persona</FieldLabel><Select disabled={disabled} value={chatMetadata?.persona || '__default__'} onValueChange={v => void setChatPersona(v === '__default__' ? null : v)}>
        <SelectTrigger id={fieldId + "-group-persona"}><SelectValue /></SelectTrigger><SelectContent><SelectGroup><SelectItem value="__default__">使用默认 Persona</SelectItem>{Object.entries(personas).map(([id, label]) => <SelectItem key={id} value={id}>{label}</SelectItem>)}</SelectGroup></SelectContent>
      </Select></Field>
      <Field><FieldLabel htmlFor={fieldId + "-group-scenario"}>本会话场景覆盖</FieldLabel><Textarea id={fieldId + "-group-scenario"} value={scenario} disabled={disabled} onChange={e => setScenario(e.target.value)} /></Field>
      <Field><FieldLabel htmlFor={fieldId + "-group-examples"}>本会话示例覆盖</FieldLabel><Textarea id={fieldId + "-group-examples"} value={examples} disabled={disabled} onChange={e => setExamples(e.target.value)} /></Field>
    </FieldGroup>
    <div className="flex flex-wrap gap-2"><Button disabled={disabled} onClick={() => void save()}>保存群详情</Button><Button variant="outline" disabled={disabled} onClick={() => setOpen(true)}>成员与发言设置</Button></div>
    <Separator />
    <Field><FieldLabel htmlFor={fieldId + "-group-card"}>查看与编辑成员卡</FieldLabel><Select value={member || '__none__'} onValueChange={v => setMember(v === '__none__' ? '' : v)}>
      <SelectTrigger id={fieldId + "-group-card"}><SelectValue /></SelectTrigger><SelectContent><SelectGroup><SelectItem value="__none__">选择成员</SelectItem>{group.members.map(a => <SelectItem key={a} value={a}>{characters.find(c => c.avatar === a)?.name ?? a}</SelectItem>)}</SelectGroup></SelectContent>
    </Select></Field>
    {member && <CharacterPanel avatar={member} onAvatarChange={setMember} />}
    <GroupSettingsDialog open={open} onOpenChange={setOpen} />
  </div>;
}
