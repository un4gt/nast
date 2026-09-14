import { useEffect, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { SliderField, SwitchField } from './components/settings/fields';
import { useStore } from './store';
import { pushToast } from './toasts';

const SOURCES = [
  { value: 'custom', label: '自定义（OpenAI 兼容）' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'claude', label: 'Anthropic Claude' },
  { value: 'makersuite', label: 'Google Gemini' },
];

export function SettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const { settings, saveSettings } = useStore();
  const [draft, setDraft] = useState<any>({});
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open && settings) {
      setDraft(structuredClone(settings));
    }
  }, [open, settings]);

  const oai = draft.oai_settings ?? {};
  const setOai = (patch: Record<string, unknown>) =>
    setDraft((d: any) => ({ ...d, oai_settings: { ...d.oai_settings, ...patch } }));

  const save = async () => {
    setSaving(true);
    try {
      await saveSettings(draft);
      pushToast('设置已保存', 'success');
      onOpenChange(false);
    } catch (e) {
      pushToast((e as Error).message, 'error');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>连接与生成参数（保存到 settings.json，与 ST 字段同构）</DialogDescription>
        </DialogHeader>

        <Tabs defaultValue="connection">
          <TabsList className="w-full">
            <TabsTrigger value="connection" className="flex-1">
              连接
            </TabsTrigger>
            <TabsTrigger value="generation" className="flex-1">
              生成
            </TabsTrigger>
            <TabsTrigger value="advanced" className="flex-1">
              高级
            </TabsTrigger>
          </TabsList>

          <TabsContent value="connection" className="flex flex-col gap-4 pt-2">
            <div className="flex flex-col gap-1.5">
              <Label>API 类型</Label>
              <Select
                value={oai.chat_completion_source ?? 'custom'}
                onValueChange={(v) => setOai({ chat_completion_source: v })}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SOURCES.map((s) => (
                    <SelectItem key={s.value} value={s.value}>
                      {s.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>模型</Label>
              <Input
                value={oai.openai_model ?? ''}
                onChange={(e) => setOai({ openai_model: e.target.value })}
                placeholder="gpt-4o / claude-3-5-sonnet / gemini-1.5-pro"
              />
            </div>
            {oai.chat_completion_source === 'custom' && (
              <div className="flex flex-col gap-1.5">
                <Label>OpenAI 兼容 baseURL</Label>
                <Input
                  value={(draft.nast_base_url as string) ?? ''}
                  onChange={(e) => setDraft((d: any) => ({ ...d, nast_base_url: e.target.value }))}
                  placeholder="http://127.0.0.1:5001/v1"
                />
                <p className="text-xs text-muted-foreground">
                  需以 NAST_OPENAI_BASE 环境变量传给服务端；此字段仅作备忘
                </p>
              </div>
            )}
          </TabsContent>

          <TabsContent value="generation" className="flex flex-col gap-5 pt-2">
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
            <SliderField
              label="Frequency Penalty"
              value={oai.frequency_penalty ?? 0}
              min={-2}
              max={2}
              step={0.05}
              onChange={(v) => setOai({ frequency_penalty: v })}
            />
            <SliderField
              label="Presence Penalty"
              value={oai.presence_penalty ?? 0}
              min={-2}
              max={2}
              step={0.05}
              onChange={(v) => setOai({ presence_penalty: v })}
            />
          </TabsContent>

          <TabsContent value="advanced" className="flex flex-col gap-4 pt-2">
            <SwitchField
              label="流式输出"
              checked={oai.stream_openai ?? true}
              onChange={(v) => setOai({ stream_openai: v })}
              description="关闭后整段返回"
            />
            <SwitchField
              label="合并连续 system 消息"
              checked={oai.squash_system_messages ?? false}
              onChange={(v) => setOai({ squash_system_messages: v })}
              description="Squash system messages（ST 同名选项）"
            />
            <SwitchField
              label="续写使用 Claude Prefill"
              checked={oai.continue_prefill ?? false}
              onChange={(v) => setOai({ continue_prefill: v })}
              description="仅 Anthropic 源有效"
            />
            <div className="flex flex-col gap-1.5">
              <Label>空输入时发送（send_if_empty）</Label>
              <Input
                value={oai.send_if_empty ?? ''}
                onChange={(e) => setOai({ send_if_empty: e.target.value })}
                placeholder="留空则不发送"
              />
            </div>
          </TabsContent>
        </Tabs>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={save} disabled={saving}>
            保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}


