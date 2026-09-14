import { SliderField, SwitchField } from '../fields';

export function WorldInfoGlobalPanel({
  wi,
  patchWi,
}: {
  wi: any;
  patchWi: (k: string, v: unknown) => void;
}) {
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">世界书全局设置</h3>
        <p className="text-xs text-muted-foreground">
          扫描与预算参数（world_info.*，与 ST 世界书面板全局区一致）。
        </p>
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
