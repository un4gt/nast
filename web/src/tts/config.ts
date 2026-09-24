import catalog from '../../../resources/tts-providers.json';

export interface Voice { name: string; voice_id: string; lang?: string; data?: string }
export type ProviderConfig = Record<string, any>;
export interface Provider {
  name: string;
  label?: string;
  runtime: 'http' | 'system' | 'browser';
  description?: string;
  defaults: ProviderConfig;
  models: string[];
  voices: (string | Voice)[];
  secrets: string[];
  maxLength?: number;
}
export const providers = catalog as Provider[];
export const DEFAULT_VOICE = '[Default Voice]';
export const DISABLED_VOICE = 'disabled';
export const SYSTEM_VOICE = '__browser_default__';
export const defaults = {
  enabled: false,
  currentProvider: 'System',
  auto_generation: true,
  narrate_user: false,
  periodic_auto_generation: false,
  narrate_by_paragraphs: false,
  narrate_quoted_only: false,
  narrate_dialogues_only: false,
  narrate_translated_only: false,
  skip_codeblocks: false,
  skip_tags: false,
  pass_asterisks: false,
  multi_voice_enabled: false,
  apply_regex: false,
  regex_pattern: '',
  playback_rate: 1,
};
export type TtsSettings = typeof defaults & Record<string, any>;
export function readTtsSettings(settings: any): TtsSettings {
  return { ...defaults, ...settings?.extension_settings?.tts };
}
export function providerInfo(name: string): Provider {
  const provider = providers.find((p) => p.name === name);
  if (!provider) throw new Error(`未知的 TTS 服务商：${name}`);
  return provider;
}
export function providerConfig(tts: TtsSettings): ProviderConfig {
  const provider = providerInfo(tts.currentProvider);
  return { ...provider.defaults, ...tts[provider.name], voiceMap: parseVoiceMap(tts[provider.name]?.voiceMap) };
}
export function parseVoiceMap(value: unknown): Record<string, string> {
  if (typeof value === 'string') {
    return Object.fromEntries(value.split(',').flatMap((part) => {
      const at = part.indexOf(':');
      return at > 0 ? [[part.slice(0, at).trim(), part.slice(at + 1).trim()]] : [];
    }));
  }
  return value && typeof value === 'object' && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === 'string')) : {};
}
export function localVoices(provider: Provider, config: ProviderConfig): Voice[] {
  const normalize = (items: any[]): Voice[] => items.map((item) => typeof item === 'string'
    ? { name: item, voice_id: item } : { ...item, voice_id: String(item.voice_id ?? item.id ?? item.name) });
  if (provider.name === 'System') return [{ name: 'System Default Voice', voice_id: SYSTEM_VOICE }];
  if (provider.name === 'SpeechT5') return normalize(config.speakers ?? []);
  if (provider.name === 'Coqui') return Object.keys(config.customVoices ?? {}).map((name) => ({ name, voice_id: name }));
  const list = normalize(config.available_voices ?? provider.voices);
  if (Array.isArray(config.customVoices)) list.push(...normalize(config.customVoices));
  return list.filter((v, i) => v.voice_id && list.findIndex((w) => w.voice_id === v.voice_id) === i);
}
export function resolveVoice(tts: TtsSettings, config: ProviderConfig, name: string, segment: string): string {
  const map = parseVoiceMap(config.voiceMap);
  const suffix = segment === 'dialogue' ? ' ("Quotes")' : segment === 'action' ? ' (*Text inside asterisks*)' : ' (Other text)';
  let mapped = map[tts.multi_voice_enabled && name !== DEFAULT_VOICE ? name + suffix : name] ?? map[name] ?? DEFAULT_VOICE;
  if (mapped === DEFAULT_VOICE) mapped = map[DEFAULT_VOICE] ?? (tts.currentProvider === 'System' ? SYSTEM_VOICE : '');
  return mapped;
}
