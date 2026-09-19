// 内置斜杠命令（对齐 ST 的发送前拦截语义）：
// 命中 → 本地执行，不触发生成；自定义命令 → 展开为文本发送；
// 插件命令（plugins.list 注册的）→ 透传给服务端执行；未知命令 → 拦截提示。

import { rpc } from './rpc';
import { pushToast } from './toasts';
import { useStore } from './store';

export type CommandResult =
  | { status: 'handled' }
  | { status: 'passthrough' }
  | { status: 'unknown' }
  | { status: 'send'; text: string };

interface Command {
  name: string;
  desc: string;
  usage?: string;
  run: (args: string) => Promise<void> | void;
}

const NL = String.fromCharCode(10);

const HELP = [
  '内置命令（本地执行，不触发生成）：',
  '  /help                    本帮助',
  '  /newchat [n]             新聊天（n=开场白序号，缺省随机）',
  '  /del <n>                 删除第 n 条消息（0 起，最新消息可省略 n）',
  '  /swipe <left|right>      切换/生成 swipe',
  '  /regenerate              重新生成最后一条回复',
  '  /continue                续写最后一条回复',
  '  /impersonate             以用户身份生成发言（填入输入框）',
  '  /sys <text>              插入旁白系统消息（落盘，不生成）',
  '  /name <新名称>            重命名当前聊天',
  '  /go <片段>                切换到名称包含片段的聊天',
  '  /persona <名称|none>      绑定/解绑本聊天的 persona',
  '  /chars                   列出全部角色卡',
  '  /char <片段>              切换到名称匹配的角色（/character 同义）',
  '  /worlds                  列出全部世界书',
  '  /world <名称|none>        绑定/解绑本聊天的世界书',
  '自定义命令（设置 → 自定义命令）以 /<名称> 展开为预设文本发送；',
  '插件命令（服务端 Lua 注册）同样以 /<命令名> 生效',
].join(NL);

export async function runSlashCommand(
  input: string,
  helpers: { setInput: (v: string) => void } = { setInput: () => {} },
): Promise<CommandResult> {
  const store = useStore.getState();
  const { activeAvatar, activeChatName, messages, chatList } = store;
  if (!activeAvatar || !activeChatName) return { status: 'unknown' };

  const body = input.slice(1);
  const [rawName, rest] = splitOnce(body);
  const name = rawName.toLowerCase();
  const args = rest;

  const commands: Command[] = [
    {
      name: 'help',
      desc: '命令帮助',
      run: () => pushToast(HELP, 'info'),
    },
    {
      name: 'newchat',
      usage: '/newchat [n]',
      desc: '新聊天',
      run: async () => {
        const idx = args.trim() === '' ? -1 : Number(args.trim());
        await store.newChat(activeAvatar, Number.isFinite(idx) ? idx : -1);
        pushToast('已创建新聊天', 'success');
      },
    },
    {
      name: 'del',
      usage: '/del <n>',
      desc: '删除消息',
      run: async () => {
        const idx = args.trim() === '' ? messages.length - 1 : Number(args.trim());
        if (!Number.isInteger(idx) || idx < 0 || idx >= messages.length) {
          pushToast(`无效的消息序号（0–${messages.length - 1}）`, 'error');
          return;
        }
        await store.deleteMessage(idx);
        pushToast(`已删除消息 #${idx}`, 'success');
      },
    },
    {
      name: 'swipe',
      usage: '/swipe <left|right>',
      desc: '切换 swipe',
      run: async () => {
        const dir = args.trim().toLowerCase();
        if (dir !== 'left' && dir !== 'right') {
          pushToast('用法：/swipe left 或 /swipe right', 'error');
          return;
        }
        await store.swipe(dir as 'left' | 'right');
      },
    },
    {
      name: 'regenerate',
      desc: '重新生成',
      run: () => store.regenerate(),
    },
    {
      name: 'continue',
      desc: '续写',
      run: () => store.continueGen(),
    },
    {
      name: 'impersonate',
      desc: '代入生成',
      run: async () => {
        const text = await store.impersonate();
        if (text) helpers.setInput(text);
      },
    },
    {
      name: 'sys',
      usage: '/sys <text>',
      desc: '插入旁白系统消息',
      run: async () => {
        const text = args.trim();
        if (!text) {
          pushToast('用法：/sys <内容>', 'error');
          return;
        }
        const raw = await rpc.call<any[]>('chats.get', {
          avatar: activeAvatar,
          file_name: activeChatName,
        });
        raw.push({
          name: 'System',
          is_user: false,
          is_system: true,
          send_date: new Date().toISOString(),
          mes: text,
          extra: { type: 'narrator' },
        });
        await rpc.call('chats.save', {
          avatar: activeAvatar,
          file_name: activeChatName,
          chat: raw,
        });
        await store.reloadChat();
        pushToast('已插入系统旁白', 'success');
      },
    },
    {
      name: 'name',
      usage: '/name <新名称>',
      desc: '重命名聊天',
      run: async () => {
        const next = args.trim();
        if (!next) {
          pushToast('用法：/name <新名称>', 'error');
          return;
        }
        const r = await rpc.call<{ name: string }>('chats.rename', {
          avatar: activeAvatar,
          original_file: activeChatName,
          renamed_file: next,
        });
        await store.openChat(activeAvatar, `${r.name}.jsonl`);
        pushToast('聊天已重命名', 'success');
      },
    },
    {
      name: 'go',
      usage: '/go <片段>',
      desc: '切换聊天',
      run: async () => {
        const frag = args.trim().toLowerCase();
        const target = chatList.find((f) =>
          f !== activeChatName && f.toLowerCase().includes(frag),
        );
        if (!frag || !target) {
          pushToast(`没有匹配的聊天（${frag || '空'}）`, 'error');
          return;
        }
        await store.openChat(activeAvatar, target);
        pushToast(`已切换到 ${target.replace(/\.jsonl$/, '')}`, 'success');
      },
    },
    {
      name: 'persona',
      usage: '/persona <名称|none>',
      desc: '绑定 persona',
      run: async () => {
        const q = args.trim();
        if (!q) {
          pushToast('用法：/persona <名称> 或 /persona none', 'error');
          return;
        }
        if (q.toLowerCase() === 'none') {
          await store.setChatPersona(null);
          pushToast('已解除 persona 绑定', 'success');
          return;
        }
        const personas: Record<string, string> =
          ((store.settings as any)?.power_user?.personas ?? {});
        const hit = Object.entries(personas).find(
          ([, n]) => String(n).toLowerCase() === q.toLowerCase(),
        ) ?? Object.entries(personas).find(([, n]) =>
          String(n).toLowerCase().includes(q.toLowerCase()),
        );
        if (!hit) {
          pushToast(`没有名为「${q}」的 persona`, 'error');
          return;
        }
        await store.setChatPersona(hit[0]);
        pushToast(`已绑定 persona：${hit[1]}`, 'success');
      },
    },
    {
      name: 'chars',
      usage: '/chars',
      desc: '列出角色卡',
      run: async () => {
        const list = store.characters
          .map((c) => `${c.fav ? '★' : '·'} ${c.name}`)
          .join(NL);
        pushToast(list || '（无角色卡）', 'info');
      },
    },
    {
      name: 'character',
      usage: '/char <片段>',
      desc: '切换角色',
      run: async () => {
        const frag = args.trim().toLowerCase();
        if (!frag) {
          pushToast('用法：/char <角色名片段>', 'error');
          return;
        }
        const hit = store.characters.find((c) => c.name.toLowerCase().includes(frag));
        if (!hit) {
          pushToast(`没有名字包含「${args.trim()}」的角色`, 'error');
          return;
        }
        await store.selectCharacter(hit.avatar);
        pushToast(`已切换角色：${hit.name}`, 'success');
      },
    },
    {
      name: 'worlds',
      usage: '/worlds',
      desc: '列出世界书',
      run: async () => {
        const list = await rpc.call<string[]>('worlds.list', {}).catch(() => [] as string[]);
        pushToast(list.length ? list.map((w) => `· ${w}`).join(NL) : '（无世界书）', 'info');
      },
    },
    {
      name: 'world',
      usage: '/world <名称|none>',
      desc: '绑定聊天世界书',
      run: async () => {
        const q = args.trim();
        const cur = store.chatMetadata?.world;
        if (!q) {
          pushToast(
            cur
              ? `当前绑定：${cur}${NL}用法：/world <名称|none>`
              : `未绑定${NL}用法：/world <名称|none>`,
            'info',
          );
          return;
        }
        if (q.toLowerCase() !== 'none') {
          const list = await rpc.call<string[]>('worlds.list', {}).catch(() => [] as string[]);
          if (!list.includes(q)) {
            pushToast(`没有名为「${q}」的世界书（/worlds 查看）`, 'error');
            return;
          }
        }
        await rpc.call('chats.set_world', {
          avatar: activeAvatar,
          file_name: activeChatName,
          world: q.toLowerCase() === 'none' ? null : q,
        });
        await store.reloadChat();
        pushToast(q.toLowerCase() === 'none' ? '已解绑世界书' : `已绑定世界书：${q}`, 'success');
      },
    },
  ];

  const builtin = commands.find((c) => c.name === name || c.name.startsWith(name));
  if (builtin) {
    await builtin.run();
    return { status: 'handled' };
  }

  // 自定义命令（power_user.custom_commands）：展开为文本发送
  const customs: { name: string; text: string }[] =
    ((store.settings as any)?.power_user?.custom_commands ?? []);
  const custom = customs.find((c) => String(c.name).toLowerCase() === name);
  if (custom?.text) {
    return { status: 'send', text: custom.text };
  }

  // 前缀唯一匹配后仍未命中 → 看是否插件命令（透传服务端），否则拦截
  const known = await pluginCommands();
  if (known.includes(name)) return { status: 'passthrough' };
  pushToast(`未知命令 /${name} —— 输入 /help 查看可用命令`, 'error');
  return { status: 'unknown' };
}

function splitOnce(s: string): [string, string] {
  const i = s.indexOf(' ');
  return i === -1 ? [s, ''] : [s.slice(0, i), s.slice(i + 1).trim()];
}

let pluginCmdCache: { at: number; list: string[] } | null = null;

/** 已注册的插件命令名（30s 缓存）。 */
export async function pluginCommands(): Promise<string[]> {
  if (pluginCmdCache && Date.now() - pluginCmdCache.at < 30_000) {
    return pluginCmdCache.list;
  }
  try {
    const r = await rpc.call<{ plugins: { commands: string[] }[] }>('plugins.list', {});
    const list = (r.plugins ?? []).flatMap((p) => p.commands ?? []);
    pluginCmdCache = { at: Date.now(), list };
    return list;
  } catch {
    return [];
  }
}
