import { useMemo, useState } from 'react';
import { Check, Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { SliderField } from '../fields';
import { useStore } from '../../../store';
import { pushToast } from '../../../toasts';

/** persona_description_position（personas.js:88-97） */
const PERSONA_POSITIONS = [
  { value: '0', label: '在提示词中（personaDescription 标记位）' },
  { value: '2', label: "Author's Note 顶部" },
  { value: '3', label: "Author's Note 底部" },
  { value: '4', label: '按深度注入' },
  { value: '9', label: '不注入（仅名字）' },
];

const PERSONA_ROLES = [
  { value: '0', label: 'System' },
  { value: '1', label: 'User' },
  { value: '2', label: 'Assistant' },
];

function newId(): string {
  return `persona-${Date.now().toString(36)}-${Math.floor(Math.random() * 1e4).toString(36)}.png`;
}

export function PersonaPanel({
  draft,
  patch,
}: {
  draft: any;
  patch: (path: string, v: unknown) => void;
}) {
  const { activeChatName, chatMetadata, setChatPersona } = useStore();
  const power = draft.power_user ?? {};
  const personas: Record<string, string> = power.personas ?? {};
  const descriptions: Record<string, any> = power.persona_descriptions ?? {};
  const [selected, setSelected] = useState<string | null>(null);

  const ids = useMemo(() => Object.keys(personas).sort(), [personas]);
  const current = selected ?? power.default_persona ?? ids[0] ?? null;
  const desc = current ? (descriptions[current] ?? {}) : {};

  const selectPersona = (id: string | null) => {
    setSelected(id);
    if (id) patch('power_user.default_persona', id);
  };

  const addPersona = () => {
    const id = newId();
    patch(`power_user.personas.${id}`, '新 Persona');
    patch(`power_user.persona_descriptions.${id}`, {
      description: '', position: 0, depth: 2, role: 0,
    });
    setSelected(id);
    patch('power_user.default_persona', id);
  };

  const deletePersona = (id: string) => {
    const nextPersonas = { ...personas };
    delete nextPersonas[id];
    patch('power_user.personas', nextPersonas);
    const nextDescs = { ...descriptions };
    delete nextDescs[id];
    patch('power_user.persona_descriptions', nextDescs);
    if (power.default_persona === id) {
      const rest = Object.keys(nextPersonas);
      patch('power_user.default_persona', rest[0] ?? null);
    }
    if (selected === id) setSelected(null);
  };

  const renamePersona = (id: string, name: string) => {
    patch(`power_user.personas.${id}`, name);
  };

  const chatBound = chatMetadata?.persona ?? null;

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">用户 / Persona</h3>
        <p className="text-xs text-muted-foreground">
          多 persona 管理（power_user.personas / persona_descriptions），生成时按「聊天绑定 &gt; 默认」解析。
        </p>
      </div>

      {/* 每聊天绑定 */}
      {activeChatName && (
        <div className="flex flex-col gap-1.5 rounded-md border bg-muted/30 p-3">
          <Label className="text-xs text-muted-foreground">当前聊天绑定</Label>
          <div className="flex items-center gap-2">
            <Select
              value={chatBound ?? '__default__'}
              onValueChange={(v) =>
                void setChatPersona(v === '__default__' ? null : v)
              }
            >
              <SelectTrigger className="flex-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__default__">跟随默认 persona</SelectItem>
                {ids.map((id) => (
                  <SelectItem key={id} value={id}>{personas[id]}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <p className="text-[10px] text-muted-foreground">
            绑定写入 chat_metadata.persona（ST 语义），仅对本聊天生效。
          </p>
        </div>
      )}

      {/* persona 列表 */}
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <Label>Persona 列表</Label>
          <Button size="sm" variant="secondary" onClick={addPersona}>
            <Plus className="size-3.5" />
            新建
          </Button>
        </div>
        <div className="flex flex-col gap-1.5">
          {ids.length === 0 && (
            <p className="text-xs text-muted-foreground">尚无 persona；未设置时使用下方用户名与描述。</p>
          )}
          {ids.map((id) => (
            <div
              key={id}
              className={'flex items-center gap-2 rounded-md border px-2.5 py-1.5 ' +
                (current === id ? 'border-primary/60 bg-primary/5' : 'bg-transparent')}
            >
              <button
                className="flex flex-1 items-center gap-2 text-left"
                onClick={() => selectPersona(id)}
              >
                <span className="text-sm">{personas[id]}</span>
                {power.default_persona === id && (
                  <Badge variant="secondary" className="px-1.5 text-[10px]">默认</Badge>
                )}
              </button>
              <Button
                variant="ghost"
                size="icon"
                className="size-6 text-muted-foreground hover:text-destructive"
                onClick={() => deletePersona(id)}
                title="删除"
              >
                <Trash2 className="size-3" />
              </Button>
            </div>
          ))}
        </div>
      </div>

      {/* 无选中 persona 时的全局回落 */}
      {!current && (
        <>
          <div className="flex flex-col gap-1.5">
            <Label>显示名（username）</Label>
            <Input
              value={power.username ?? 'User'}
              onChange={(e) => patch('power_user.username', e.target.value)}
              placeholder="User"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>Persona 描述（回落）</Label>
            <Textarea
              value={power.persona_description ?? ''}
              onChange={(e) => patch('power_user.persona_description', e.target.value)}
              placeholder="描述你是谁——将替换 prompt 中的 {{persona}}"
              rows={4}
              className="min-h-0 resize-y text-xs"
            />
          </div>
        </>
      )}

      {/* 选中 persona 编辑 */}
      {current && (
        <>
          <div className="flex flex-col gap-1.5">
            <Label>名称</Label>
            <Input
              value={personas[current] ?? ''}
              onChange={(e) => renamePersona(current, e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>描述</Label>
            <Textarea
              value={desc.description ?? ''}
              onChange={(e) =>
                patch(`power_user.persona_descriptions.${current}.description`, e.target.value)
              }
              placeholder="描述你是谁——将替换 prompt 中的 {{persona}}"
              rows={6}
              className="min-h-0 resize-y text-xs"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>注入位置</Label>
            <Select
              value={String(desc.position ?? 0)}
              onValueChange={(v) =>
                patch(`power_user.persona_descriptions.${current}.position`, Number(v))
              }
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PERSONA_POSITIONS.map((p) => (
                  <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {Number(desc.position ?? 0) === 4 && (
            <>
              <SliderField
                label="注入深度"
                value={desc.depth ?? 2}
                min={0}
                max={16}
                step={1}
                onChange={(v) => patch(`power_user.persona_descriptions.${current}.depth`, v)}
              />
              <div className="flex flex-col gap-1.5">
                <Label>注入角色</Label>
                <Select
                  value={String(desc.role ?? 0)}
                  onValueChange={(v) =>
                    patch(`power_user.persona_descriptions.${current}.role`, Number(v))
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {PERSONA_ROLES.map((r) => (
                      <SelectItem key={r.value} value={r.value}>{r.label}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </>
          )}
          <div className="flex justify-end">
            <Button size="sm" variant="secondary" onClick={() => selectPersona(current)}>
              <Check className="size-3.5" />
              设为默认
            </Button>
          </div>
        </>
      )}

      <p className="text-[10px] text-muted-foreground">
        改动随「保存」生效（写入 settings.json 的 power_user）；聊天绑定即时生效。
      </p>
      <p className="text-[10px] text-muted-foreground">
        {ids.length > 0 && '提示：全局用户名在无默认 persona 时使用。'}
      </p>
    </div>
  );
}
