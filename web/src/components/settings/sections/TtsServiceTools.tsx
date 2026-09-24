import { useEffect, useId, useRef, useState } from 'react';
import { RefreshCw, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Spinner } from '@/components/ui/spinner';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field';
import { ttsRequest } from '@/tts/api';
import { type ProviderConfig } from '@/tts/config';
import { ttsPlayer } from '@/tts/player';
import coquiModels from '../../../../../resources/coqui-models.json';

export function TtsServiceTools({ provider, config, onChange }: {
  provider: string; config: ProviderConfig; onChange: (config: ProviderConfig) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [server, setServer] = useState<any>(null);
  const [model, setModel] = useState('');
  const [localModels, setLocalModels] = useState<string[]>([]);
  const [modelState, setModelState] = useState('');
  const [name, setName] = useState('');
  const [language, setLanguage] = useState('');
  const [speaker, setSpeaker] = useState('');
  const controller = useRef<AbortController>();
  const id = useId();
  useEffect(() => () => controller.current?.abort(), []);
  const request = async (action: string, extra: Record<string, unknown> = {}) => {
    const response = await ttsRequest('manage', { provider, settings: config, action, model_id: model, ...extra }, controller.current?.signal);
    return response.json();
  };
  const run = (task: () => Promise<void>) => {
    if (busy) return;
    controller.current = new AbortController();
    setBusy(true); setError(''); setStatus('正在处理…');
    void task().catch((e) => { if (!controller.current?.signal.aborted) { setError(e.message); setStatus(''); } }).finally(() => setBusy(false));
  };
  const readServer = async () => {
    const next = await request('status');
    setServer(next); setModel(next.current_model_loaded ?? ''); setStatus('服务状态已更新');
  };
  const meta = coquiModels.find((m) => m.id === model) as { languages?: string[]; speakers?: string[] } | undefined;
  const modelList = provider === 'AllTalk'
    ? (server?.models_available ?? []).map((m: any) => typeof m === 'string' ? m : m.name)
    : [...coquiModels.map((m) => m.id), ...localModels];
  return <FieldGroup>
    <FieldDescription>以下服务操作立即执行。音色配置加入草稿后仍需保存更改。</FieldDescription>
    {provider === 'AllTalk' ? <>
      <Button variant="outline" size="sm" className="self-start" disabled={busy} onClick={() => run(readServer)}><RefreshCw data-icon="inline-start" />读取服务状态</Button>
      {server && <FieldDescription>当前引擎：{server.current_engine_loaded || '未报告'} · 模型：{server.current_model_loaded || '未加载'}</FieldDescription>}
    </> : <Button variant="outline" size="sm" className="self-start" disabled={busy} onClick={() => run(async () => {
      const data = await request('local-models');
      setLocalModels((data.models_list ?? []).map((m: string) => m.startsWith('local/') ? m : `local/${m}`)); setStatus('本地模型列表已更新');
    })}><RefreshCw data-icon="inline-start" />获取本地模型</Button>}
    <Field><FieldLabel htmlFor={`${id}-model`}>服务模型 ID</FieldLabel>
      <Input id={`${id}-model`} list={`${id}-models`} value={model} onChange={(e) => { setModel(e.target.value); setModelState(''); setLanguage(''); setSpeaker(''); }} placeholder={provider === 'AllTalk' ? '读取服务状态后选择模型' : 'tts_models/en/ljspeech/vits 或 local/模型名'} />
      <datalist id={`${id}-models`}>{modelList.map((m: string) => <option key={m} value={m} />)}</datalist>
    </Field>
    {provider === 'AllTalk' ? <>
      <Button variant="outline" size="sm" className="self-start" disabled={busy || !model} onClick={() => run(async () => { ttsPlayer.cancel(); await request('reload'); await readServer(); })}>切换服务模型</Button>
      {server && [
        { action: 'deepspeed', key: 'deepspeed_enabled', label: 'DeepSpeed', capable: server.deepspeed_capable && server.deepspeed_available },
        { action: 'low-vram', key: 'lowvram_enabled', label: '低显存模式', capable: server.lowvram_capable },
      ].map((item) => <Field key={item.key} orientation="horizontal"><FieldContent><FieldLabel htmlFor={`${id}-${item.key}`}>{item.label}</FieldLabel></FieldContent>
        <Switch id={`${id}-${item.key}`} disabled={busy || !item.capable} checked={Boolean(server[item.key])} onCheckedChange={(value) => run(async () => { ttsPlayer.cancel(); await request(item.action, { value }); await readServer(); })} />
      </Field>)}
      {config.server_version === 'v2' && <Button variant="outline" size="sm" className="self-start" disabled={busy} onClick={() => run(async () => {
        const data = await request('rvc-voices'); setStatus(`可用 RVC 音色：${(data.rvcvoices ?? []).join('、') || '暂无'}`);
      })}>查询 RVC 音色</Button>}
    </> : <>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={busy || !model || model.startsWith('local/')} onClick={() => run(async () => {
          const data = await request('check-model'); setModelState(data.model_state); setStatus(({ installed: '模型已安装', absent: '模型尚未安装', corrupted: '模型文件不完整' } as Record<string, string>)[data.model_state] ?? '服务未报告安装状态');
        })}>检查安装状态</Button>
        <Button variant="outline" size="sm" disabled={busy || !['absent', 'corrupted'].includes(modelState)} onClick={() => run(async () => {
          const data = await request(modelState === 'corrupted' ? 'repair-model' : 'install-model');
          if (data.status === 'done') { setModelState('installed'); setStatus('模型安装完成'); }
          else if (data.status === 'downloading') setStatus('服务正在下载模型，可稍后检查安装状态');
          else throw new Error('服务未确认安装结果，请检查服务日志');
        })}>{modelState === 'corrupted' ? '修复模型' : '下载模型'}</Button>
      </div>
      <Field><FieldLabel htmlFor={`${id}-name`}>音色名称</FieldLabel><Input id={`${id}-name`} value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：故事旁白" /></Field>
      {model.includes('multilingual') && <Field><FieldLabel htmlFor={`${id}-language`}>语言索引</FieldLabel><Input id={`${id}-language`} type="number" min={0} step={1} value={language} onChange={(e) => setLanguage(e.target.value)} />
        <FieldDescription>{meta?.languages?.map((v, i) => `${i}: ${v}`).join(' · ') || '填写模型支持的语言序号，从 0 开始。'}</FieldDescription>
      </Field>}
      {!model.startsWith('local/') && <Field><FieldLabel htmlFor={`${id}-speaker`}>说话人索引（可选）</FieldLabel><Input id={`${id}-speaker`} type="number" min={0} step={1} list={`${id}-speakers`} value={speaker} onChange={(e) => setSpeaker(e.target.value)} />
        <datalist id={`${id}-speakers`}>{meta?.speakers?.map((v, i) => <option key={i} value={i}>{v}</option>)}</datalist>
      </Field>}
      <Button variant="outline" size="sm" className="self-start" disabled={!model || !name.trim() || (model.includes('multilingual') && language === '')} onClick={() => {
        const voiceName = name.trim();
        const modelLanguage = model.includes('multilingual') && language !== '' ? Number(language) : null;
        const modelSpeaker = !model.startsWith('local/') && speaker !== '' ? Number(speaker) : null;
        if ([modelLanguage, modelSpeaker].some((v) => v !== null && (!Number.isInteger(v) || v < 0))) { setError('语言和说话人索引应为非负整数'); return; }
        const encoded = model + (modelLanguage === null ? '' : `[${modelLanguage}]`) + (modelSpeaker === null ? '' : `[${modelSpeaker}]`);
        onChange({ ...config, customVoices: { ...config.customVoices, [voiceName]: encoded }, voiceMapDict: { ...config.voiceMapDict, [voiceName]: { model_type: model.startsWith('local/') ? 'local' : 'coqui-api', model_id: model, model_language: modelLanguage, model_speaker: modelSpeaker } } });
        setStatus('音色已加入草稿，可在默认音色或角色映射中选择');
      }}>保存音色到草稿</Button>
      {Object.entries(config.customVoices ?? {}).map(([voice, modelId]) => <Field key={voice} orientation="horizontal"><FieldContent><FieldLabel>{voice}</FieldLabel><FieldDescription className="break-all">{String(modelId)}</FieldDescription></FieldContent>
        <Button variant="ghost" size="icon" aria-label={`移除音色 ${voice}`} onClick={() => { const customVoices = { ...config.customVoices }, voiceMapDict = { ...config.voiceMapDict }; delete customVoices[voice]; delete voiceMapDict[voice]; onChange({ ...config, customVoices, voiceMapDict }); }}><Trash2 /></Button>
      </Field>)}
    </>}
    {busy && <Spinner />}
    {status && <FieldDescription role="status" className="break-words">{status}</FieldDescription>}
    {error && <Alert variant="destructive"><AlertTitle>服务操作未完成</AlertTitle><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>;
}
