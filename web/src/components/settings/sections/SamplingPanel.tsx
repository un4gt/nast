import { SliderField, SwitchField } from '../fields';

export function SamplingPanel({
  oai,
  patchOai,
}: {
  oai: any;
  patchOai: (k: string, v: unknown) => void;
}) {
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">采样参数</h3>
        <p className="text-xs text-muted-foreground">对应 ST 采样区（Temperature / 采样器组）。</p>
      </div>

      <SliderField
        label="Temperature"
        value={oai.temperature ?? 1.0}
        min={0}
        max={2}
        step={0.05}
        onChange={(v) => patchOai('temperature', v)}
      />
      <SliderField
        label="Top P"
        value={oai.top_p ?? 1.0}
        min={0}
        max={1}
        step={0.01}
        onChange={(v) => patchOai('top_p', v)}
      />
      <SliderField
        label="Frequency Penalty"
        value={oai.frequency_penalty ?? 0}
        min={-2}
        max={2}
        step={0.05}
        onChange={(v) => patchOai('frequency_penalty', v)}
      />
      <SliderField
        label="Presence Penalty"
        value={oai.presence_penalty ?? 0}
        min={-2}
        max={2}
        step={0.05}
        onChange={(v) => patchOai('presence_penalty', v)}
      />
      <SliderField
        label="回复长度上限（max_tokens）"
        value={oai.openai_max_tokens ?? 300}
        min={16}
        max={8192}
        step={16}
        onChange={(v) => patchOai('openai_max_tokens', v)}
      />
      <SliderField
        label="上下文大小（context）"
        value={oai.openai_max_context ?? 4095}
        min={512}
        max={200000}
        step={512}
        onChange={(v) => patchOai('openai_max_context', v)}
      />
      <SwitchField
        label="解除上下文上限校验"
        checked={oai.max_context_unlocked ?? false}
        onChange={(v) => patchOai('max_context_unlocked', v)}
        description="允许 context 超过模型标称值（max_context_unlocked）"
      />
    </div>
  );
}
