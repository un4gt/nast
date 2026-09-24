import { useEffect, useMemo, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import {
  Accordion, AccordionContent, AccordionItem, AccordionTrigger,
} from '@/components/ui/accordion';
import {
  Popover, PopoverContent, PopoverTrigger,
} from '@/components/ui/popover';
import {
  Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList,
} from '@/components/ui/command';
import { Check, ChevronsUpDown, KeyRound, Loader2, RefreshCw } from 'lucide-react';
import { cn } from '@/lib/utils';
import { rpc } from '@/rpc';
import { pushToast } from '@/toasts';

const SECRET_KEY = 'api_key_custom';

export function ConnectionPanel({
  oai,
  patchOai,
}: {
  oai: any;
  patchOai: (k: string, v: unknown) => void;
}) {
  const [keyMask, setKeyMask] = useState('');
  const [keyInput, setKeyInput] = useState('');
  const [savingKey, setSavingKey] = useState(false);

  const [modelOpen, setModelOpen] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);

  // 现阶段仅暴露 OpenAI 兼容源；进入面板即迁移源值
  useEffect(() => {
    if ((oai.chat_completion_source ?? 'custom') !== 'custom') {
      patchOai('chat_completion_source', 'custom');
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const secrets = await rpc.call<Record<string, any>>('secrets.get');
        const active = (secrets?.[SECRET_KEY] ?? []).find((e: any) => e.active);
        setKeyMask(active?.masked ?? '');
      } catch {
        /* 未连接时静默 */
      }
    })();
  }, []);

  const currentModel = oai.custom_model || oai.openai_model || '';

  const fetchModels = async () => {
    setLoadingModels(true);
    try {
      const params: Record<string, unknown> = {};
      const url = (oai.custom_url ?? '').trim();
      if (url) params.url = url;
      if (keyInput.trim()) params.key = keyInput.trim();
      const res = await rpc.call<{ data: string[] }>('models.list', params);
      setModels(res.data ?? []);
      pushToast(`已获取 ${res.data?.length ?? 0} 个模型`, 'success');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setLoadingModels(false);
    }
  };

  const saveKey = async () => {
    setSavingKey(true);
    try {
      const res = await rpc.call<{ masked: string }>('secrets.set', {
        key: SECRET_KEY,
        value: keyInput.trim(),
      });
      setKeyMask(keyInput.trim() ? res.masked : '');
      setKeyInput('');
      pushToast(keyInput.trim() ? '密钥已保存' : '密钥已清除', 'success');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setSavingKey(false);
    }
  };

  const modelItems = useMemo(() => models, [models]);

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h3 className="mb-1 text-sm font-semibold">API 连接</h3>
        <p className="text-xs text-muted-foreground">
          连接你喜欢的模型服务，支持 OpenRouter、DeepSeek 等 OpenAI 兼容接口。填写端点与模型后，点击下方保存更改。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>API 类型</Label>
        <Select value="custom" onValueChange={() => {}}>
          <SelectTrigger aria-label="API 类型">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup><SelectItem value="custom">自定义（OpenAI 兼容）</SelectItem></SelectGroup>
          </SelectContent>
        </Select>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="api-endpoint">自定义端点（含 /v1）</Label>
        <Input
          id="api-endpoint"
          value={oai.custom_url ?? ''}
          onChange={(e) => patchOai('custom_url', e.target.value)}
          placeholder="https://openrouter.ai/api/v1"
        />
        <p className="text-xs text-muted-foreground">
          服务端将自动拼接 <code className="rounded bg-secondary px-1">/chat/completions</code>。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="api-key">API 密钥</Label>
        <div className="flex items-center gap-2">
          <KeyRound className="size-4 shrink-0 text-muted-foreground" />
          <Input
            id="api-key"
            className="min-w-0"
            type="password"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder={keyMask ? `已保存 ${keyMask}（输入以更换）` : 'sk-…（留空保存 = 清除）'}
            autoComplete="off"
          />
          <Button
            size="sm"
            variant="secondary"
            disabled={savingKey || !keyInput.trim()}
            onClick={() => void saveKey()}
          >
            {savingKey && <Loader2 className="size-3.5 animate-spin" />}
            保存
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          「获取模型列表」优先使用输入框中的密钥（未保存则仅本次生效）；生成始终使用已保存的密钥。
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label>模型</Label>
        <div className="flex items-center gap-2">
          <Popover open={modelOpen} onOpenChange={setModelOpen}>
            <PopoverTrigger asChild>
              <Button
                variant="outline"
                role="combobox"
                aria-label="选择模型"
                aria-expanded={modelOpen}
                className="min-w-0 flex-1 justify-between font-normal"
              >
                <span className="truncate">{currentModel || '选择或输入模型…'}</span>
                <ChevronsUpDown className="size-4 shrink-0 opacity-50" />
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0" align="start">
              <Command>
                <CommandInput placeholder="搜索模型（先点右侧获取列表）…" />
                <CommandList>
                  <CommandEmpty>
                    {modelItems.length === 0 ? '点击「获取模型列表」拉取，或在下方手动填写' : '无匹配模型'}
                  </CommandEmpty>
                  {modelItems.length > 0 && (
                    <CommandGroup>
                      {modelItems.map((m) => (
                        <CommandItem
                          key={m}
                          value={m}
                          onSelect={() => {
                            patchOai('custom_model', m);
                            setModelOpen(false);
                          }}
                        >
                          <Check className={cn('size-4', currentModel === m ? 'opacity-100' : 'opacity-0')} />
                          {m}
                        </CommandItem>
                      ))}
                    </CommandGroup>
                  )}
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
          <Button
            size="sm"
            variant="secondary"
            disabled={loadingModels}
            onClick={() => void fetchModels()}
          >
            {loadingModels ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <RefreshCw className="size-3.5" />
            )}
            获取模型列表
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          列表来自端点 <code className="rounded bg-secondary px-1">GET /v1/models</code>
          ；获取按钮同时校验 URL 与密钥。
        </p>
        <Input
          aria-label="模型 ID"
          value={oai.custom_model ?? ''}
          onChange={(e) => patchOai('custom_model', e.target.value)}
          placeholder="或手动填写模型 ID（如 deepseek-chat）"
        />
      </div>

      <Accordion type="multiple" className="border-t pt-2">
        <AccordionItem value="advanced">
          <AccordionTrigger className="text-xs text-muted-foreground">
            高级：附加请求头 / 请求体
          </AccordionTrigger>
          <AccordionContent className="flex flex-col gap-4">
            <div className="flex flex-col gap-1.5">
              <Label>附加请求头（每行 Header-Name: value）</Label>
              <Textarea
                rows={3}
                value={oai.custom_include_headers ?? ''}
                onChange={(e) => patchOai('custom_include_headers', e.target.value)}
                placeholder={'X-Title: my-app\nHTTP-Referer: https://example.com'}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>附加请求体（JSON 对象，合并进请求）</Label>
              <Textarea
                rows={3}
                value={oai.custom_include_body ?? ''}
                onChange={(e) => patchOai('custom_include_body', e.target.value)}
                placeholder='{"top_k": 5}'
              />
            </div>
          </AccordionContent>
        </AccordionItem>
        <AccordionItem value="env-help">
          <AccordionTrigger className="text-xs text-muted-foreground">
            环境变量降级说明
          </AccordionTrigger>
          <AccordionContent>
            <div className="flex flex-col gap-1.5 text-xs text-muted-foreground">
              <p>密钥未在 UI 保存时回落 <code className="rounded bg-secondary px-1">OPENAI_API_KEY</code>；端点为空时回落 <code className="rounded bg-secondary px-1">NAST_OPENAI_BASE</code>（默认 OpenAI 官方）。</p>
              <p><code className="rounded bg-secondary px-1">NAST_PORT</code> / <code className="rounded bg-secondary px-1">NAST_DATA</code> — 端口与数据目录。</p>
            </div>
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </div>
  );
}
