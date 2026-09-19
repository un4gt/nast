// 内置斜杠命令（对齐 ST 的发送前拦截语义）：
// 命中 → 本地执行，不触发生成；未知命令 → 拦截提示；
// 插件命令（plugins.list 注册的）→ 透传给服务端执行。
// 返回值：handled=已处理；passthrough=交由服务端（插件命令）；unknown=未知命令。

import { rpc } from './rpc';
import { pushToast } from './toasts';
import { useStore } from './store';

export type CommandResult = 'handled' | 'passthrough' | 'unknown';

interface Command {
  name: string;
  desc: string;
  usage?: string;
  run: (args: string) => Promise<void> | void;
}

const HELP = `
内置命令（本地执行，不触发生成）：
  /help                    本帮助
  /newchat [n]             新聊天（n=开场白序号，缺省随机）
  /del <n>                 删除第 n 条消息（0 起，最新消息可省略 n）
  /swipe <left|right>      切换/生成 swipe
  /regenerate              重新生成最后一条回复
  /continue                续写最后一条回复
  /impersonate             以用户身份生成发言（填入输入框）
  /sys <text>              插入旁白系统消息（落盘，不生成）
  /name <新名称>            重命名当前聊天
  /go <片段>                切换到名称包含片段的聊天
  /persona <名称|none>      绑定/解绑本聊天的 persona
插件命令（服务端 Lua 注册）将以 /<命令名> 形式生效`.trim();

export async function runSlashCommand(
  input: string,
  helpers: { setInput: (v: string) => void } = { setInput: () => {} },
): Promise<CommandResult> {
  const store = useStore.getState();
  const { activeAvatar, activeChatName, messages, chatList } = store;
  if (!activeAvatar || !activeChatName) return 'unknown';

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
  ];

  const builtin = commands.find((c) => c.name === name || c.name.startsWith(name));
  if (builtin) {
    await builtin.run();
    return 'handled';
  }

  // 前缀唯一匹配后仍未命中 → 看是否插件命令（透传服务端），否则拦截
  const known = await pluginCommands();
  if (known.includes(name)) return 'passthrough';
  pushToast(`未知命令 /${name} —— 输入 /help 查看可用命令`, 'error');
  return 'unknown';
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
