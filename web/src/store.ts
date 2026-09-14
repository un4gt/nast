import { create } from 'zustand';
import { rpc } from './rpc';
import { pushToast } from './toasts';

export interface CharacterSummary {
  avatar: string;
  /** /thumbnail?file=<avatar> 头像 URL */
  avatarUrl: string;
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
  deleteMessage: (index: number) => Promise<void>;
  exportChat: () => Promise<void>;
  send: (text: string) => Promise<void>;
  swipe: (direction: 'left' | 'right') => Promise<void>;
  regenerate: () => Promise<void>;
  impersonate: () => Promise<string | undefined>;
  continueGen: () => Promise<void>;
  stopGeneration: () => Promise<void>;
  appendStreamToken: (t: string) => void;
  reloadChat: () => Promise<void>;
  saveSettings: (s: Settings) => Promise<void>;
  openChat: (avatar: string, file: string) => Promise<void>;
  newChat: (avatar: string, greetingIndex?: number) => Promise<void>;
  personaDraft: string;
  setPersonaDraft: (v: string) => void;
  anDraft: { prompt: string; depth: number };
  setAnDraft: (v: { prompt: string; depth: number }) => void;
}

/** generate.run 的 persona/AN 附加参数（第一期：前端草稿，localStorage 持久化）。 */
function buildGenExtras(get: () => AppState): Record<string, unknown> {
  const { personaDraft, anDraft } = get();
  const extras: Record<string, unknown> = {
    persona_description: personaDraft,
    persona_position_in_prompt: true,
  };
  if (anDraft.prompt.trim()) {
    extras.in_chat_injections = [
      { content: anDraft.prompt, depth: anDraft.depth, role: 0, injection_order: 100 },
    ];
  }
  return extras;
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
  personaDraft: '',
  anDraft: { prompt: '', depth: 4 },

  setPersonaDraft: (v) => set({ personaDraft: v }),
  setAnDraft: (v) => set({ anDraft: v }),

  openChat: async (avatar, file) => {
    const chatList = await rpc.call<string[]>('characters.chats', { avatar });
    const raw = await rpc.call<any[]>('chats.get', { avatar, file_name: file });
    set({
      activeAvatar: avatar,
      chatList,
      activeChatName: file,
      messages: raw.slice(1),
      streamingText: null,
    });
  },

  newChat: async (avatar, greetingIndex = -1) => {
    const r = await rpc.call<{ file_name: string }>('chats.new', {
      avatar,
      greeting_index: greetingIndex,
    });
    await get().openChat(avatar, r.file_name);
  },

  setConnected: (v) => set({ connected: v }),

  loadAll: async () => {
    const [characters, settings] = await Promise.all([
      rpc.call<CharacterSummary[]>('characters.all', {}),
      rpc.call<Settings>('settings.get', {}),
    ]);
    const withAvatars = characters.map((c) => ({
      ...c,
      avatarUrl: `/thumbnail?file=${encodeURIComponent(c.avatar)}`,
    }));
    set({ characters: withAvatars, settings });
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

  deleteMessage: async (index) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    await rpc.call('chats.delete_message', { avatar: activeAvatar, file_name: activeChatName, index });
    await get().reloadChat();
  },

  exportChat: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    const r = await rpc.call<{ content: string }>('chats.export', {
      avatar: activeAvatar,
      file_name: activeChatName,
    });
    const blob = new Blob([r.content], { type: 'application/jsonl' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = activeChatName;
    a.click();
    URL.revokeObjectURL(url);
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
        ...buildGenExtras(get),
      });
      await get().reloadChat();
    } finally {
      set({ generating: false, streamingText: null });
    }
  },

  swipe: async (direction) => {
    const { activeAvatar, activeChatName, generating } = get();
    if (!activeAvatar || !activeChatName || generating) return;
    const last = get().messages[get().messages.length - 1];
    const total = last?.swipes?.length ?? 0;
    const idx = last?.swipe_id ?? 0;
    // ST 语义：left 总是导航；right 在末位时生成新 swipe，否则导航
    const isNav =
      direction === 'left' ? idx > 0 : idx < total - 1;
    if (isNav && total > 1) {
      await rpc.call('chats.swipe', { avatar: activeAvatar, file_name: activeChatName, direction });
      await get().reloadChat();
      return;
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
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'regenerate', ...buildGenExtras(get) });
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
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'continue', ...buildGenExtras(get) });
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
