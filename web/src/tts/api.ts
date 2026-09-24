import { localVoices, providerInfo, SYSTEM_VOICE, type ProviderConfig, type Voice } from './config';

export async function ttsRequest(path: string, body: Record<string, unknown>, signal?: AbortSignal): Promise<Response> {
  const response = await fetch(`/api/tts/${path}`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body), signal,
  });
  if (!response.ok) {
    let message = `语音请求失败（HTTP ${response.status}）`;
    try { message = (await response.json()).error ?? message; } catch { /* retain HTTP status */ }
    throw new Error(message);
  }
  return response;
}
export async function fetchVoices(provider: string, config: ProviderConfig, signal?: AbortSignal): Promise<Voice[]> {
  if (provider === 'System') {
    if (!('speechSynthesis' in window)) throw new Error('此浏览器不支持系统朗读，请选择其他语音服务商');
    const voices = speechSynthesis.getVoices();
    return [
      { name: 'System Default Voice', voice_id: SYSTEM_VOICE, lang: navigator.language },
      ...voices.map((v) => ({ name: v.name, voice_id: v.voiceURI, lang: v.lang })),
    ];
  }
  if (providerInfo(provider).runtime === 'browser') return localVoices(providerInfo(provider), config);
  const response = await ttsRequest('voices', { provider, settings: config }, signal);
  const data = await response.json();
  if (!Array.isArray(data)) throw new Error('未收到音色列表，请确认后端已更新并重启');
  return data;
}
