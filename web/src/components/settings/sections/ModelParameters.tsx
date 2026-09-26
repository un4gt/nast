import { ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { object, outputValue, providerSampling, samplingKeys, setOutput } from '@/lib/model-config';
import type { Route } from '@/models';

// Reuses shadcn's Collapsible settings-panel and Field/Select compositions.
export function ModelParameters({ route, patch, presetOutput }: {
  route: Route; patch: (change: (route: Route) => void) => void; presetOutput: number;
}) {
  const p = route.config.parameters;
  const thinking = object(p.thinking);
  const gemini = object(object(p.generationConfig).thinkingConfig);
  const mode = route.protocol === 'openai' ? String(p.reasoning_effort ?? 'default')
    : route.protocol === 'anthropic' ? String(thinking.type ?? 'default')
    : gemini.thinkingLevel ? 'level' : gemini.thinkingBudget != null ? 'budget' : 'default';
  const options = route.protocol === 'openai'
    ? [['default', '使用模型默认值'], ['none', '关闭（none）'], ['minimal', '最低（minimal）'], ['low', '低（low）'], ['medium', '中（medium）'], ['high', '高（high）'], ['xhigh', '更高（xhigh）'], ['max', '最高（max）']]
    : route.protocol === 'anthropic'
      ? [['default', '使用模型默认值'], ['adaptive', '自适应思考'], ['enabled', '指定思考预算'], ['disabled', '关闭思考']]
      : [['default', '使用模型默认值'], ['level', '指定思考级别'], ['budget', '指定思考预算']];
  const setMode = (value: string) => patch(r => {
    const params = r.config.parameters;
    if (r.protocol === 'openai') {
      delete params.reasoning_effort;
      if (value !== 'default') params.reasoning_effort = value;
    } else if (r.protocol === 'anthropic') {
      delete params.thinking;
      const output = { ...object(params.output_config) }; delete output.effort;
      if (Object.keys(output).length) params.output_config = output; else delete params.output_config;
      if (value !== 'default') params.thinking = value === 'enabled' ? { type: 'enabled', budget_tokens: 1024 } : { type: value };
    } else {
      const config = { ...object(params.generationConfig) };
      const next = { ...object(config.thinkingConfig) }; delete next.thinkingBudget; delete next.thinkingLevel;
      if (value === 'budget') next.thinkingBudget = -1;
      if (value === 'level') next.thinkingLevel = 'LOW';
      if (Object.keys(next).length) config.thinkingConfig = next; else delete config.thinkingConfig;
      params.generationConfig = config;
    }
    if (r.protocol !== 'gemini' && !['default', 'none', 'disabled'].includes(value)) providerSampling(r, true);
  });
  return <Collapsible>
    <CollapsibleTrigger asChild><Button type="button" variant="ghost" className="w-full justify-between">模型参数<ChevronDown data-icon="inline-end" /></Button></CollapsibleTrigger>
    <CollapsibleContent className="pt-4"><FieldGroup>
      <FieldDescription>设置该模型的输入、输出预算和思考方式。留空时使用采样预设。</FieldDescription>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field><FieldLabel htmlFor="model-input-limit">最大输入 Token</FieldLabel><Input id="model-input-limit" type="number" min={1} step={1} placeholder="使用预设的输入预算" value={route.config.input_limit ?? ''} onChange={e => patch(r => { r.config.input_limit = e.target.value ? Number(e.target.value) : null; })} /><FieldDescription>包含系统提示、世界书和聊天历史。</FieldDescription></Field>
        <Field><FieldLabel htmlFor="model-output-limit">最大输出 Token</FieldLabel><Input id="model-output-limit" type="number" min={1} step={1} placeholder={`使用预设：${presetOutput}`} value={outputValue(route) ?? ''} onChange={e => patch(r => setOutput(r, e.target.value ? Number(e.target.value) : null))} /><FieldDescription>填写后覆盖预设的回复上限；思考模型通常也计入思考 Token。</FieldDescription></Field>
      </div>
      <Field><FieldLabel htmlFor="model-context-limit">上下文窗口（选填）</FieldLabel><Input id="model-context-limit" type="number" min={1} step={1} placeholder="模型支持的输入与输出总量" value={route.config.context_limit ?? ''} onChange={e => patch(r => { r.config.context_limit = e.target.value ? Number(e.target.value) : null; })} /><FieldDescription>填写后，输入预算不会超过“上下文窗口 − 最大输出”。</FieldDescription></Field>
      {route.protocol === 'openai' && <Field><FieldLabel htmlFor="model-output-field">输出参数</FieldLabel><Select value={'max_completion_tokens' in p ? 'max_completion_tokens' : 'max_tokens'} onValueChange={field => patch(r => setOutput(r, outputValue(r) ?? presetOutput, field))}><SelectTrigger id="model-output-field"><SelectValue /></SelectTrigger><SelectContent><SelectGroup><SelectItem value="max_tokens">max_tokens · 通用兼容接口</SelectItem><SelectItem value="max_completion_tokens">max_completion_tokens · OpenAI 推理模型</SelectItem></SelectGroup></SelectContent></Select></Field>}
      <Field><FieldLabel htmlFor="model-thinking">{route.protocol === 'openai' ? '思考强度' : '思考方式'}</FieldLabel><Select value={mode} onValueChange={setMode}><SelectTrigger id="model-thinking"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{options.map(([value, label]) => <SelectItem value={value} key={value}>{label}</SelectItem>)}</SelectGroup></SelectContent></Select><FieldDescription>支持范围由具体模型决定。不了解模型能力时保留默认值。</FieldDescription></Field>
      {route.protocol === 'anthropic' && mode === 'adaptive' && <Field><FieldLabel htmlFor="model-effort">思考强度</FieldLabel><Select value={String(object(p.output_config).effort ?? 'default')} onValueChange={effort => patch(r => { const next = { ...object(r.config.parameters.output_config) }; if (effort === 'default') delete next.effort; else next.effort = effort; r.config.parameters.output_config = next; })}><SelectTrigger id="model-effort"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{['default','low','medium','high','xhigh','max'].map(value => <SelectItem key={value} value={value}>{value === 'default' ? '使用模型默认值' : value}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>}
      {route.protocol === 'anthropic' && mode === 'enabled' && <Field><FieldLabel htmlFor="model-thinking-budget">思考预算 Token</FieldLabel><Input id="model-thinking-budget" type="number" min={1024} step={1} value={thinking.budget_tokens ?? ''} onChange={e => patch(r => { r.config.parameters.thinking = { type: 'enabled', budget_tokens: Number(e.target.value) }; })} /><FieldDescription>至少 1024，且小于最大输出。适用于支持手动思考预算的模型。</FieldDescription></Field>}
      {route.protocol === 'gemini' && mode === 'level' && <Field><FieldLabel htmlFor="model-thinking-level">思考级别</FieldLabel><Select value={String(gemini.thinkingLevel)} onValueChange={value => patch(r => { const config = object(r.config.parameters.generationConfig); config.thinkingConfig = { ...object(config.thinkingConfig), thinkingLevel: value }; })}><SelectTrigger id="model-thinking-level"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{['MINIMAL','LOW','MEDIUM','HIGH'].map(value => <SelectItem key={value} value={value}>{value.toLowerCase()}</SelectItem>)}</SelectGroup></SelectContent></Select><FieldDescription>用于支持 thinkingLevel 的 Gemini 3 及后续模型。</FieldDescription></Field>}
      {route.protocol === 'gemini' && mode === 'budget' && <Field><FieldLabel htmlFor="model-thinking-budget">思考预算 Token</FieldLabel><Input id="model-thinking-budget" type="number" min={-1} step={1} value={gemini.thinkingBudget ?? ''} onChange={e => patch(r => { const config = object(r.config.parameters.generationConfig); config.thinkingConfig = { ...object(config.thinkingConfig), thinkingBudget: Number(e.target.value) }; })} /><FieldDescription>-1 自动，0 关闭。可用范围及是否允许关闭取决于模型。</FieldDescription></Field>}
      {route.protocol !== 'gemini' && <Field orientation="horizontal"><FieldLabel htmlFor="model-default-sampling">使用服务商默认采样参数</FieldLabel><Switch id="model-default-sampling" checked={samplingKeys.every(k => route.config.remove_parameters.includes(k))} onCheckedChange={enabled => patch(r => providerSampling(r, enabled))} /></Field>}
      {route.protocol !== 'gemini' && <FieldDescription>启用后不发送 Temperature、Top P 和频率／存在惩罚；思考模型可能不支持这些参数。</FieldDescription>}
    </FieldGroup></CollapsibleContent>
  </Collapsible>;
}
