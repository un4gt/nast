import { Label } from '@/components/ui/label';
import { Input } from '@/components/ui/input';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { SliderField, SwitchField } from '../fields';
import {
  Accordion, AccordionContent, AccordionItem, AccordionTrigger,
} from '@/components/ui/accordion';

const POSTFIX = [
  { value: ' ', label: '空格（默认）' },
  { value: '', label: '无' },
  { value: '\n', label: '换行' },
  { value: '\n\n', label: '双换行' },
];

const NAMES_BEHAVIOR = [
  { value: '0', label: '默认 — 仅群聊/强制头像带名字' },
  { value: '-1', label: '无 — 不带名字' },
  { value: '1', label: '补全对象 — name 字段' },
  { value: '2', label: '消息内容 — 全部加前缀' },
];

export function AiResponsePanel({
  oai,
  patchOai,
}: {
  oai: any;
  patchOai: (k: string, v: unknown) => void;
}) {
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">AI 回复行为</h3>
        <p className="text-xs text-muted-foreground">流式、续写、名字行为与 prompt 后处理（ST AI Response Configuration）。</p>
      </div>

      <SwitchField
        label="流式输出"
        checked={oai.stream_openai ?? true}
        onChange={(v) => patchOai('stream_openai', v)}
        description="关闭后整段返回"
      />
      <SwitchField
        label="合并连续 system 消息"
        checked={oai.squash_system_messages ?? false}
        onChange={(v) => patchOai('squash_system_messages', v)}
        description="Squash system messages"
      />

      <div className="flex flex-col gap-1.5">
        <Label>角色名字行为（names_behavior）</Label>
        <Select
          value={String(oai.character_names_behavior ?? 0)}
          onValueChange={(v) => patchOai('character_names_behavior', Number(v))}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {NAMES_BEHAVIOR.map((n) => (
              <SelectItem key={n.value} value={n.value}>
                {n.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>续写连接符（continue_postfix）</Label>
        <Select
          value={JSON.stringify(oai.continue_postfix ?? ' ')}
          onValueChange={(v) => patchOai('continue_postfix', JSON.parse(v))}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {POSTFIX.map((p) => (
              <SelectItem key={p.label} value={JSON.stringify(p.value)}>
                {p.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <SwitchField
        label="续写使用 Claude Prefill"
        checked={oai.continue_prefill ?? false}
        onChange={(v) => patchOai('continue_prefill', v)}
        description="仅 Anthropic 源生效；被续消息以 assistant prefill 形式注入"
      />

      <div className="flex flex-col gap-1.5">
        <Label>空输入发送（send_if_empty）</Label>
        <Input
          value={oai.send_if_empty ?? ''}
          onChange={(e) => patchOai('send_if_empty', e.target.value)}
          placeholder="留空则不发送"
        />
      </div>

      <Accordion type="multiple" className="border-t pt-2">
        <AccordionItem value="utility-prompts">
          <AccordionTrigger className="text-xs text-muted-foreground">实用提示词（Utility Prompts）</AccordionTrigger>
          <AccordionContent className="flex flex-col gap-3">
            <UtilityPromptField
              label="新聊天标记（new_chat_prompt）"
              value={oai.new_chat_prompt ?? '[Start a new Chat]'}
              onChange={(v) => patchOai('new_chat_prompt', v)}
            />
            <UtilityPromptField
              label="新群聊标记（new_group_chat_prompt）"
              value={oai.new_group_chat_prompt ?? '[Start a new group chat. Group members: {{group}}]'}
              onChange={(v) => patchOai('new_group_chat_prompt', v)}
            />
            <UtilityPromptField
              label="示例块标记（new_example_prompt）"
              value={oai.new_example_prompt ?? '[Example Chat]'}
              onChange={(v) => patchOai('new_example_prompt', v)}
            />
            <UtilityPromptField
              label="续写提示（continue_nudge_prompt）"
              value={oai.continue_nudge_prompt ?? '[Continue your last message without repeating its original content.]'}
              onChange={(v) => patchOai('continue_nudge_prompt', v)}
            />
            <UtilityPromptField
              label="群聊点名（group_nudge_prompt）"
              value={oai.group_nudge_prompt ?? '[Write the next reply only as {{char}}.]'}
              onChange={(v) => patchOai('group_nudge_prompt', v)}
            />
            <UtilityPromptField
              label="代入提示（impersonation_prompt）"
              value={oai.impersonation_prompt ?? ''}
              rows={4}
              onChange={(v) => patchOai('impersonation_prompt', v)}
            />
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </div>
  );
}

function UtilityPromptField({
  label,
  value,
  onChange,
  rows = 1,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  rows?: number;
}) {
  return (
    <div className="flex flex-col gap-1">
      <Label className="text-xs text-muted-foreground">{label}</Label>
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        rows={rows}
        className="w-full resize-y rounded-md border border-input bg-background px-3 py-1.5 text-xs shadow-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
      />
    </div>
  );
}
