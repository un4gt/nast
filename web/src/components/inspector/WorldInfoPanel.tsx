import { useEffect, useState } from 'react';
import { BookPlus, Link2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { rpc } from '../../rpc';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';

interface EntrySummary {
  uid: number;
  comment: string;
  keys: string[];
  disable: boolean;
}

const NONE = '__none__';

export function WorldInfoPanel({ onOpenEditor }: { onOpenEditor: () => void }) {
  const { activeAvatar, activeChatName, chatMetadata, reloadChat, settings, saveSettings } = useStore();
  const [worlds, setWorlds] = useState<string[]>([]);
  const [extraBooks, setExtraBooks] = useState<string[]>([]);
  const [embedded, setEmbedded] = useState<string | null>(null);
  const [entries, setEntries] = useState<EntrySummary[]>([]);

  useEffect(() => {
    rpc.call<string[]>('worlds.list', {}).then(setWorlds).catch(() => {});
  }, []);

  // 角色内嵌书（data.extensions.world）+ charLore 辅助书（settings.world_info.char_lore）
  const avatarKey = activeAvatar?.replace(/\.png$/, '') ?? '';
  useEffect(() => {
    setExtraBooks([]);
    setEmbedded(null);
    setEntries([]);
    if (!activeAvatar) return;
    rpc
      .call<any>('characters.get', { avatar: activeAvatar })
      .then((c) => {
        const w = c.data?.extensions?.world ?? null;
        setEmbedded(w);
        const names = w ? [w] : [];
        if (w) {
          return rpc.call<any>('worlds.get', { name: w }).then((book) => ({ names, book }));
        }
        return { names, book: undefined };
      })
      .then(({ names, book }) => {
        if (book?.entries) {
          setEntries(
            Object.values(book.entries).map((e: any) => ({
              uid: e.uid,
              comment: e.comment || '(untitled)',
              keys: e.key ?? [],
              disable: e.disable ?? false,
            })),
          );
        }
        void names;
      })
      .catch(() => {});
  }, [activeAvatar]);

  // charLore 从 settings 读取
  useEffect(() => {
    const charLore: any[] = Array.isArray((settings as any)?.world_info?.char_lore)
      ? (settings as any).world_info.char_lore
      : [];
    const mine = charLore.find((c) => c?.name === avatarKey);
    setExtraBooks(Array.isArray(mine?.extraBooks) ? mine.extraBooks : []);
  }, [settings, avatarKey]);

  const setChatWorld = async (world: string | null) => {
    if (!activeAvatar || !activeChatName) return;
    try {
      await rpc.call('chats.set_world', {
        avatar: activeAvatar,
        file_name: activeChatName,
        world,
      });
      await reloadChat();
      pushToast(world ? '已绑定聊天世界书' : '已解绑聊天世界书', 'success');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    }
  };

  const toggleExtraBook = async (name: string, on: boolean) => {
    const next = on ? [...extraBooks, name] : extraBooks.filter((b) => b !== name);
    setExtraBooks(next);
    if (!settings) return;
    const wi = { ...((settings as any).world_info ?? {}) };
    const charLore: any[] = Array.isArray(wi.char_lore) ? [...wi.char_lore] : [];
    const idx = charLore.findIndex((c) => c?.name === avatarKey);
    if (next.length === 0) {
      if (idx >= 0) charLore.splice(idx, 1);
    } else if (idx >= 0) {
      charLore[idx] = { ...charLore[idx], extraBooks: next };
    } else {
      charLore.push({ name: avatarKey, extraBooks: next });
    }
    wi.char_lore = charLore;
    try {
      await saveSettings({ ...settings, world_info: wi } as any);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="mb-1 text-sm font-semibold">World Info</h3>
        <p className="text-[10px] text-muted-foreground">
          三个层级：本聊天绑定（chat_metadata.world）→ 角色辅助书（charLore）→ 角色内嵌书（卡内）。全局激活在设置 → 世界书全局。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="flex items-center gap-1 text-xs text-muted-foreground">
          <Link2 className="size-3" />
          本聊天绑定
        </Label>
        <Select
          value={chatMetadata?.world ?? NONE}
          onValueChange={(v) => void setChatWorld(v === NONE ? null : v)}
          disabled={!activeChatName}
        >
          <SelectTrigger className="h-8">
            <SelectValue placeholder={activeChatName ? '未绑定' : '未打开聊天'} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NONE}>未绑定</SelectItem>
            {worlds.map((w) => (
              <SelectItem key={w} value={w}>{w}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="flex items-center gap-1 text-xs text-muted-foreground">
          <BookPlus className="size-3" />
          角色辅助书（charLore，随角色生效）
        </Label>
        {worlds.length === 0 && (
          <p className="text-xs text-muted-foreground">暂无世界书可绑定。</p>
        )}
        <ScrollArea className="max-h-32 rounded-md border p-2">
          <div className="flex flex-col gap-1">
            {worlds
              .filter((w) => w !== embedded)
              .map((w) => (
                <label key={w} className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent/50">
                  <Checkbox
                    checked={extraBooks.includes(w)}
                    onCheckedChange={(v) => void toggleExtraBook(w, v === true)}
                  />
                  <span className="truncate">{w}</span>
                </label>
              ))}
          </div>
        </ScrollArea>
      </div>

      {embedded && (
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">
            内嵌书（卡内）：{embedded}
          </Label>
          <div className="flex flex-col gap-1">
            {entries.map((e) => (
              <div key={e.uid} className="flex items-center gap-2 rounded px-2 py-1 text-xs hover:bg-accent/50">
                <span className="flex-1 truncate">{e.comment}</span>
                {e.keys.slice(0, 2).map((k) => (
                  <Badge key={k} variant="secondary" className="px-1 text-[10px]">
                    {k}
                  </Badge>
                ))}
                {e.disable && <Badge variant="outline" className="px-1 text-[10px]">off</Badge>}
              </div>
            ))}
          </div>
        </div>
      )}

      <Button variant="outline" size="sm" onClick={onOpenEditor}>
        打开世界书管理
      </Button>
    </div>
  );
}
