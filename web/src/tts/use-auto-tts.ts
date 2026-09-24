import { useEffect } from 'react';
import { rpc } from '../rpc';
import { useStore, type ChatMessage } from '../store';
import { readTtsSettings } from './config';
import { ttsPlayer } from './player';
import { streamingBoundary } from './text';

function target() {
  const state = useStore.getState();
  const group = state.groups.find((g) => g.id === state.activeGroupId);
  return group
    ? { key: `group:${group.id}:${group.chat_id}`, method: 'groups.get_chat', params: { chat_id: group.chat_id } }
    : { key: `${state.activeAvatar}:${state.activeChatName}`, method: 'chats.get', params: { avatar: state.activeAvatar, file_name: state.activeChatName } };
}

export function useAutoTts() {
  useEffect(() => {
    let key = target().key;
    let generationKey = '';
    let type = '';
    let name = '';
    let streamed = '';
    let spoken = '';
    let prefix = '';
    let suppressed = false;
    let revision = 0;
    let reads = Promise.resolve();
    const seen = new Set<string>();
    const reset = () => { revision++; streamed = ''; spoken = ''; prefix = ''; generationKey = ''; seen.clear(); ttsPlayer.stop(); };
    const unstore = useStore.subscribe((state, previous) => {
      const nextKey = target().key;
      const next = readTtsSettings(state.settings), prev = readTtsSettings(previous.settings);
      if (nextKey !== key || !state.connected || (prev.enabled && !next.enabled) || next.currentProvider !== prev.currentProvider) {
        const changedChat = nextKey !== key;
        key = nextKey; reset();
        // ST narrates the sole greeting when a new one-message chat is rendered.
        if (changedChat && state.connected && !state.generating && state.messages.length === 1) {
          ttsPlayer.speakMessage(state.messages[0], 0, { append: true });
        }
      }
      if (state.settings !== previous.settings) ttsPlayer.clearVoiceCache();
    });
    const onStarted = (data: { type?: string }) => {
      const state = useStore.getState();
      if (!state.generating) return;
      type = data.type ?? 'normal';
      if (['quiet', 'impersonate'].includes(type)) { generationKey = ''; return; }
      generationKey = key;
      streamed = ''; spoken = '';
      prefix = type === 'continue' ? state.messages[state.messages.length - 1]?.mes ?? '' : '';
      name = state.characters.find((c) => c.avatar === state.activeAvatar)?.name ?? name;
      suppressed = false;
      if (type === 'swipe' || type === 'regenerate') ttsPlayer.stop();
    };
    const onMember = (data: string | { name?: string }) => {
      if (!useStore.getState().generating) return;
      name = typeof data === 'string' ? data : data.name ?? '';
      generationKey = key; type = 'normal'; streamed = ''; spoken = ''; prefix = '';
    };
    const onToken = (data: { text: string }) => {
      if (!generationKey || generationKey !== key || suppressed) return;
      streamed += data.text;
      const tts = readTtsSettings(useStore.getState().settings);
      if (!tts.enabled || !tts.auto_generation || !tts.periodic_auto_generation || tts.narrate_translated_only) return;
      const pending = streamed.slice(spoken.length);
      const boundary = streamingBoundary(pending, tts);
      if (!boundary) return;
      const text = pending.slice(0, boundary);
      spoken += text;
      ttsPlayer.speakMessage({ name, mes: text, is_user: false, is_system: false, send_date: '' }, null, { append: true });
    };
    const onMessage = (index: number) => {
      if (!generationKey || generationKey !== key || suppressed) return;
      const currentTarget = target();
      const version = revision;
      const consumed = spoken;
      const previousText = prefix;
      reads = reads.then(async () => {
        const raw = await rpc.call<ChatMessage[]>(currentTarget.method, currentTarget.params);
        if (currentTarget.key !== target().key || version !== revision || suppressed) return;
        const message = raw[index]; // ST events include the JSONL header at index 0.
        if (!message || index < 1) return;
        const signature = JSON.stringify([currentTarget.key, index, message.mes, message.swipe_id]);
        if (seen.has(signature)) return;
        seen.add(signature);
        const tts = readTtsSettings(useStore.getState().settings);
        let mes = message.mes;
        if (!message.is_user && !tts.narrate_translated_only) {
          if (previousText && mes.startsWith(previousText)) mes = mes.slice(previousText.length);
          if (consumed && mes.startsWith(consumed)) mes = mes.slice(consumed.length);
          // If final post-processing rewrites the streamed prefix, avoid speaking it twice.
          else if (consumed) mes = '';
        }
        if (mes) ttsPlayer.speakMessage({ ...message, mes }, index - 1, { append: true });
      }).catch(() => { /* RPC reports errors; a failed read must not poison later messages. */ });
    };
    const stopUser = () => { suppressed = true; revision++; };
    window.addEventListener('nast:tts-stop', stopUser);
    const listeners = [
      rpc.on('generation_started', onStarted), rpc.on('group_member_drafted', onMember),
      rpc.on('stream_token_received', onToken), rpc.on('message_received', onMessage),
      rpc.on('message_sent', onMessage),
      rpc.on('message_swiped', () => { revision++; ttsPlayer.stop(); }),
      rpc.on('message_deleted', () => { revision++; ttsPlayer.stop(); }),
      rpc.on('message_updated', () => { revision++; ttsPlayer.stop(); }),
      rpc.on('generation_stopped', () => { generationKey = ''; }),
    ];
    return () => { listeners.forEach((off) => off()); unstore(); window.removeEventListener('nast:tts-stop', stopUser); ttsPlayer.stop(); };
  }, []);
}
