import { KokoroTTS } from 'kokoro-js';
import { pipeline, env, RawAudio } from '@huggingface/transformers';

// Large inference dependencies are bundled only in this on-demand worker.
env.allowLocalModels = false;
let kokoro: KokoroTTS | null = null;
let speech: any = null;
let identity = '';

self.onmessage = async (event: MessageEvent) => {
  const { id, provider, config, text, voice, speaker } = event.data;
  try {
    const key = JSON.stringify([provider, config.modelId, config.model, config.dtype, config.device]);
    if (identity !== key) {
      self.postMessage({ id, status: '正在加载语音模型…' });
      kokoro = null;
      if (speech) await speech.dispose();
      speech = null;
      const progress_callback = (progress: any) => {
        if (progress.status === 'progress' && Number.isFinite(progress.progress)) {
          self.postMessage({ id, status: `下载模型文件 ${Math.round(progress.progress)}%` });
        }
      };
      if (provider === 'Kokoro') {
        kokoro = await KokoroTTS.from_pretrained(config.modelId, { dtype: config.dtype, device: config.device, progress_callback });
      } else {
        speech = await pipeline('text-to-speech', config.model || 'Xenova/speecht5_tts', { device: 'wasm', progress_callback });
      }
      identity = key;
    }
    self.postMessage({ id, status: '正在合成语音…' });
    let blob: Blob;
    if (provider === 'Kokoro') {
      const audio = await kokoro!.generate(text, { voice, speed: Number(config.speakingRate) || 1 });
      blob = audio.toBlob();
    } else {
      const encoded = String(speaker ?? '').split(',').pop()!;
      const binary = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
      if (binary.byteLength !== 512 * 4) throw new Error('SpeechT5 音色文件应为 512 维 Float32（2048 字节）');
      const result = await speech(text, { speaker_embeddings: new Float32Array(binary.buffer) });
      blob = new RawAudio(result.audio, result.sampling_rate).toBlob();
    }
    self.postMessage({ id, blob });
  } catch (error) {
    identity = '';
    self.postMessage({ id, error: error instanceof Error ? error.message : String(error) });
  }
};
