import { create } from 'zustand';
import { useStore, type ChatMessage } from '../store';
import { pushToast } from '../toasts';
import { fetchVoices, ttsRequest } from './api';
import { DEFAULT_VOICE, DISABLED_VOICE, SYSTEM_VOICE, localVoices, providerConfig, providerInfo, readTtsSettings, resolveVoice, type ProviderConfig, type TtsSettings, type Voice } from './config';
import { prepareText, splitLongText } from './text';

interface PlaybackState {
  status: 'idle' | 'loading' | 'playing' | 'paused' | 'blocked' | 'error';
  label: string;
  character: string;
  messageId: number | null;
  queued: number;
  error: string;
}
export const useTtsPlayback = create<PlaybackState>(() => ({ status: 'idle', label: '', character: '', messageId: null, queued: 0, error: '' }));
interface Job { text: string; voice: string; name: string; id: number | null; tts: TtsSettings; config: ProviderConfig }

class TtsPlayer {
  private queue: Job[] = [];
  private epoch = 0;
  private running = false;
  private controller: AbortController | null = null;
  private audio: HTMLAudioElement | null = null;
  private objectUrl = '';
  private worker: Worker | null = null;
  private voiceCache = new Map<string, Voice[]>();
  private paused = false;
  private resumeWait: (() => void) | null = null;
  private sequence = 0;

  cancel = () => {
    this.stop();
    window.dispatchEvent(new Event('nast:tts-stop'));
  };

  stop = () => {
    this.epoch++;
    this.queue = [];
    this.running = false;
    this.paused = false;
    this.controller?.abort();
    this.controller = null;
    this.audio?.pause();
    if (this.audio) this.audio.removeAttribute('src');
    this.audio = null;
    if (this.objectUrl) URL.revokeObjectURL(this.objectUrl);
    this.objectUrl = '';
    if ('speechSynthesis' in window) speechSynthesis.cancel();
    // Terminating also cancels an in-flight model download / inference.
    this.worker?.terminate();
    this.worker = null;
    this.resumeWait?.();
    this.resumeWait = null;
    useTtsPlayback.setState({ status: 'idle', label: '', character: '', messageId: null, queued: 0, error: '' });
  };

  togglePause = () => {
    const status = useTtsPlayback.getState().status;
    if (status === 'idle' || status === 'error') return;
    this.paused = !this.paused;
    if (this.paused) {
      this.audio?.pause();
      if ('speechSynthesis' in window) speechSynthesis.pause();
      useTtsPlayback.setState({ status: 'paused', label: '已暂停' });
    } else {
      if ('speechSynthesis' in window) speechSynthesis.resume();
      this.resumeWait?.();
      this.resumeWait = null;
      const speaking = Boolean(this.audio) || ('speechSynthesis' in window && speechSynthesis.speaking);
      useTtsPlayback.setState({ status: speaking ? 'playing' : 'loading', label: speaking ? '正在朗读' : '准备语音…' });
      if (this.audio) void this.audio.play().catch(() => this.blocked());
    }
  };
  private blocked() {
    this.paused = true;
    useTtsPlayback.setState({ status: 'blocked', label: '点击继续播放' });
  }
  private async waitForResume(signal: AbortSignal) {
    if (!this.paused) return;
    await new Promise<void>((resolve) => {
      const done = () => { signal.removeEventListener('abort', done); resolve(); };
      this.resumeWait = done;
      signal.addEventListener('abort', done, { once: true });
    });
    signal.throwIfAborted();
  }
  private async voices(provider: string, config: ProviderConfig, signal: AbortSignal): Promise<Voice[]> {
    const key = JSON.stringify([provider, config]);
    const cached = this.voiceCache.get(key);
    if (cached) return cached;
    const list = await fetchVoices(provider, config, signal);
    if (this.voiceCache.size > 12) this.voiceCache.clear();
    this.voiceCache.set(key, list);
    return list;
  }
  clearVoiceCache = () => this.voiceCache.clear();

  speakMessage = (message: ChatMessage, id: number | null, options: { manual?: boolean; append?: boolean; settings?: TtsSettings; voice?: string } = {}) => {
    const store = useStore.getState();
    const tts = structuredClone(options.settings ?? readTtsSettings(store.settings));
    if (!tts.enabled) { if (options.manual) pushToast('请在设置 → 语音朗读中启用 TTS', 'info'); return; }
    if (!options.manual && (!tts.auto_generation || message.is_system || (message.is_user && !tts.narrate_user))) return;
    if (!options.manual && tts.narrate_translated_only && !message.extra?.display_text) return;
    if (!options.append) this.stop();
    try {
      const config = providerConfig(tts);
      const provider = providerInfo(tts.currentProvider);
      const raw = tts.narrate_translated_only ? String(message.extra?.display_text || message.mes) : message.mes;
      const user = String(store.settings?.name1 || 'User');
      const allowName = Boolean((store.settings?.power_user as any)?.allow_name2_display);
      const segments = prepareText(raw, tts, message.name, user, allowName);
      let added = 0;
      for (const segment of segments) {
        const voice = options.voice ?? resolveVoice(tts, config, message.name, segment.type);
        if (voice === DISABLED_VOICE) continue;
        if (!voice) throw new Error(`请先为 ${message.name} 设置音色，或选择默认音色`);
        for (const text of splitLongText(segment.text, provider.runtime === 'system' ? 200 : provider.maxLength ?? 2000)) {
          if (this.queue.length >= 500) throw new Error('朗读队列已满，请先播放或停止当前队列');
          this.queue.push({ text, voice, name: message.name, id, tts, config });
          added++;
        }
      }
      if (!added && options.manual) pushToast(segments.length ? '该角色的朗读已禁用' : '过滤后没有可朗读的文本', 'info');
      useTtsPlayback.setState({ queued: this.queue.length });
      void this.pump();
    } catch (error) {
      this.fail(error);
    }
  };

  speakText = (text: string, voiceName?: string) => {
    const store = useStore.getState();
    const name = voiceName || store.characters.find((c) => c.avatar === store.activeAvatar)?.name || DEFAULT_VOICE;
    this.speakMessage({ name, mes: text, is_system: false, is_user: false, send_date: '' }, null, { manual: true });
  };
  speakConversation = () => {
    this.stop();
    const settings = readTtsSettings(useStore.getState().settings);
    if (!settings.enabled) { pushToast('请先在设置中启用语音朗读', 'info'); return; }
    for (const [id, message] of useStore.getState().messages.entries()) {
      if (!message.is_system) this.speakMessage(message, id, { manual: true, append: true });
    }
  };

  private fail(error: unknown) {
    this.stop();
    const message = error instanceof Error ? error.message : String(error);
    useTtsPlayback.setState({ status: 'error', error: message, label: '朗读失败' });
    pushToast(message, 'error');
  }
  private async pump() {
    if (this.running || !this.queue.length) return;
    this.running = true;
    const epoch = this.epoch;
    try {
      while (this.queue.length && epoch === this.epoch) {
        const job = this.queue.shift()!;
        const controller = new AbortController();
        this.controller = controller;
        const signal = controller.signal;
        const provider = providerInfo(job.tts.currentProvider);
        useTtsPlayback.setState({ status: this.paused ? 'paused' : 'loading', label: '准备语音…', character: job.name === DEFAULT_VOICE ? '试听' : job.name, messageId: job.id, queued: this.queue.length, error: '' });
        const configured = localVoices(provider, job.config);
        let voice = configured.find((v) => v.name === job.voice || v.voice_id === job.voice);
        if (!voice) {
          const voices = await this.voices(provider.name, job.config, signal);
          voice = voices.find((v) => v.name === job.voice || v.voice_id === job.voice);
        }
        signal.throwIfAborted();
        // Manual IDs remain usable on providers with no voice listing endpoint.
        const id = voice?.voice_id ?? job.voice;
        await this.waitForResume(signal);
        if (provider.runtime === 'system') await this.systemAudio(job, id, signal);
        else {
          let blob: Blob;
          if (provider.runtime === 'browser') blob = await this.localAudio(job, id, voice?.data, signal);
          else {
            const response = await ttsRequest('synthesize', { provider: provider.name, settings: job.config, text: job.text, voice: id, character: job.name, characters: useStore.getState().characters.map((c) => c.name) }, signal);
            blob = await response.blob();
            if (!blob.type.startsWith('audio/')) throw new Error('未收到可播放的音频，请检查语音端点');
          }
          signal.throwIfAborted();
          await this.waitForResume(signal);
          await this.playBlob(blob, job, signal);
        }
        signal.throwIfAborted();
      }
      if (epoch === this.epoch) {
        this.running = false;
        this.controller = null;
        useTtsPlayback.setState({ status: 'idle', label: '', messageId: null, character: '', queued: 0 });
      }
    } catch (error) {
      if (epoch === this.epoch) this.fail(error);
    }
  }
  private systemAudio(job: Job, id: string, signal: AbortSignal): Promise<void> {
    return new Promise((resolve, reject) => {
      if (!('speechSynthesis' in window)) { reject(new Error('此浏览器不支持系统朗读')); return; }
      const utterance = new SpeechSynthesisUtterance(job.text);
      if (id !== SYSTEM_VOICE) {
        utterance.voice = speechSynthesis.getVoices().find((v) => v.voiceURI === id || v.name === id) ?? null;
        if (!utterance.voice) { reject(new Error('所选系统音色已不可用，请刷新音色列表')); return; }
      }
      utterance.rate = Math.min(10, Math.max(0.1, Number(job.config.rate) || 1));
      utterance.pitch = Math.min(2, Math.max(0, Number(job.config.pitch) || 0));
      const abort = () => { speechSynthesis.cancel(); done(); };
      const done = (error?: Error) => {
        signal.removeEventListener('abort', abort);
        utterance.onstart = null; utterance.onend = null; utterance.onerror = null;
        error ? reject(error) : resolve();
      };
      utterance.onstart = () => useTtsPlayback.setState({ status: 'playing', label: '正在朗读' });
      utterance.onend = () => done();
      utterance.onerror = (event) => {
        if (event.error === 'not-allowed') {
          this.blocked();
          this.resumeWait = () => speechSynthesis.speak(utterance);
        } else done(new Error(`系统朗读失败：${event.error}`));
      };
      signal.addEventListener('abort', abort, { once: true });
      speechSynthesis.speak(utterance);
    });
  }
  private playBlob(blob: Blob, job: Job, signal: AbortSignal): Promise<void> {
    return new Promise((resolve, reject) => {
      const audio = new Audio();
      this.audio = audio;
      const url = URL.createObjectURL(blob);
      this.objectUrl = url;
      audio.src = url;
      audio.playbackRate = Math.min(3, Math.max(0.25, Number(job.tts.playback_rate) || 1));
      audio.volume = Math.min(1, Math.max(0, Number(job.config.volume ?? 1)));
      const abort = () => done();
      const done = (error?: Error) => {
        signal.removeEventListener('abort', abort);
        audio.onended = null; audio.onerror = null; audio.onplaying = null;
        audio.pause(); audio.removeAttribute('src');
        URL.revokeObjectURL(url);
        if (this.audio === audio) { this.audio = null; this.objectUrl = ''; }
        error ? reject(error) : resolve();
      };
      signal.addEventListener('abort', abort, { once: true });
      audio.onplaying = () => useTtsPlayback.setState({ status: 'playing', label: '正在朗读' });
      audio.onended = () => done();
      audio.onerror = () => done(new Error('音频无法解码，请检查服务商的输出格式'));
      void audio.play().catch((error) => {
        if (signal.aborted) done();
        else if (error.name === 'NotAllowedError') this.blocked();
        else done(error);
      });
    });
  }
  private localAudio(job: Job, voice: string, speaker: string | undefined, signal: AbortSignal): Promise<Blob> {
    if (!this.worker) this.worker = new Worker(new URL('./local-worker.ts', import.meta.url), { type: 'module' });
    const worker = this.worker;
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const cleanup = () => { worker.removeEventListener('message', message); worker.removeEventListener('error', error); signal.removeEventListener('abort', abort); };
      const abort = () => { cleanup(); reject(new DOMException('Aborted', 'AbortError')); };
      const error = () => { cleanup(); reject(new Error('本地语音模型加载失败，请检查网络或选择其他服务商')); };
      const message = (event: MessageEvent) => {
        if (event.data.id !== id) return;
        if (event.data.status) {
          if (!this.paused) useTtsPlayback.setState({ label: event.data.status });
          return;
        }
        cleanup();
        event.data.error ? reject(new Error(event.data.error)) : resolve(event.data.blob);
      };
      worker.addEventListener('message', message);
      worker.addEventListener('error', error);
      signal.addEventListener('abort', abort, { once: true });
      worker.postMessage({ id, provider: job.tts.currentProvider, config: job.config, text: job.text, voice, speaker });
    });
  }
}
export const ttsPlayer = new TtsPlayer();
