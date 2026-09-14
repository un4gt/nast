import { useStore } from '../../store';
import { SliderField, SwitchField } from '../settings/fields';
import { pushToast } from '../../toasts';

export function GenerationPanel() {
  const { settings, saveSettings } = useStore();
  const oai = settings?.oai_settings ?? {};

  const setOai = (patch: Record<string, unknown>) => {
    if (!settings) return;
    const next = { ...settings, oai_settings: { ...oai, ...patch } };
    saveSettings(next).catch((e) => pushToast(e instanceof Error ? e.message : String(e), 'error'));
  };

  if (!settings) return <p className="text-xs text-muted-foreground">设置加载中…</p>;

  return (
    <div className="flex flex-col gap-4">
      <SliderField
        label="上下文大小"
        value={oai.openai_max_context ?? 4095}
        min={512}
        max={200000}
        step={512}
        onChange={(v) => setOai({ openai_max_context: v })}
      />
      <SliderField
        label="回复长度上限"
        value={oai.openai_max_tokens ?? 300}
        min={16}
        max={8192}
        step={16}
        onChange={(v) => setOai({ openai_max_tokens: v })}
      />
      <SliderField
        label="Temperature"
        value={oai.temperature ?? 1.0}
        min={0}
        max={2}
        step={0.05}
        onChange={(v) => setOai({ temperature: v })}
      />
      <SliderField
        label="Top P"
        value={oai.top_p ?? 1.0}
        min={0}
        max={1}
        step={0.01}
        onChange={(v) => setOai({ top_p: v })}
      />
      <SwitchField
        label="流式输出"
        checked={oai.stream_openai ?? true}
        onChange={(v) => setOai({ stream_openai: v })}
      />
      <p className="text-[10px] text-muted-foreground">改动即时保存到 settings.json</p>
    </div>
  );
}
