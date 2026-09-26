import { v4 as uuidv4 } from 'uuid';
import type { LogicalModel, Route } from '@/models';

export const endpoints: Record<string, string> = {
  openai: 'https://api.openai.com/v1', anthropic: 'https://api.anthropic.com/v1', gemini: 'https://generativelanguage.googleapis.com/v1beta',
};
export const protocols: Record<string, string> = { openai: 'OpenAI 兼容', anthropic: 'Anthropic', gemini: 'Gemini' };
export const object = (value: unknown): Record<string, any> => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, any> : {};
export function newConnection(): Route {
  return { id: uuidv4(), provider: '', protocol: 'openai', upstream_model: '', priority: 0, enabled: true,
    config: { endpoint: endpoints.openai, connect_timeout_secs: 10, first_token_timeout_secs: 60, idle_timeout_secs: 60, headers: {}, parameters: {}, remove_parameters: [] } };
}
export function newModel(): LogicalModel {
  return { id: uuidv4(), display_name: '', routes: [newConnection()] };
}
export function primaryConnection(model: LogicalModel): Route | undefined {
  return model.routes.filter(r => r.enabled).sort((a, b) => b.priority - a.priority)[0] ?? model.routes[0];
}
export function outputValue(route: Route): number | undefined {
  const p = route.config.parameters;
  const value = route.protocol === 'gemini' ? object(p.generationConfig).maxOutputTokens : p.max_completion_tokens ?? p.max_tokens;
  return value as number | undefined ?? route.config.output_limit ?? undefined;
}
export function setOutput(route: Route, value: number | null, field?: string) {
  route.config.output_limit = value;
  const p = route.config.parameters;
  if (route.protocol === 'gemini') {
    const config = { ...object(p.generationConfig) };
    delete config.maxOutputTokens;
    if (value !== null) config.maxOutputTokens = value;
    p.generationConfig = config;
  } else {
    const key = field ?? ('max_completion_tokens' in p ? 'max_completion_tokens' : 'max_tokens');
    delete p.max_tokens; delete p.max_completion_tokens;
    route.config.remove_parameters = route.config.remove_parameters.filter(k => !['max_tokens', 'max_completion_tokens'].includes(k));
    if (value !== null) p[key] = value;
    if (value !== null && key === 'max_completion_tokens') route.config.remove_parameters.push('max_tokens');
  }
}
export const samplingKeys = ['temperature', 'top_p', 'frequency_penalty', 'presence_penalty'];
export function providerSampling(route: Route, enabled: boolean) {
  route.config.remove_parameters = route.config.remove_parameters.filter(k => !samplingKeys.includes(k));
  if (enabled) route.config.remove_parameters.push(...samplingKeys);
}
export function validateConnection(route: Route, fallbackOutput: number) {
  let url: URL;
  try { url = new URL(route.config.endpoint); } catch { return '请填写有效的 API 地址，例如 https://api.example.com/v1'; }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) return 'API 地址必须为 HTTP(S) 地址，不包含密钥、查询参数或片段。';
  if (!route.upstream_model.trim()) return '请填写模型 ID，或从获取的模型列表中选择。';
  for (const value of [route.config.context_limit, route.config.input_limit, outputValue(route)]) {
    if (value != null && (!Number.isSafeInteger(value) || value <= 0)) return '输入、输出和上下文限制必须为正整数。';
  }
  const output = outputValue(route) ?? fallbackOutput;
  if (route.config.context_limit && output >= route.config.context_limit) return '最大输出必须小于上下文窗口，为输入留出空间。';
  const thinking = object(route.config.parameters.thinking);
  if (route.protocol === 'anthropic' && thinking.type === 'enabled' &&
      (!Number.isSafeInteger(thinking.budget_tokens) || thinking.budget_tokens < 1024 || thinking.budget_tokens >= output)) {
    return 'Anthropic 思考预算至少为 1024，且必须小于最大输出 Token。';
  }
  const budget = object(object(route.config.parameters.generationConfig).thinkingConfig).thinkingBudget;
  if (route.protocol === 'gemini' && budget != null && (!Number.isSafeInteger(budget) || budget < -1)) return 'Gemini 思考预算须为非负整数，或用 -1 表示自动。';
  return '';
}
