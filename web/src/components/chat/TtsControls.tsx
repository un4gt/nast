import { Headphones, Pause, Play, Square, Volume2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { useStore } from '@/store';
import { readTtsSettings } from '@/tts/config';
import { ttsPlayer, useTtsPlayback } from '@/tts/player';

export function TtsControls({ onSettings }: { onSettings: () => void }) {
  const tts = readTtsSettings(useStore((s) => s.settings));
  const messages = useStore((s) => s.messages);
  const state = useTtsPlayback();
  const active = !['idle', 'error'].includes(state.status);
  if (!tts.enabled && !active) return null;
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b bg-muted/30 px-4 py-2 sm:px-6" aria-label="语音播放器">
      <Headphones className="size-4 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1" role="status" aria-live="polite">
        <p className="truncate text-xs">
          {active ? `${state.character} · ${state.label}` : state.status === 'error' ? state.error : '语音朗读已启用'}
        </p>
        {state.queued > 0 && <p className="text-[11px] text-muted-foreground">还有 {state.queued} 段待播放</p>}
      </div>
      {active ? (
        <>
          <Button variant="outline" size="sm" onClick={ttsPlayer.togglePause}>
            {state.status === 'paused' || state.status === 'blocked' ? <Play data-icon="inline-start" /> : state.status === 'loading' ? <Spinner /> : <Pause data-icon="inline-start" />}
            {state.status === 'paused' || state.status === 'blocked' ? '继续播放' : '暂停'}
          </Button>
          <Button variant="ghost" size="icon" className="size-8" aria-label="停止朗读" onClick={ttsPlayer.cancel}><Square /></Button>
        </>
      ) : (
        <Button variant="ghost" size="sm" disabled={!messages.length} onClick={ttsPlayer.speakConversation}>
          <Volume2 data-icon="inline-start" />朗读全部
        </Button>
      )}
      <Button variant="ghost" size="sm" onClick={onSettings}>语音设置</Button>
    </div>
  );
}
