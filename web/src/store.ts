import { create } from 'zustand';
import { rpc } from './rpc';

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

interface AppState {
  connected: boolean;
  characters: CharacterSummary[];
  activeAvatar: string | null;
  activeChatName: string | null;
  chatList: string[];
  messages: ChatMessage[];
  streamingText: string | null; // 流式中的增量文本
  generating: boolean;
  setConnected: (v: boolean) => void;
  loadCharacters: () => Promise<void>;
  selectCharacter: (avatar: string) => Promise<void>;
  importFile: (file: File) => Promise<void>;
  send: (text: string) => Promise<void>;
  swipe: () => Promise<void>;
  regenerate: () => Promise<void>;
  impersonate: () => Promise<void>;
  appendStreamToken: (t: string) => void;
  clearStreaming: () => void;
  reloadChat: () => Promise<void>;
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

  setConnected: (v) => set({ connected: v }),

  loadCharacters: async () => {
    const characters = await rpc.call<CharacterSummary[]>('characters.all', {});
    set({ characters });
  },

  selectCharacter: async (avatar) => {
    const chatList = await rpc.call<string[]>('characters.chats', { avatar });
    const activeChatName = chatList.length ? chatList[chatList.length - 1] : null;
    let messages: ChatMessage[] = [];
    if (activeChatName) {
      const raw = await rpc.call<any[]>('chats.get', { avatar, file_name: activeChatName });
      messages = raw.slice(1) as ChatMessage[];
    }
    set({ activeAvatar: avatar, chatList, activeChatName, messages, streamingText: null });
  },

  importFile: async (file) => {
    const buf = await file.arrayBuffer();
    const bin = Array.from(new Uint8Array(buf), (b) => String.fromCharCode(b)).join('');
    const data_base64 = btoa(bin);
    await rpc.call('characters.import', { data_base64 });
    await get().loadCharacters();
  },

  reloadChat: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    const raw = await rpc.call<any[]>('chats.get', { avatar: activeAvatar, file_name: activeChatName });
    set({ messages: raw.slice(1) as ChatMessage[] });
  },

  send: async (text) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return;
    set({ generating: true, streamingText: '', user_message_pending: text } as any);
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
      delete (get() as any).user_message_pending;
    }
  },

  swipe: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return;
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

  appendStreamToken: (t) =>
    set((s) => ({ streamingText: (s.streamingText ?? '') + t })),

  clearStreaming: () => set({ streamingText: null }),
}));
