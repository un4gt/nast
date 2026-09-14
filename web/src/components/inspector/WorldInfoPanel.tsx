import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { Badge } from '@/components/ui/badge';
import { rpc } from '../../rpc';
import { useStore } from '../../store';

export function WorldInfoPanel({ onOpenEditor }: { onOpenEditor: () => void }) {
  const { characters, activeAvatar } = useStore();
  const [boundWorld, setBoundWorld] = useState<string | null>(null);
  const [entries, setEntries] = useState<{ uid: number; comment: string; keys: string[]; disable: boolean }[]>([]);
  const [worlds, setWorlds] = useState<string[]>([]);
  const ch = characters.find((c) => c.avatar === activeAvatar);

  useEffect(() => {
    rpc.call<string[]>('worlds.list', {}).then(setWorlds).catch(() => {});
  }, []);

  useEffect(() => {
    setBoundWorld(null);
    setEntries([]);
    if (!activeAvatar) return;
    rpc
      .call<any>('characters.get', { avatar: activeAvatar })
      .then((c) => {
        const w = c.data?.extensions?.world;
        if (w) {
          setBoundWorld(w);
          return rpc.call<any>('worlds.get', { name: w });
        }
        return undefined;
      })
      .then((book) => {
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
      })
      .catch(() => {});
  }, [activeAvatar]);

  void ch;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1.5">
        <Label className="text-xs text-muted-foreground">绑定世界书</Label>
        <Select value={boundWorld ?? ''} onValueChange={(v) => setBoundWorld(v)}>
          <SelectTrigger className="h-8">
            <SelectValue placeholder="未绑定" />
          </SelectTrigger>
          <SelectContent>
            {worlds.map((w) => (
              <SelectItem key={w} value={w}>
                {w}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {entries.length > 0 && (
        <div className="flex flex-col gap-1">
          <Label className="text-xs text-muted-foreground">
            条目（{entries.filter((e) => !e.disable).length}/{entries.length} 启用）
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
