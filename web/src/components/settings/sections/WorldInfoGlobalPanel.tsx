import { useEffect, useState } from 'react';
import { Globe } from 'lucide-react';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { SliderField, SwitchField } from '../fields';
import { rpc } from '@/rpc';

const STRATEGIES = [
  { value: '1', label: '角色优先（character first）' },
  { value: '2', label: '全局优先（global first）' },
  { value: '0', label: '均匀混排（evenly）' },
];

export function WorldInfoGlobalPanel({
  wi,
  patchWi,
}: {
  wi: any;
  patchWi: (k: string, v: unknown) => void;
}) {
  const [worlds, setWorlds] = useState<string[]>([]);
  const globalSelect: string[] = Array.isArray(wi.global_select) ? wi.global_select : [];

  useEffect(() => {
    rpc.call<string[]>('worlds.list', {}).then(setWorlds).catch(() => {});
  }, []);

  const toggleWorld = (name: string, on: boolean) => {
    const next = on
      ? [...globalSelect, name]
      : globalSelect.filter((w) => w !== name);
    patchWi('global_select', next);
  };

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">世界书全局设置</h3>
        <p className="text-xs text-muted-foreground">
          全局激活书（globalSelect）与扫描/预算参数（world_info.*，与 ST 世界书面板一致）。
        </p>
      </div>

      <div className="flex flex-col gap-2">
        <Label className="flex items-center gap-1.5">
          <Globe className="size-3.5" />
          全局激活（所有聊天生效）
        </Label>
        {worlds.length === 0 && (
          <p className="text-xs text-muted-foreground">暂无世界书；在 Inspector → World Info 打开管理器创建。</p>
        )}
        <ScrollArea className="max-h-44 rounded-md border p-2">
          <div className="flex flex-col gap-1">
            {worlds.map((w) => (
              <label key={w} className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent/50">
                <Checkbox
                  checked={globalSelect.includes(w)}
                  onCheckedChange={(v) => toggleWorld(w, v === true)}
                />
                <span className="truncate">{w}</span>
              </label>
            ))}
          </div>
        </ScrollArea>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>插入策略</Label>
        <Select
          value={String(wi.world_info_character_strategy ?? 1)}
          onValueChange={(v) => patchWi('world_info_character_strategy', Number(v))}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {STRATEGIES.map((s) => (
              <SelectItem key={s.value} value={s.value}>{s.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <SliderField
        label="扫描深度（条消息）"
        value={wi.world_info_depth ?? 2}
        min={0}
        max={100}
        step={1}
        onChange={(v) => patchWi('world_info_depth', v)}
      />
      <SliderField
        label="预算（上下文 %）"
        value={wi.world_info_budget ?? 25}
        min={1}
        max={100}
        step={1}
        onChange={(v) => patchWi('world_info_budget', v)}
      />
      <SliderField
        label="预算硬上限（token，0=关闭）"
        value={wi.world_info_budget_cap ?? 0}
        min={0}
        max={4096}
        step={128}
        onChange={(v) => patchWi('world_info_budget_cap', v)}
      />
      <SliderField
        label="最少激活条数"
        value={wi.world_info_min_activations ?? 0}
        min={0}
        max={100}
        step={1}
        onChange={(v) => patchWi('world_info_min_activations', v)}
      />
      <SliderField
        label="最少激活的深度上限（0=不限）"
        value={wi.world_info_min_activations_depth_max ?? 0}
        min={0}
        max={100}
        step={1}
        onChange={(v) => patchWi('world_info_min_activations_depth_max', v)}
      />
      <SliderField
        label="最大递归步数（0=不限）"
        value={Number(wi.world_info_max_recursion_steps) || 0}
        min={0}
        max={10}
        step={1}
        onChange={(v) => patchWi('world_info_max_recursion_steps', v)}
      />

      <SwitchField
        label="递归扫描"
        checked={wi.world_info_recursive ?? false}
        onChange={(v) => patchWi('world_info_recursive', v)}
        description="条目内容可作为新扫描源"
      />
      <SwitchField
        label="大小写敏感"
        checked={wi.world_info_case_sensitive ?? false}
        onChange={(v) => patchWi('world_info_case_sensitive', v)}
      />
      <SwitchField
        label="全词匹配"
        checked={wi.world_info_match_whole_words ?? false}
        onChange={(v) => patchWi('world_info_match_whole_words', v)}
      />
      <SwitchField
        label="插入组评分"
        checked={wi.world_info_use_group_scoring ?? false}
        onChange={(v) => patchWi('world_info_use_group_scoring', v)}
      />
    </div>
  );
}
