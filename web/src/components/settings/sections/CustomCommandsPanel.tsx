import { Plus, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { ScrollArea } from '@/components/ui/scroll-area';

interface CustomCommand {
  name: string;
  text: string;
}

/**
 * 自定义命令（power_user.custom_commands）：`/名称` 展开为预设文本发送。
 * 网页端与 QQ 机器人共用同一份定义；文本支持 {{char}}/{{user}} 宏（服务端展开）。
 */
export function CustomCommandsPanel({
  draft,
  patch,
}: {
  draft: any;
  patch: (path: string, v: unknown) => void;
}) {
  const list: CustomCommand[] = Array.isArray(draft.power_user?.custom_commands)
    ? draft.power_user.custom_commands
    : [];

  const setList = (next: CustomCommand[]) => patch('power_user.custom_commands', next);

  const update = (i: number, p: Partial<CustomCommand>) =>
    setList(list.map((c, idx) => (idx === i ? { ...c, ...p } : c)));

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h3 className="mb-1 text-sm font-semibold">自定义命令</h3>
        <p className="text-xs text-muted-foreground">
          在聊天输入框输入 <code className="rounded bg-secondary px-1">/名称</code> 即展开为预设文本发送（等效 QuickReply）。
          文本支持 {'{{char}}'} / {'{{user}}'} 宏，QQ 机器人同样生效。随「保存」写入设置。
        </p>
      </div>

      <Button
        size="sm"
        variant="secondary"
        className="self-start"
        onClick={() => setList([...list, { name: '', text: '' }])}
      >
        <Plus className="size-3.5" />
        新增命令
      </Button>

      <ScrollArea className="max-h-[420px] rounded-md border p-2">
        <div className="flex flex-col gap-3">
          {list.map((c, i) => (
            <div key={i} className="flex flex-col gap-1.5 rounded-md border bg-card p-2.5">
              <div className="flex items-center gap-2">
                <Badge variant="outline" className="font-mono text-[10px]">/{c.name || '名称'}</Badge>
                <Input
                  value={c.name}
                  onChange={(e) => update(i, { name: e.target.value.trim().replace(/^\//, '') })}
                  placeholder="命令名（不含斜杠）"
                  className="h-7 max-w-40 text-xs"
                />
                <div className="flex-1" />
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-6 text-muted-foreground hover:text-destructive"
                  onClick={() => setList(list.filter((_, idx) => idx !== i))}
                  title="删除"
                >
                  <Trash2 className="size-3" />
                </Button>
              </div>
              <Textarea
                value={c.text}
                onChange={(e) => update(i, { text: e.target.value })}
                rows={4}
                className="min-h-0 resize-y text-xs"
                placeholder="展开发送的文本…（支持 {{char}}/{{user}} 宏）"
              />
            </div>
          ))}
          {list.length === 0 && (
            <p className="px-2 py-3 text-center text-xs text-muted-foreground">
              暂无自定义命令；点「新增命令」创建，例如 名称 <code>开场</code> → 一段长开场白。
            </p>
          )}
        </div>
      </ScrollArea>

      <Label className="text-[10px] text-muted-foreground">
        共 {list.length} 条；命令名避免与内置命令（/help、/char 等）重名。
      </Label>
    </div>
  );
}
