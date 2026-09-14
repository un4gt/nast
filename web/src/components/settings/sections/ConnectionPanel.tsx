import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import {
  Accordion, AccordionContent, AccordionItem, AccordionTrigger,
} from '@/components/ui/accordion';

const SOURCES = [
  { value: 'custom', label: '自定义（OpenAI 兼容）' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'claude', label: 'Anthropic Claude' },
  { value: 'makersuite', label: 'Google Gemini' },
];

export function ConnectionPanel({
  oai,
  patchOai,
}: {
  oai: any;
  patchOai: (k: string, v: unknown) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <div>
        <h3 className="mb-1 text-sm font-semibold">API 连接</h3>
        <p className="text-xs text-muted-foreground">
          生成走服务端转发；密钥通过环境变量注入（OPENAI_API_KEY / ANTHROPIC_API_KEY / GOOGLE_API_KEY），自定义源用
          NAST_OPENAI_BASE。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>API 类型</Label>
        <Select
          value={oai.chat_completion_source ?? 'custom'}
          onValueChange={(v) => patchOai('chat_completion_source', v)}
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
          onChange={(e) => patchOai('openai_model', e.target.value)}
          placeholder="gpt-4o / claude-sonnet-4 / gemini-2.0-flash"
        />
      </div>

      <Accordion type="multiple" className="border-t pt-2">
        <AccordionItem value="env-help">
          <AccordionTrigger className="text-xs text-muted-foreground">
            环境变量速查
          </AccordionTrigger>
          <AccordionContent>
            <div className="flex flex-col gap-1.5 text-xs text-muted-foreground">
              <p><code className="rounded bg-secondary px-1">NAST_OPENAI_BASE</code> — 自定义源 baseURL（vLLM / 中转 / OpenRouter 等）</p>
              <p><code className="rounded bg-secondary px-1">OPENAI_API_KEY</code> — OpenAI / 自定义源密钥</p>
              <p><code className="rounded bg-secondary px-1">ANTHROPIC_API_KEY</code> — Claude 密钥</p>
              <p><code className="rounded bg-secondary px-1">GOOGLE_API_KEY</code> — Gemini 密钥</p>
              <p><code className="rounded bg-secondary px-1">NAST_PORT</code> / <code className="rounded bg-secondary px-1">NAST_DATA</code> — 端口与数据目录</p>
            </div>
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </div>
  );
}
