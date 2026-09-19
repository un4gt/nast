import { useEffect, useState } from 'react';
import { Trash2, UserPlus, X } from 'lucide-react';
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from '@/components/ui/dialog';
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';
import { rpc } from '../../rpc';

const STRATEGIES = [
  { value: '0', label: '自然（提及 + 健谈度）' },
  { value: '1', label: '列表顺序（全部成员）' },
  { value: '2', label: '手动（随机一名）' },
  { value: '3', label: '轮流（未发言者优先）' },
];

const MODES = [
  { value: '0', label: 'SWAP（当前成员卡片）' },
  { value: '1', label: 'APPEND（合并全部成员卡片）' },
  { value: '2', label: 'APPEND_DISABLED（合并含静音成员）' },
];

export function GroupSettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const { groups, activeGroupId, characters, saveGroup, deleteGroup, loadAll } = useStore();
  const group = groups.find((g) => g.id === activeGroupId);
  const [draft, setDraft] = useState<any>(null);
  const [talkativeness, setTalkativeness] = useState<Record<string, number>>({});
  const [addOpen, setAddOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open && group) setDraft({ ...group });
    else setDraft(null);
  }, [open, group]);

  useEffect(() => {
    if (!open || !group) return;
    // 读成员 talkativeness
    Promise.all(
      group.members.map((a) => rpc.call<any>('characters.get', { avatar: a }).catch(() => null)),
    ).then((cards) => {
      const t: Record<string, number> = {};
      cards.forEach((c, i) => {
        if (c) t[group.members[i]] = Number(c.talkativeness ?? 0.5);
      });
      setTalkativeness(t);
    });
  }, [open, group]);

  if (!group || !draft) return null;

  const members = group.members
    .map((a) => characters.find((c) => c.avatar === a))
    .filter(Boolean);
  const nonMembers = characters.filter((c) => !group.members.includes(c.avatar));

  const save = async () => {
    setBusy(true);
    try {
      await saveGroup(draft);
      // 持久化 talkativeness 到成员卡
      for (const [avatar, t] of Object.entries(talkativeness)) {
        await rpc
          .call('characters.edit', { avatar, data: { talkativeness: String(t) } })
          .catch(() => {});
      }
      pushToast('群设置已保存', 'success');
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  const toggleMute = (avatar: string, muted: boolean) => {
    const next = muted
      ? [...draft.disabled_members, avatar]
      : draft.disabled_members.filter((a: string) => a !== avatar);
    setDraft({ ...draft, disabled_members: next });
  };

  const removeMember = (avatar: string) => {
    setDraft({
      ...draft,
      members: draft.members.filter((a: string) => a !== avatar),
      disabled_members: draft.disabled_members.filter((a: string) => a !== avatar),
    });
  };

  const addMember = (avatar: string) => {
    setDraft({ ...draft, members: [...draft.members, avatar] });
    setAddOpen(false);
  };

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>群设置</DialogTitle>
            <DialogDescription>{group.name}（{group.members.length} 名成员）</DialogDescription>
          </DialogHeader>

          <div className="flex flex-col gap-4">
            <div className="flex flex-col gap-1.5">
              <Label>群名称</Label>
              <Input
                value={draft.name ?? ''}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <Label>激活策略</Label>
              <Select
                value={String(draft.activation_strategy ?? 0)}
                onValueChange={(v) => setDraft({ ...draft, activation_strategy: Number(v) })}
              >
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {STRATEGIES.map((s) => (
                    <SelectItem key={s.value} value={s.value}>{s.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="flex flex-col gap-1.5">
              <Label>卡片合并模式</Label>
              <Select
                value={String(draft.generation_mode ?? 0)}
                onValueChange={(v) => setDraft({ ...draft, generation_mode: Number(v) })}
              >
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {MODES.map((s) => (
                    <SelectItem key={s.value} value={s.value}>{s.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="flex items-center justify-between gap-4">
              <div className="flex flex-col">
                <Label>允许连续自回复</Label>
                <p className="text-xs text-muted-foreground">同一成员可连续发言</p>
              </div>
              <Switch
                checked={draft.allow_self_responses ?? false}
                onCheckedChange={(v) => setDraft({ ...draft, allow_self_responses: v })}
              />
            </div>

            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <Label>成员（静音 / 健谈度 / 移除）</Label>
                <Button size="sm" variant="secondary" onClick={() => setAddOpen(true)}>
                  <UserPlus className="size-3.5" />
                  添加
                </Button>
              </div>
              <div className="flex max-h-56 flex-col gap-2 overflow-y-auto rounded-md border p-2">
                {members.map((m) => {
                  const muted = (draft.disabled_members ?? []).includes(m!.avatar);
                  const t = talkativeness[m!.avatar] ?? 0.5;
                  return (
                    <div key={m!.avatar} className="flex items-center gap-2">
                      <Avatar className="size-7">
                        <AvatarImage src={m!.avatarUrl} alt={m!.name} className="size-full object-cover" />
                        <AvatarFallback className="text-[10px]">{m!.name.slice(0, 1)}</AvatarFallback>
                      </Avatar>
                      <span className="w-16 shrink-0 truncate text-xs">{m!.name}</span>
                      <Checkbox
                        checked={!muted}
                        onCheckedChange={(v) => toggleMute(m!.avatar, v !== true)}
                        title="启用（取消勾选 = 静音）"
                      />
                      <Slider
                        className="flex-1"
                        value={[t]}
                        min={0}
                        max={1}
                        step={0.05}
                        onValueChange={([v]) =>
                          setTalkativeness((prev) => ({ ...prev, [m!.avatar]: v }))
                        }
                      />
                      <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-muted-foreground">
                        {t.toFixed(2)}
                      </span>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-6 text-muted-foreground hover:text-destructive"
                        onClick={() => removeMember(m!.avatar)}
                        title="移出群"
                      >
                        <X className="size-3" />
                      </Button>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          <DialogFooter>
            <Button
              variant="ghost"
              className="mr-auto text-destructive hover:text-destructive"
              onClick={() => setDeleteOpen(true)}
            >
              <Trash2 className="size-3.5" />
              删除群
            </Button>
            <Button onClick={() => void save()} disabled={busy}>保存</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={addOpen} onOpenChange={setAddOpen}>
        <DialogContent className="max-w-xs">
          <DialogHeader>
            <DialogTitle>添加成员</DialogTitle>
          </DialogHeader>
          <div className="flex max-h-64 flex-col gap-1 overflow-y-auto">
            {nonMembers.map((c) => (
              <Button key={c.avatar} variant="ghost" className="justify-start" onClick={() => addMember(c.avatar)}>
                <Avatar className="size-6">
                  <AvatarImage src={c.avatarUrl} alt={c.name} className="size-full object-cover" />
                  <AvatarFallback className="text-[10px]">{c.name.slice(0, 1)}</AvatarFallback>
                </Avatar>
                <span className="truncate text-sm">{c.name}</span>
              </Button>
            ))}
            {nonMembers.length === 0 && (
              <p className="text-xs text-muted-foreground">没有可添加的角色</p>
            )}
          </div>
        </DialogContent>
      </Dialog>

      <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除群组？</AlertDialogTitle>
            <AlertDialogDescription>
              「{group.name}」及其群聊记录将被删除（成员角色卡不受影响）。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => {
                setDeleteOpen(false);
                onOpenChange(false);
                void deleteGroup(group.id).then(loadAll);
              }}
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
