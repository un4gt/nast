import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { SliderField } from '../fields';

const PERSONA_POSITIONS = [
  { value: '0', label: '在提示词中（personaDescription 标记位）' },
  { value: '1', label: "Author's Note 顶部" },
  { value: '2', label: "Author's Note 底部" },
  { value: '3', label: '按深度注入' },
];

export function PersonaPanel({
  draft,
  patch,
}: {
  draft: any;
  patch: (path: string, v: unknown) => void;
}) {
  const power = draft.power_user ?? {};
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">用户 / Persona</h3>
        <p className="text-xs text-muted-foreground">
          你的显示名与 persona 描述（power_user.username / persona_description）。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>显示名（username）</Label>
        <Input
          value={power.username ?? 'User'}
          onChange={(e) => patch('power_user.username', e.target.value)}
          placeholder="User"
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>Persona 描述</Label>
        <Textarea
          value={power.persona_description ?? ''}
          onChange={(e) => patch('power_user.persona_description', e.target.value)}
          placeholder="描述你是谁——将替换 prompt 中的 {{persona}}"
          rows={6}
          className="min-h-0 resize-y text-xs"
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>注入位置</Label>
        <Select
          value={String(power.persona_description_position ?? 0)}
          onValueChange={(v) => patch('power_user.persona_description_position', Number(v))}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PERSONA_POSITIONS.map((p) => (
              <SelectItem key={p.value} value={p.value}>
                {p.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {Number(power.persona_description_position ?? 0) === 3 && (
        <SliderField
          label="注入深度"
          value={power.persona_description_depth ?? 4}
          min={0}
          max={16}
          step={1}
          onChange={(v) => patch('power_user.persona_description_depth', v)}
        />
      )}
    </div>
  );
}
