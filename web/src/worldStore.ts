import { create } from 'zustand';
import { rpc } from './rpc';

export interface WIEntry {
  uid: number;
  key: string[];
  keysecondary: string[];
  comment: string;
  content: string;
  constant: boolean;
  selective: boolean;
  selectiveLogic: number;
  order: number;
  position: number | string;
  disable: boolean;
  excludeRecursion: boolean;
  preventRecursion: boolean;
  delayUntilRecursion: boolean | number;
  probability: number;
  useProbability: boolean;
  depth: number;
  group: string;
  groupOverride: boolean;
  groupWeight: number;
  role: number | null;
  sticky: number;
  cooldown: number;
  delay: number;
  ignoreBudget: boolean;
  [key: string]: unknown;
}

interface WorldStore {
  worlds: string[];
  activeWorld: string | null;
  entries: Record<string, WIEntry>;
  bookExtra: Record<string, unknown>;
  loadWorlds: () => Promise<void>;
  openWorld: (name: string) => Promise<void>;
  saveWorld: () => Promise<void>;
  createWorld: (name: string) => Promise<void>;
  deleteWorld: (name: string) => Promise<void>;
  setActiveWorld: (name: string | null) => void;
  updateEntry: (uid: string, patch: Partial<WIEntry>) => void;
  addEntry: () => void;
  deleteEntry: (uid: string) => void;
}

export const useWorldStore = create<WorldStore>((set, get) => ({
  worlds: [],
  activeWorld: null,
  entries: {},
  bookExtra: {},

  loadWorlds: async () => {
    const worlds = await rpc.call<string[]>('worlds.list', {});
    set({ worlds });
  },

  openWorld: async (name) => {
    const book = await rpc.call<{ entries: Record<string, WIEntry>; [key: string]: unknown }>('worlds.get', { name });
    const { entries, ...bookExtra } = book;
    set({ activeWorld: name, entries: entries ?? {}, bookExtra });
  },

  setActiveWorld: (name) => set({ activeWorld: name, entries: {} }),

  saveWorld: async () => {
    const { activeWorld, entries, bookExtra } = get();
    if (!activeWorld) return;
    await rpc.call('worlds.save', { name: activeWorld, book: { ...bookExtra, entries } });
    await get().loadWorlds();
  },

  createWorld: async (name) => {
    if ((await rpc.call<string[]>('worlds.list', {})).includes(name)) throw new Error('已有同名世界书，请使用其他名称');
    await rpc.call('worlds.save', { name, book: { entries: {} } });
    await get().loadWorlds();
    await get().openWorld(name);
  },

  deleteWorld: async (name) => {
    await rpc.call('worlds.delete', { name });
    if (get().activeWorld === name) set({ activeWorld: null, entries: {} });
    await get().loadWorlds();
  },

  updateEntry: (uid, patch) =>
    set((s) => ({
      entries: { ...s.entries, [uid]: { ...s.entries[uid], ...patch } },
    })),

  addEntry: () =>
    set((s) => {
      let candidate = 0;
      while (String(candidate) in s.entries) candidate += 1;
      const uid = String(candidate);
      return {
        entries: {
          ...s.entries,
          [uid]: {
            uid: Number(uid),
            key: [],
            keysecondary: [],
            comment: '新条目',
            content: '',
            constant: false,
            selective: true,
            selectiveLogic: 0,
            order: 100,
            position: 0,
            disable: false,
            excludeRecursion: false,
            preventRecursion: false,
            delayUntilRecursion: false,
            probability: 100,
            useProbability: true,
            depth: 4,
            group: '',
            groupOverride: false,
            groupWeight: 100,
            role: null,
            sticky: 0,
            cooldown: 0,
            delay: 0,
            ignoreBudget: false,
          },
        },
      };
    }),

  deleteEntry: (uid) =>
    set((s) => {
      const entries = { ...s.entries };
      delete entries[uid];
      return { entries };
    }),
}));
