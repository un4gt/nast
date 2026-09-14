import { create } from 'zustand';
import { rpc } from './rpc';
import { pushToast } from './toasts';

export interface CharacterSummary {
  avatar: string;
  name: string;
  description: string;
  tags: string[];
  fav: boolean;
  chat: string | null;
  error?: string;
}

export interface ChatMessage {
  name: string;
  is_user: boolean;
  is_system: boolean;
  send_date: string;
  mes: string;
  swipes?: string[];
  swipe_id?: number;
  swipe_info?: unknown[];
  extra?: Record<string, unknown>;
}

export interface Settings {
  oai_settings?: {
    chat_completion_source?: string;
    openai_model?: string;
    openai_max_context?: number;
    openai_max_tokens?: number;
    temperature?: number;
    top_p?: number;
    frequency_penalty?: number;
    presence_penalty?: number;
    stream_openai?: boolean;
    continue_prefill?: boolean;
    send_if_empty?: string;
    squash_system_messages?: boolean;
    [k: string]: unknown;
  };
  world_info?: Record<string, unknown>;
  [k: string]: unknown;
}

interface AppState {
  connected: boolean;
  characters: CharacterSummary[];
  activeAvatar: string | null;
  activeChatName: string | null;
  chatList: string[];
  messages: ChatMessage[];
  streamingText: string | null;
  generating: boolean;
  settings: Settings | null;
  setConnected: (v: boolean) => void;
  loadAll: () => Promise<void>;
  selectCharacter: (avatar: string) => Promise<void>;
  importFile: (file: File) => Promise<void>;
  deleteCharacter: (avatar: string) => Promise<void>;
  send: (text: string) => Promise<void>;
  swipe: (direction: 'left' | 'right') => Promise<void>;
  regenerate: () => Promise<void>;
  impersonate: () => Promise<string | undefined>;
  continueGen: () => Promise<void>;
  stopGeneration: () => Promise<void>;
  appendStreamToken: (t: string) => void;
  reloadChat: () => Promise<void>;
  saveSettings: (s: Settings) => Promise<void>;
}

export const useStore = create<AppState>((set, get) => ({
  connected: false,
  characters: [],
  activeAvatar: null,
  activeChatName: null,
  chatList: [],
  messages: [],
  streamingText: null,
  generating: false,
  settings: null,

  setConnected: (v) => set({ connected: v }),

  loadAll: async () => {
    const [characters, settings] = await Promise.all([
      rpc.call<CharacterSummary[]>('characters.all', {}),
      rpc.call<Settings>('settings.get', {}),
    ]);
    set({ characters, settings });
  },

  selectCharacter: async (avatar) => {
    const chatList = await rpc.call<string[]>('characters.chats', { avatar });
    const activeChatName = chatList.length ? chatList[chatList.length - 1] : null;
    let messages = [];
    if (activeChatName) {
      const raw = await rpc.call<any[]>('chats.get', { avatar, file_name: activeChatName });
      messages = raw.slice(1);
    }
    set({ activeAvatar: avatar, chatList, activeChatName, messages, streamingText: null });
  },

  importFile: async (file) => {
    const buf = await file.arrayBuffer();
    const bin = Array.from(new Uint8Array(buf), (b) => String.fromCharCode(b)).join('');
    const data_base64 = btoa(bin);
    try {
      await rpc.call('characters.import', { data_base64 });
      await get().loadAll();
      pushToast('已导入 ' + file.name, 'success');
    } catch (e) {
      pushToast('导入失败 ' + file.name + ': ' + (e instanceof Error ? e.message : String(e)), 'error');
      throw e;
    }
  },

  deleteCharacter: async (avatar) => {
    await rpc.call('characters.delete', { avatar });
    if (get().activeAvatar === avatar) {
      set({ activeAvatar: null, activeChatName: null, messages: [] });
    }
    await get().loadAll();
  },

  reloadChat: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    const raw = await rpc.call<any[]>('chats.get', { avatar: activeAvatar, file_name: activeChatName });
    set({ messages: raw.slice(1) });
  },

  send: async (text) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return;
    set({ generating: true, streamingText: '' });
    try {
      await rpc.call('generate.run', {
        avatar: activeAvatar,
        chat_file: activeChatName,
        type: 'normal',
        user_message: text,
      });
      await get().reloadChat();
    } finally {
      set({ generating: false, streamingText: null });
    }
  },

  swipe: async (direction) => {
    const { activeAvatar, activeChatName, generating } = get();
    if (!activeAvatar || !activeChatName) return;
    if (!generating) {
      const last = get().messages[get().messages.length - 1];
      const total = last?.swipes?.length ?? 0;
      if (total > 1) {
        await rpc.call('chats.swipe', { avatar: activeAvatar, file_name: activeChatName, direction });
        await get().reloadChat();
        return;
      }
    }
    set({ generating: true, streamingText: '' });
    try {
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'swipe' });
      await get().reloadChat();
    } finally {
      set({ generating: false, streamingText: null });
    }
  },

  regenerate: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return;
    set({ generating: true, streamingText: '' });
    try {
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'regenerate' });
      await get().reloadChat();
    } finally {
      set({ generating: false, streamingText: null });
    }
  },

  impersonate: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return;
    set({ generating: true });
    try {
      const r = await rpc.call<{ text: string; saved: boolean }>('generate.run', {
        avatar: activeAvatar,
        chat_file: activeChatName,
        type: 'impersonate',
      });
      return r.text;
    } finally {
      set({ generating: false });
    }
  },

  continueGen: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return;
    set({ generating: true, streamingText: '' });
    try {
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'continue' });
      await get().reloadChat();
    } finally {
      set({ generating: false, streamingText: null });
    }
  },

  stopGeneration: async () => {
    await rpc.call('generate.stop', {});
    set({ generating: false });
  },

  appendStreamToken: (t) => set((s) => ({ streamingText: (s.streamingText ?? '') + t })),

  saveSettings: async (s) => {
    await rpc.call('settings.save', { settings: s });
    set({ settings: s });
  },
}));
