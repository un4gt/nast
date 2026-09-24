import { useEffect, useId, useRef, useState } from 'react';
import { Headphones, Play, RefreshCw, Save, Square, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion';
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Spinner } from '@/components/ui/spinner';
import { useStore } from '@/store';
import { rpc } from '@/rpc';
import { pushToast } from '@/toasts';
import { fetchVoices, ttsRequest } from '@/tts/api';
import { DEFAULT_VOICE, DISABLED_VOICE, SYSTEM_VOICE, localVoices, parseVoiceMap, providerConfig, providers, readTtsSettings, type TtsSettings, type Voice } from '@/tts/config';
import { ttsPlayer, useTtsPlayback } from '@/tts/player';
import { parseFilter } from '@/tts/text';
import { TtsServiceTools } from './TtsServiceTools';

const toggleLabels: [keyof TtsSettings, string, string?][] = [
  ['auto_generation', '自动朗读新消息'],
  ['narrate_user', '也朗读用户消息'],
  ['periodic_auto_generation', '生成时按段朗读', '等待完整段落后朗读；需要开启自动朗读。'],
  ['narrate_by_paragraphs', '完整消息也按段合成'],
  ['narrate_quoted_only', '只朗读引号内容', '与 SillyTavern 一致：找不到完整引号时朗读原文。'],
  ['narrate_dialogues_only', '忽略星号内的动作描写'],
  ['narrate_translated_only', '朗读翻译 / 显示文本', '自动朗读仅处理已有显示文本的消息。'],
  ['skip_codeblocks', '跳过代码块'], ['skip_tags', '跳过成对标签包裹的内容'],
  ['pass_asterisks', '将星号传给语音引擎', '开启后保留动作标记，用于下方的多音色分段。'],
  ['multi_voice_enabled', '为对话、动作、其他文本分别分配音色'],
  ['apply_regex', '使用正则过滤文本'],
];
const labels: Record<string, string> = {
  provider_endpoint: '服务端点', apiHost: 'API Host', region: 'Azure 区域', model: '语音模型', modelId: '模型仓库',
  speed: '合成语速', rate: '语速', pitch: '音高', speakingRate: '合成语速', resource_id: 'Resource ID',
  available_voices: '可用音色（逗号分隔）', customVoices: '自定义音色', customModels: '自定义模型',
  language: '语言', lang: '语言', text_lang: '文本语言', prompt_lang: '参考音频语言',
  stability: '稳定性', similarity_boost: '音色相似度', style_exaggeration: '风格强度', speaker_boost: '增强说话人特征',
  reuse_history: '复用已有合成记录', instructions: '朗读风格指令',
  volume: '音量', format: '音频格式', output_format: '输出格式', audioSampleRate: '采样率', bitrate: '比特率',
  temperature: '温度', top_p: 'Top P', top_k: 'Top K', seed: '随机种子（-1 为随机）',
  server_version: 'AllTalk 服务版本', narrator_enabled: '启用旁白（true / false）', narrator_voice_gen: '旁白音色',
  at_narrator_text_not_inside: '非引号文本归属', at_generation_method: '生成方式',
  rvc_character_voice: '角色 RVC 音色', rvc_character_pitch: '角色 RVC 音高', rvc_narrator_voice: '旁白 RVC 音色', rvc_narrator_pitch: '旁白 RVC 音高',
  provider: 'Edge 服务类型（extras / plugin）', dtype: '模型精度', device: '运行设备', media_type: '音频封装',
  speed_factor: '语速系数', exaggeration: '表现力', cfg_weight: 'CFG 权重', split_text: '自动切分文本', chunk_size: '每段字符数',
  length: '音频时长系数', noise: '噪声', noisew: '噪声宽度', sdp_ratio: 'SDP 比例',
  reference_audio_path: '参考音频路径', customVoiceId: '自定义 Voice ID',
  streaming: '使用服务商流式响应', stream: '使用服务商流式响应',
  characterInstructions: '按角色设置朗读风格', speakers: '音色嵌入',
};

export function TtsPanel({ draft, patch }: { draft: any; patch: (path: string, value: unknown) => void }) {
  const tts = readTtsSettings(draft);
  const provider = providers.find((p) => p.name === tts.currentProvider);
  const config = provider ? providerConfig(tts) : {};
  const store = useStore();
  const [voices, setVoices] = useState<Voice[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [sample, setSample] = useState('你好，很高兴与你相遇。让我们继续这个故事吧。');
  const [voiceQuery, setVoiceQuery] = useState('');
  const [models, setModels] = useState<string[]>([]);
  const [modelLoading, setModelLoading] = useState(false);
  const [extra, setExtra] = useState('');
  const request = useRef<AbortController>();
  const playback = useTtsPlayback();
  const voiceListId = useId();
  const modelListId = useId();
  const fingerprint = JSON.stringify([tts.currentProvider, config.provider_endpoint, config.apiHost, config.region, config.model, config.available_voices, config.customVoices, config.speakers]);
  useEffect(() => {
    request.current?.abort();
    setVoices(provider ? localVoices(provider, config) : []);
    setModels(provider?.models ?? []);
    setLoaded(false); setError(''); setLoading(false); setModelLoading(false);
    return () => request.current?.abort();
  }, [fingerprint]);
  const change = (key: string, value: unknown) => patch('extension_settings.tts', { ...tts, [key]: value });
  const changeProvider = (key: string, value: unknown) => change(tts.currentProvider, { ...config, [key]: value });
  const map = parseVoiceMap(config.voiceMap);
  const mapped = (name: string, value: string) => changeProvider('voiceMap', { ...map, [name]: value });
  const characters = Array.from(new Set([
    String(draft.name1 || 'User'),
    ...store.characters.map((c) => c.name),
    ...store.messages.map((m) => m.name),
    ...Object.keys(map).filter((name) => name !== DEFAULT_VOICE && !/ \((?:"Quotes"|\*Text inside asterisks\*|Other text)\)$/.test(name)),
  ]));
  let regexError = '';
  if (tts.apply_regex && tts.regex_pattern) {
    try { parseFilter(tts.regex_pattern); } catch { regexError = '正则表达式无效，请检查语法后再朗读。'; }
  }
  const refresh = async () => {
    request.current?.abort();
    const controller = new AbortController(); request.current = controller;
    setLoading(true); setError('');
    try {
      const list = await fetchVoices(tts.currentProvider, config, controller.signal);
      if (controller.signal.aborted) return;
      setVoices(list); setLoaded(true); ttsPlayer.clearVoiceCache();
    } catch (error) {
      if (!controller.signal.aborted) setError(error instanceof Error ? error.message : String(error));
    } finally { if (!controller.signal.aborted) setLoading(false); }
  };
  const loadModels = async () => {
    request.current?.abort();
    const controller = new AbortController(); request.current = controller;
    setModelLoading(true); setError('');
    try {
      const response = await ttsRequest('models', { provider: tts.currentProvider, settings: config }, controller.signal);
      const list = await response.json();
      if (!Array.isArray(list)) throw new Error('未收到模型列表');
      if (!controller.signal.aborted) setModels(list.map((v: any) => typeof v === 'string' ? v : v.id));
    } catch (error) { if (!controller.signal.aborted) setError(error instanceof Error ? error.message : String(error)); }
    finally { if (!controller.signal.aborted) setModelLoading(false); }
  };
  const renderValue = (key: string, value: any) => {
    if (typeof value === 'boolean') return <Toggle key={key} label={labels[key] ?? key} checked={Boolean(config[key])} onChange={(v) => changeProvider(key, v)} />;
    if (typeof value === 'object') return null;
    return <Field key={key}><FieldLabel htmlFor={`tts-${key}`}>{labels[key] ?? key}</FieldLabel>
      <Input id={`tts-${key}`} type={typeof value === 'number' ? 'number' : 'text'} step="any" value={config[key] ?? value}
        onChange={(e) => changeProvider(key, typeof value === 'number' && e.target.value !== '' ? Number(e.target.value) : e.target.value)} /></Field>;
  };
  return (
    <div className="flex min-w-0 flex-col gap-6">
      <div className="flex items-center gap-3">
        <Headphones className="size-5 text-primary" />
        <div><h2 className="text-base font-semibold">语音朗读</h2><p className="mt-1 text-xs text-muted-foreground">为角色挑选声音，让对话自然延续。</p></div>
      </div>
      <FieldGroup>
        <Toggle label="启用语音朗读" checked={tts.enabled} onChange={(v) => change('enabled', v)} description="保存后，可从消息旁的扬声器按钮开始朗读。" />
        <Field><FieldLabel htmlFor="tts-provider">语音服务商</FieldLabel>
          <Select value={tts.currentProvider} onValueChange={(name) => { ttsPlayer.cancel(); change('currentProvider', name); }}>
            <SelectTrigger id="tts-provider"><SelectValue /></SelectTrigger>
            <SelectContent><SelectGroup>{providers.map((p) => <SelectItem key={p.name} value={p.name}>{p.label ?? p.name}</SelectItem>)}</SelectGroup></SelectContent>
          </Select>
          {provider?.description && <FieldDescription>{provider.description}</FieldDescription>}
        </Field>
        {!provider ? <Alert variant="destructive"><AlertTitle>服务商未安装</AlertTitle><AlertDescription>请从上方选择支持的服务商。原有配置会保留。</AlertDescription></Alert> : <>
          {['provider_endpoint', 'apiHost', 'region', 'resource_id', 'modelId'].filter((key) => key in config).map((key) => renderValue(key, config[key]))}
          {provider.secrets.map((key) => <SecretField key={key} secretKey={key} onSaved={() => { ttsPlayer.clearVoiceCache(); setLoaded(false); }} />)}
          {'model' in config && <Field><FieldLabel htmlFor="tts-model">语音模型</FieldLabel>
            <Input id="tts-model" list={modelListId} value={config.model} onChange={(e) => changeProvider('model', e.target.value)} placeholder="选择或输入模型 ID" />
            <datalist id={modelListId}>{models.map((m) => <option key={m} value={m} />)}</datalist>
            {provider.runtime === 'http' && <Button variant="outline" size="sm" className="self-start" disabled={modelLoading || loading} onClick={() => void loadModels()}>
              {modelLoading ? <Spinner /> : <RefreshCw data-icon="inline-start" />}获取模型列表
            </Button>}
          </Field>}
          {['speed', 'rate', 'pitch', 'speakingRate'].filter((key) => key in config).map((key) => renderValue(key, config[key]))}
          <Field><FieldLabel htmlFor="tts-default-voice">默认音色</FieldLabel>
            <Input id="tts-default-voice" list={voiceListId} value={map[DEFAULT_VOICE] === SYSTEM_VOICE || (!map[DEFAULT_VOICE] && tts.currentProvider === 'System') ? 'System Default Voice' : map[DEFAULT_VOICE] ?? ''} placeholder="选择下方音色，或输入音色名称 / ID" onChange={(e) => mapped(DEFAULT_VOICE, e.target.value)} />
            <datalist id={voiceListId}>{voices.map((v) => <option key={v.voice_id} value={v.name}>{v.lang} · {v.voice_id}</option>)}</datalist>
            <FieldDescription>没有单独指定音色的角色会使用此音色；自定义 Voice ID 可直接输入。</FieldDescription>
          </Field>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" disabled={loading || modelLoading} onClick={() => void refresh()}>{loading ? <Spinner /> : <RefreshCw data-icon="inline-start" />}刷新音色</Button>
            <Badge variant="secondary">{voices.length} 个音色</Badge>
            {loaded && <span className="text-xs text-muted-foreground">列表已更新</span>}
          </div>
          {error && <Alert variant="destructive"><AlertTitle>连接未完成</AlertTitle><AlertDescription>{error}</AlertDescription></Alert>}
          {!!voices.length && <Field>
            <FieldLabel htmlFor="tts-voice-search">查找音色</FieldLabel>
            <Input id="tts-voice-search" value={voiceQuery} onChange={(e) => setVoiceQuery(e.target.value)} placeholder="名称、ID 或语言" />
            <div className="flex max-h-44 flex-col gap-1 overflow-y-auto rounded-lg border p-1">
              {voices.filter((v) => `${v.name} ${v.voice_id} ${v.lang}`.toLowerCase().includes(voiceQuery.toLowerCase())).map((v) =>
                <Button key={v.voice_id} variant={map[DEFAULT_VOICE] === v.name || map[DEFAULT_VOICE] === v.voice_id ? 'secondary' : 'ghost'} size="sm" className="h-auto min-h-9 justify-between gap-3 whitespace-normal text-left" onClick={() => mapped(DEFAULT_VOICE, v.name)}>
                  <span className="min-w-0 break-all">{v.name}</span><span className="shrink-0 text-xs text-muted-foreground">{v.lang}</span>
                </Button>)}
            </div>
          </Field>}
          <Field><FieldLabel htmlFor="tts-sample">试听文本</FieldLabel><Textarea id="tts-sample" rows={2} value={sample} onChange={(e) => setSample(e.target.value)} />
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="outline" disabled={!sample.trim() || Boolean(regexError)} onClick={() => ttsPlayer.speakMessage({ name: DEFAULT_VOICE, mes: sample, is_user: false, is_system: false, send_date: '' }, null, { manual: true, settings: { ...tts, enabled: true } })}>
                <Play data-icon="inline-start" />试听当前配置
              </Button>
              {!['idle', 'error'].includes(playback.status) && <Button size="sm" variant="ghost" onClick={ttsPlayer.cancel}><Square data-icon="inline-start" />停止试听</Button>}
            </div>
            <FieldDescription role="status">{!['idle', 'error'].includes(playback.status) ? playback.label : '试听使用当前草稿配置；密钥需先单独保存。'}</FieldDescription>
          </Field>
        </>}
      </FieldGroup>
      <Accordion type="multiple" defaultValue={['behavior']}>
        <AccordionItem value="behavior"><AccordionTrigger>朗读规则</AccordionTrigger><AccordionContent>
          <FieldGroup>{toggleLabels.map(([key, label, description]) => <Toggle key={key} label={label} description={description} checked={Boolean(tts[key])} onChange={(v) => change(key, v)} />)}
            {tts.apply_regex && <Field data-invalid={Boolean(regexError)}><FieldLabel htmlFor="tts-regex">移除匹配的文本</FieldLabel><Input id="tts-regex" aria-invalid={Boolean(regexError)} value={tts.regex_pattern} placeholder="/匹配内容/g" onChange={(e) => change('regex_pattern', e.target.value)} />{regexError && <FieldDescription role="alert">{regexError}</FieldDescription>}</Field>}
            {tts.currentProvider !== 'System' && <Field><FieldLabel htmlFor="tts-playback-rate">播放倍速（0.25–3）</FieldLabel><Input id="tts-playback-rate" type="number" min={0.25} max={3} step={0.05} value={tts.playback_rate} onChange={(e) => change('playback_rate', Number(e.target.value))} /></Field>}
          </FieldGroup>
        </AccordionContent></AccordionItem>
        {provider && <AccordionItem value="voicemap"><AccordionTrigger>角色音色映射</AccordionTrigger><AccordionContent>
          <FieldGroup>{characters.map((name) => <FieldSet key={name}><FieldLegend className={tts.multi_voice_enabled ? undefined : 'sr-only'}>{name}</FieldLegend>
            {(tts.multi_voice_enabled ? [' ("Quotes")', ' (*Text inside asterisks*)', ' (Other text)'] : ['']).map((suffix, i) => <VoiceField key={suffix} label={suffix ? ['对话', '动作', '其他文本'][i] : name} value={map[name + suffix] ?? DEFAULT_VOICE} voices={voices} onChange={(value) => mapped(name + suffix, value)} />)}
            {tts.currentProvider === 'OpenAI' && String(config.model).startsWith('gpt-4o-mini-tts') && <Field><FieldLabel htmlFor={`tts-instruction-${name}`}>朗读风格</FieldLabel><Textarea id={`tts-instruction-${name}`} rows={2} value={config.characterInstructions?.[name] ?? ''} onChange={(e) => changeProvider('characterInstructions', { ...config.characterInstructions, [name]: e.target.value })} placeholder="例如：温柔、平静地讲述" /></Field>}
          </FieldSet>)}</FieldGroup>
        </AccordionContent></AccordionItem>}
        {provider && ['AllTalk', 'Coqui'].includes(provider.name) && <AccordionItem value="service"><AccordionTrigger>模型服务管理</AccordionTrigger><AccordionContent>
          <TtsServiceTools key={`${provider.name}:${config.provider_endpoint}`} provider={provider.name} config={config} onChange={(value) => change(provider.name, value)} />
        </AccordionContent></AccordionItem>}
        {provider && <AccordionItem value="provider"><AccordionTrigger>服务商高级参数</AccordionTrigger><AccordionContent>
          <FieldGroup>{Object.entries(provider.defaults).filter(([key]) => !['provider_endpoint','apiHost','region','resource_id','model','modelId','speed','rate','pitch','speakingRate','voiceMap','defaultVoice'].includes(key)).map(([key, value]) => renderValue(key, value))}
            {'available_voices' in config && <Field><FieldLabel htmlFor="tts-available">可用音色（逗号分隔）</FieldLabel><Input id="tts-available" value={(config.available_voices ?? []).join(', ')} onChange={(e) => changeProvider('available_voices', e.target.value.split(',').map((v) => v.trim()).filter(Boolean))} /></Field>}
            {tts.currentProvider === 'SpeechT5' && <Field><FieldLabel htmlFor="tts-speaker-file">添加音色嵌入（.bin）</FieldLabel><Input id="tts-speaker-file" type="file" accept=".bin" onChange={(e) => {
              const file = e.target.files?.[0]; if (!file) return;
              if (file.size !== 2048) { pushToast('音色文件必须为 512 维 Float32（2048 字节）', 'error'); return; }
              void fileData(file).then((data) => changeProvider('speakers', [...(config.speakers ?? []).filter((v: Voice) => v.name !== file.name), { name: file.name, voice_id: file.name, data }]));
            }} /></Field>}
            {tts.currentProvider === 'SpeechT5' && (config.speakers ?? []).map((speaker: Voice) => <Field key={speaker.voice_id} orientation="horizontal"><FieldContent><FieldLabel>{speaker.name}</FieldLabel></FieldContent><Button variant="ghost" size="icon" aria-label={`移除音色 ${speaker.name}`} onClick={() => changeProvider('speakers', config.speakers.filter((v: Voice) => v.voice_id !== speaker.voice_id))}><Trash2 /></Button></Field>)}
            <Field><FieldLabel htmlFor="tts-advanced-json">补充配置（JSON 对象）</FieldLabel><Textarea id="tts-advanced-json" rows={4} value={extra} onChange={(e) => setExtra(e.target.value)} placeholder={'{"customVoices": ["my-voice"]}'} />
              <FieldDescription>用于自定义音色、Coqui 模型映射及服务商参数。API 密钥请使用上方密钥栏。</FieldDescription>
              <Button variant="outline" size="sm" className="self-start" disabled={!extra.trim()} onClick={() => {
                try { const value = JSON.parse(extra); if (!value || Array.isArray(value) || typeof value !== 'object') throw new Error();
                  if (Object.keys(value).some((key) => /key|secret|token|authorization/i.test(key))) { pushToast('请在密钥栏保存密钥', 'error'); return; }
                  change(tts.currentProvider, { ...config, ...value }); setExtra(''); pushToast('已加入草稿，请保存更改', 'success');
                } catch { pushToast('请输入有效的 JSON 对象', 'error'); }
              }}>加入草稿</Button>
            </Field>
            {tts.currentProvider === 'ElevenLabs' && <VoiceUpload config={config} onAdded={() => void refresh()} />}
          </FieldGroup>
        </AccordionContent></AccordionItem>}
      </Accordion>
    </div>
  );
}

function Toggle({ label, description, checked, onChange }: { label: string; description?: string; checked: boolean; onChange: (v: boolean) => void }) {
  const id = useId();
  return <Field orientation="horizontal"><FieldContent><FieldLabel htmlFor={id}>{label}</FieldLabel>{description && <FieldDescription id={`${id}-desc`}>{description}</FieldDescription>}</FieldContent><Switch id={id} checked={checked} aria-describedby={description ? `${id}-desc` : undefined} onCheckedChange={onChange} /></Field>;
}
function VoiceField({ label, value, voices, onChange }: { label: string; value: string; voices: Voice[]; onChange: (v: string) => void }) {
  const id = useId();
  return <Field><FieldLabel htmlFor={id}>{label}</FieldLabel><Select value={value} onValueChange={onChange}>
    <SelectTrigger id={id}><SelectValue /></SelectTrigger><SelectContent updatePositionStrategy="always" sticky="always" className="max-h-[min(20rem,var(--radix-select-content-available-height))]"><SelectGroup>
      <SelectItem value={DEFAULT_VOICE}>使用默认音色</SelectItem><SelectItem value={DISABLED_VOICE}>不朗读</SelectItem>
      {!voices.some((v) => v.name === value) && ![DEFAULT_VOICE, DISABLED_VOICE].includes(value) && <SelectItem value={value}>{value}</SelectItem>}
      {voices.filter((v, i) => v.name && voices.findIndex((w) => w.name === v.name) === i).map((v) => <SelectItem key={v.voice_id} value={v.name}>{v.name}</SelectItem>)}
    </SelectGroup></SelectContent>
  </Select><Input aria-label={`${label} 自定义音色 ID`} placeholder="或输入自定义音色 ID" value={[DEFAULT_VOICE, DISABLED_VOICE].includes(value) ? '' : value} onChange={(e) => onChange(e.target.value || DEFAULT_VOICE)} /></Field>;
}
function SecretField({ secretKey, onSaved }: { secretKey: string; onSaved: () => void }) {
  const [value, setValue] = useState(''); const [masked, setMasked] = useState(''); const [busy, setBusy] = useState(false);
  const id = useId();
  const label = ({ minimax_group_id: 'Group ID', volcengine_app_id: 'App ID', volcengine_access_key: 'Access Key' } as Record<string, string>)[secretKey] ?? 'API Key';
  useEffect(() => { let live = true; void rpc.call<any>('secrets.get', {}).then((all) => { if (live) setMasked(all[secretKey]?.find((v: any) => v.active)?.masked ?? ''); }).catch(() => {}); return () => { live = false; }; }, [secretKey]);
  const save = async (clear = false) => { setBusy(true); try { const result = await rpc.call<{ masked: string }>('secrets.set', { key: secretKey, value: clear ? '' : value }); setMasked(result.masked); setValue(''); onSaved(); pushToast(clear ? '密钥已清除' : '密钥已保存', 'success'); } finally { setBusy(false); } };
  return <Field><FieldLabel htmlFor={id}>{label}</FieldLabel><Input id={id} type="password" autoComplete="new-password" value={value} placeholder={masked ? `已保存 ${masked}` : '输入密钥'} onChange={(e) => setValue(e.target.value)} />
    <div className="flex gap-2"><Button variant="outline" size="sm" disabled={busy || !value.trim()} onClick={() => void save().catch(() => {})}>{busy ? <Spinner /> : <Save data-icon="inline-start" />}保存密钥</Button>{masked && <Button variant="ghost" size="sm" disabled={busy} onClick={() => void save(true).catch(() => {})}><Trash2 data-icon="inline-start" />清除</Button>}</div>
  </Field>;
}
function fileData(file: File): Promise<string> { return new Promise((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = reject; reader.readAsDataURL(file); }); }
function VoiceUpload({ config, onAdded }: { config: any; onAdded: () => void }) {
  const [name, setName] = useState(''); const [files, setFiles] = useState<File[]>([]); const [busy, setBusy] = useState(false);
  return <FieldSet><FieldLegend>上传 ElevenLabs 音色样本</FieldLegend><Field><FieldLabel htmlFor="tts-clone-name">新音色名称</FieldLabel><Input id="tts-clone-name" value={name} onChange={(e) => setName(e.target.value)} /></Field>
    <Field><FieldLabel htmlFor="tts-clone-files">音频样本</FieldLabel><Input id="tts-clone-files" type="file" multiple accept="audio/*" onChange={(e) => setFiles(Array.from(e.target.files ?? []))} /></Field>
    <Button variant="outline" size="sm" disabled={busy || !name.trim() || !files.length} onClick={() => {
      if (files.length > 10 || files.reduce((sum, f) => sum + f.size, 0) > 8 * 1024 * 1024) { pushToast('最多 10 个样本，总大小不超过 8 MB', 'error'); return; }
      setBusy(true); void Promise.all(files.map(fileData)).then((data) => ttsRequest('voices/add', { provider: 'ElevenLabs', settings: config, name, files: data })).then(() => { pushToast('音色已添加', 'success'); onAdded(); }).catch((e) => pushToast(e.message, 'error')).finally(() => setBusy(false));
    }}>{busy ? <Spinner /> : null}上传并创建音色</Button>
  </FieldSet>;
}
