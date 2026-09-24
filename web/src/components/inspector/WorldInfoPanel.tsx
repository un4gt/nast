import { useEffect, useState } from 'react';
import { BookOpen, BookPlus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Field, FieldGroup, FieldLabel, FieldDescription, FieldSet, FieldLegend } from '@/components/ui/field';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { withCharacterBooks, worldInfoView } from '@/lib/world-settings';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';

const NONE = '__none__';

export function WorldInfoPanel({ onOpenEditor }: { onOpenEditor: () => void }) {
  const avatar = useStore((s) => s.activeAvatar);
  return <BookBindings key={avatar ?? 'none'} avatar={avatar} onOpenEditor={onOpenEditor} />;
}

function BookBindings({ avatar, onOpenEditor }: { avatar: string | null; onOpenEditor: () => void }) {
  const { activeChatName, chatMetadata, reloadChat, settings, saveSettings } = useStore();
  const [worlds, setWorlds] = useState<string[]>([]);
  const [character, setCharacter] = useState<any>(null);
  const [bookName, setBookName] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [revision, setRevision] = useState(0);
  const embedded = character?.data?.character_book;
  const primary = character?.data?.extensions?.world ?? '';
  const avatarKey = avatar?.replace(/\.png$/, '') ?? '';
  const extraBooks: string[] = worldInfoView(settings).char_lore.find((item: any) => item.name === avatarKey)?.extraBooks ?? [];

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    Promise.all([
      rpc.call<string[]>('worlds.list', {}),
      avatar ? rpc.call<any>('characters.get', { avatar }) : Promise.resolve(null),
    ]).then(([books, card]) => {
      if (cancelled) return;
      setWorlds(books); setCharacter(card);
      setBookName((previous) => previous || card?.data?.character_book?.name || (card ? card.name + ' 的角色书' : ''));
    }).catch((e) => { if (!cancelled) setError(String(e.message ?? e)); })
      .finally(() => { if (!cancelled) setLoaded(true); });
    return () => { cancelled = true; };
  }, [avatar, revision]);

  const perform = async (action: () => Promise<void>, message: string) => {
    if (busy) return;
    setBusy(true); setError('');
    try { await action(); pushToast(message, 'success'); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  };

  const bindPrimary = (name: string) => perform(async () => {
    await rpc.call('characters.edit', { avatar, data: { extensions: { world: name === NONE ? '' : name } } });
    setRevision((v) => v + 1);
  }, '角色主书已更新');

  const importEmbedded = () => perform(async () => {
    await rpc.call('characters.import_book', { avatar, name: bookName.trim() });
    setRevision((v) => v + 1);
  }, '卡内书已导入并设为角色主书');

  const bindChat = (name: string) => perform(async () => {
    await rpc.call('chats.set_world', { avatar, file_name: activeChatName, world: name === NONE ? null : name });
    await reloadChat();
  }, '聊天世界书已更新');

  const toggleExtra = (name: string, checked: boolean) => perform(async () => {
    if (!settings) return;
    const next = checked ? [...new Set([...extraBooks, name])] : extraBooks.filter((item) => item !== name);
    await saveSettings(withCharacterBooks(settings, avatarKey, next));
  }, '角色辅助书已更新');

  const options = (selected: string) => [...new Set([...worlds, ...(selected ? [selected] : [])])].map((name) =>
    <SelectItem key={name} value={name}>{name}{!worlds.includes(name) ? '（文件缺失）' : ''}</SelectItem>);

  return (
    <FieldGroup aria-busy={busy || !loaded}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">世界设定</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">让角色在需要时想起设定。按角色、聊天分别关联，完整条目在管理器中编辑。</p>
        </div>
        <Button variant="ghost" size="icon" aria-label="刷新书籍列表" disabled={busy} onClick={() => setRevision((v) => v + 1)}><BookOpen /></Button>
      </div>
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      {!loaded && <p role="status" className="text-xs text-muted-foreground">正在读取书籍…</p>}
      {avatar && loaded && (
        <>
          <FieldSet disabled={busy}>
            <FieldLegend>卡内携带的书</FieldLegend>
            {embedded ? (
              <>
                <div className="flex min-w-0 items-center gap-2">
                  <span className="min-w-0 flex-1 truncate" title={embedded.name}>{embedded.name || '未命名角色书'}</span>
                  <Badge variant="secondary">{embedded.entries?.length ?? 0} 条</Badge>
                </div>
                <FieldDescription>导入到世界书库并关联后参与生成。导出角色卡时会携带当前主书内容。</FieldDescription>
                <Field>
                  <FieldLabel htmlFor="embedded-book-name">导入为</FieldLabel>
                  <Input id="embedded-book-name" value={bookName} onChange={(event) => setBookName(event.target.value)} />
                  {worlds.includes(bookName.trim()) && <FieldDescription>已有同名书，请更换名称，现有内容会保留。</FieldDescription>}
                </Field>
                <Button variant="outline" size="sm" disabled={!bookName.trim() || worlds.includes(bookName.trim())} onClick={() => void importEmbedded()}>
                  <BookPlus data-icon="inline-start" />导入并关联
                </Button>
              </>
            ) : <FieldDescription>此角色卡未携带书籍。可以在下方关联已有世界书。</FieldDescription>}
          </FieldSet>
          <Field data-disabled={busy}>
            <FieldLabel htmlFor="character-primary-book">角色主书</FieldLabel>
            <Select value={primary || NONE} onValueChange={(value) => void bindPrimary(value)} disabled={busy}>
              <SelectTrigger id="character-primary-book"><SelectValue /></SelectTrigger>
              <SelectContent><SelectGroup><SelectItem value={NONE}>不关联</SelectItem>{options(primary)}</SelectGroup></SelectContent>
            </Select>
            <FieldDescription>随角色生效；群聊中仅当前发言角色的主书参与扫描。取消关联也会移除卡内副本。</FieldDescription>
          </Field>
          <FieldSet disabled={busy}>
            <FieldLegend>角色辅助书</FieldLegend>
            <FieldDescription>补充主书，可同时选择多本。</FieldDescription>
            <div className="flex max-h-48 flex-col gap-3 overflow-y-auto">
              {worlds.filter((name) => name !== primary).map((name, index) => (
                <Field key={name} orientation="horizontal">
                  <FieldLabel htmlFor={'extra-book-' + index} className="min-w-0 flex-1 truncate" title={name}>{name}</FieldLabel>
                  <Checkbox id={'extra-book-' + index} checked={extraBooks.includes(name)} onCheckedChange={(value) => void toggleExtra(name, value === true)} />
                </Field>
              ))}
              {worlds.filter((name) => name !== primary).length === 0 && <FieldDescription>暂无其他书籍，可在世界书管理器中新建或导入。</FieldDescription>}
            </div>
          </FieldSet>
        </>
      )}
      <Field data-disabled={busy || !avatar || !activeChatName}>
        <FieldLabel htmlFor="chat-world-book">本聊天绑定</FieldLabel>
        <Select value={chatMetadata?.world_info || chatMetadata?.world || NONE} onValueChange={(value) => void bindChat(value)} disabled={busy || !avatar || !activeChatName}>
          <SelectTrigger id="chat-world-book"><SelectValue placeholder="先打开聊天" /></SelectTrigger>
          <SelectContent><SelectGroup><SelectItem value={NONE}>不关联</SelectItem>{options(chatMetadata?.world_info ?? chatMetadata?.world ?? '')}</SelectGroup></SelectContent>
        </Select>
        <FieldDescription>只影响当前聊天。所有聊天共用的书籍在设置中启用。</FieldDescription>
      </Field>
      <Button variant="outline" size="sm" onClick={onOpenEditor}>打开世界书管理</Button>
    </FieldGroup>
  );
}
