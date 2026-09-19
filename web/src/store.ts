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

export interface ChatMetadata {
  world?: string;
  persona?: string;
  note_prompt?: string;
  note_interval?: number;
  note_depth?: number;
  note_position?: number;
  note_role?: number;
  tainted?: boolean;
  [k: string]: unknown;
}

/** Author's Note（chats.set_note 参数形状；prompt 为 null = 清除） */
export interface NotePayload {
  prompt: string | null;
  interval?: number;
  depth?: number;
  position?: number;
  role?: number;
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

export interface Group {
  id: string;
  name: string;
  members: string[];
  allow_self_responses: boolean;
  activation_strategy: number;
  generation_mode: number;
  disabled_members: string[];
  fav: boolean;
  chat_id: string;
  chats: string[];
  auto_mode_delay: number;
  [k: string]: unknown;
}

interface AppState {
  connected: boolean;
  characters: CharacterSummary[];
  groups: Group[];
  activeGroupId: string | null;
  activeAvatar: string | null;
  activeChatName: string | null;
  chatList: string[];
  messages: ChatMessage[];
  chatMetadata: ChatMetadata | null;
  streamingText: string | null;
  streamingReasoning: string | null;
  generating: boolean;
  settings: Settings | null;
  setConnected: (v: boolean) => void;
  loadAll: () => Promise<void>;
  selectCharacter: (avatar: string) => Promise<void>;
  openGroup: (groupId: string) => Promise<void>;
  createGroup: (name: string, members: string[]) => Promise<void>;
  saveGroup: (group: Group) => Promise<void>;
  deleteGroup: (groupId: string) => Promise<void>;
  sendGroup: (text: string, member?: string) => Promise<boolean>;
  importFile: (file: File) => Promise<void>;
  deleteCharacter: (avatar: string) => Promise<void>;
  deleteMessage: (index: number) => Promise<void>;
  editMessage: (index: number, text: string) => Promise<void>;
  exportChat: () => Promise<void>;
  send: (text: string) => Promise<boolean>;
  swipe: (direction: 'left' | 'right') => Promise<boolean>;
  regenerate: () => Promise<boolean>;
  impersonate: () => Promise<string | undefined>;
  continueGen: () => Promise<boolean>;
  stopGeneration: () => Promise<void>;
  appendStreamToken: (t: string) => void;
  appendStreamReasoning: (t: string) => void;
  reloadChat: () => Promise<void>;
  saveSettings: (s: Settings) => Promise<void>;
  openChat: (avatar: string, file: string) => Promise<void>;
  newChat: (avatar: string, greetingIndex?: number) => Promise<void>;
  setNote: (note: NotePayload) => Promise<void>;
  setChatPersona: (persona: string | null) => Promise<void>;
}

function parseChat(raw: any[]): { messages: ChatMessage[]; metadata: ChatMetadata | null } {
  return {
    messages: raw.slice(1),
    metadata: raw[0]?.chat_metadata ?? null,
  };
}

export const useStore = create<AppState>((set, get) => ({
  connected: false,
  characters: [],
  groups: [],
  activeGroupId: null,
  activeAvatar: null,
  activeChatName: null,
  chatList: [],
  messages: [],
  chatMetadata: null,
  streamingText: null,
  streamingReasoning: null,
  generating: false,
  settings: null,

  openChat: async (avatar, file) => {
    const chatList = await rpc.call<string[]>('characters.chats', { avatar });
    const raw = await rpc.call<any[]>('chats.get', { avatar, file_name: file });
    const { messages, metadata } = parseChat(raw);
    set({
      activeGroupId: null,
      activeAvatar: avatar,
      chatList,
      activeChatName: file,
      messages,
      chatMetadata: metadata,
      streamingText: null,
      streamingReasoning: null,
    });
  },

  openGroup: async (groupId) => {
    const groups = await rpc.call<Group[]>('groups.all', {});
    const g = groups.find((x) => x.id === groupId);
    if (!g) return;
    const raw = await rpc.call<any[]>('groups.get_chat', { chat_id: g.chat_id });
    set({
      activeGroupId: groupId,
      activeAvatar: null,
      activeChatName: null,
      chatList: [],
      groups,
      messages: raw.slice(1),
      chatMetadata: raw[0]?.chat_metadata ?? null,
      streamingText: null,
      streamingReasoning: null,
    });
  },

  createGroup: async (name, members) => {
    const g = await rpc.call<Group>('groups.create', {
      group: { name, members },
    });
    await get().loadAll();
    await get().openGroup(g.id);
  },

  saveGroup: async (group) => {
    await rpc.call('groups.edit', { group });
    const groups = await rpc.call<Group[]>('groups.all', {});
    set({ groups });
  },

  deleteGroup: async (groupId) => {
    await rpc.call('groups.delete', { id: groupId });
    const groups = await rpc.call<Group[]>('groups.all', {});
    set({
      groups,
      ...(get().activeGroupId === groupId
        ? { activeGroupId: null, messages: [], chatMetadata: null }
        : {}),
    });
  },

  sendGroup: async (text, member) => {
    const { activeGroupId, groups, generating } = get();
    const g = groups.find((x) => x.id === activeGroupId);
    if (!g || generating) return false;
    set({ generating: true, streamingText: '', streamingReasoning: '' });
    let ok = true;
    try {
      await rpc.call('generate.group', {
        id: g.id,
        chat_id: g.chat_id,
        user_message: text,
        ...(member ? { member } : {}),
      });
      const raw = await rpc.call<any[]>('groups.get_chat', { chat_id: g.chat_id });
      set({ messages: raw.slice(1), chatMetadata: raw[0]?.chat_metadata ?? null });
    } catch {
      ok = false;
      await get().reloadChat().catch(() => {});
    } finally {
      set({ generating: false, streamingText: null, streamingReasoning: null });
    }
    return ok;
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
    const [characters, settings, groups] = await Promise.all([
      rpc.call<CharacterSummary[]>('characters.all', {}),
      rpc.call<Settings>('settings.get', {}),
      rpc.call<Group[]>('groups.all', {}),
    ]);
    const withAvatars = characters.map((c) => ({
      ...c,
      avatarUrl: `/thumbnail?file=${encodeURIComponent(c.avatar)}`,
    }));
    set({ characters: withAvatars, settings, groups });
  },

  selectCharacter: async (avatar) => {
    const chatList = await rpc.call<string[]>('characters.chats', { avatar });
    const activeChatName = chatList.length ? chatList[chatList.length - 1] : null;
    let messages: ChatMessage[] = [];
    let chatMetadata: ChatMetadata | null = null;
    if (activeChatName) {
      const raw = await rpc.call<any[]>('chats.get', { avatar, file_name: activeChatName });
      const parsed = parseChat(raw);
      messages = parsed.messages;
      chatMetadata = parsed.metadata;
    }
    set({ activeGroupId: null, activeAvatar: avatar, chatList, activeChatName, messages, chatMetadata, streamingText: null, streamingReasoning: null });
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
      set({ activeAvatar: null, activeChatName: null, messages: [], chatMetadata: null });
    }
    await get().loadAll();
  },

  deleteMessage: async (index) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    await rpc.call('chats.delete_message', { avatar: activeAvatar, file_name: activeChatName, index });
    await get().reloadChat();
  },

  editMessage: async (index, text) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    await rpc.call('chats.update_message', {
      avatar: activeAvatar,
      file_name: activeChatName,
      index,
      text,
    });
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
    const { messages, metadata } = parseChat(raw);
    set({ messages, chatMetadata: metadata });
  },

  send: async (text) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return false;
    set({ generating: true, streamingText: '', streamingReasoning: '' });
    let ok = true;
    try {
      await rpc.call('generate.run', {
        avatar: activeAvatar,
        chat_file: activeChatName,
        type: 'normal',
        user_message: text,
      });
      await get().reloadChat();
    } catch {
      ok = false;
      await get().reloadChat().catch(() => {});
    } finally {
      set({ generating: false, streamingText: null, streamingReasoning: null });
    }
    return ok;
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
    set({ generating: true, streamingText: '', streamingReasoning: '' });
    let ok = true;
    try {
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'swipe' });
      await get().reloadChat();
    } catch {
      ok = false;
      await get().reloadChat().catch(() => {});
    } finally {
      set({ generating: false, streamingText: null, streamingReasoning: null });
    }
    return ok;
  },

  regenerate: async () => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName || get().generating) return;
    set({ generating: true, streamingText: '', streamingReasoning: '' });
    let ok = true;
    try {
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'regenerate' });
      await get().reloadChat();
    } catch {
      ok = false;
      await get().reloadChat().catch(() => {});
    } finally {
      set({ generating: false, streamingText: null, streamingReasoning: null });
    }
    return ok;
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
    set({ generating: true, streamingText: '', streamingReasoning: '' });
    let ok = true;
    try {
      await rpc.call('generate.run', { avatar: activeAvatar, chat_file: activeChatName, type: 'continue' });
      await get().reloadChat();
    } catch {
      ok = false;
      await get().reloadChat().catch(() => {});
    } finally {
      set({ generating: false, streamingText: null, streamingReasoning: null });
    }
    return ok;
  },

  stopGeneration: async () => {
    await rpc.call('generate.stop', {});
    set({ generating: false });
  },

  appendStreamToken: (t) => set((s) => ({ streamingText: (s.streamingText ?? '') + t })),
  appendStreamReasoning: (t) => set((s) => ({ streamingReasoning: (s.streamingReasoning ?? '') + t })),

  saveSettings: async (s) => {
    await rpc.call('settings.save', { settings: s });
    set({ settings: s });
  },

  setNote: async (note) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    await rpc.call('chats.set_note', { avatar: activeAvatar, file_name: activeChatName, note });
    await get().reloadChat();
  },

  setChatPersona: async (persona) => {
    const { activeAvatar, activeChatName } = get();
    if (!activeAvatar || !activeChatName) return;
    await rpc.call('chats.set_persona', { avatar: activeAvatar, file_name: activeChatName, persona });
    await get().reloadChat();
  },
}));
