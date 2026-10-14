// 业务命令统一由后端分发；浏览器仅处理导航、帮助和朗读。
import { rpc } from './rpc';
import { currentConversation } from './models';
import { pushToast } from './toasts';
import { useStore } from './store';
import { ttsPlayer } from './tts/player';

export type CommandResult =
  | { status: 'handled' }
  | { status: 'passthrough' }
  | { status: 'unknown' }
  | { status: 'send'; text: string };

const HELP = [
  '/help 命令帮助；/model [id|info] 选择模型',
  '/newchat [n] 新会话；/name 名称 重命名；/go 片段 切换会话',
  '/trigger [成员] 触发发言；/regenerate 重生成；/continue 续写',
  '/swipe left|right 候选；/impersonate 代用户生成；/quiet 提示 静默生成',
  '/sys 文本 旁白；/del [序号] 删除；/world 名称|none 世界书',
  '/persona 名称|none 绑定 Persona；/branch [序号] 分支；/bookmark [序号] 书签',
  '/member-list 列出成员；/member-enable、/member-disable、/member-up、/member-down 成员',
  '/chars 角色列表；/char 片段 切换角色；/worlds 世界书列表',
  '/speak [voice="角色名"] 文本 朗读；/tts-stop 停止朗读',
  'Lua 与自定义命令直接交给服务端，无需预先获取插件清单。',
].join('\n');

export async function runSlashCommand(
  input: string,
  helpers: { setInput: (v: string) => void } = { setInput: () => {} },
): Promise<CommandResult> {
  const store = useStore.getState();
  const match = input.trim().match(/^\/([^\s]+)(?:\s+([\s\S]*))?$/);
  if (!match) return { status: 'passthrough' };
  const name = match[1].toLowerCase();
  const args = match[2]?.trim() ?? '';
  if (name === 'help') { pushToast(HELP, 'info'); return { status: 'handled' }; }
  if (name === 'model' && !args) {
    window.dispatchEvent(new CustomEvent('nast:select-model'));
    return { status: 'handled' };
  }
  if (name === 'speak' || name === 'tts-stop') {
    if (name === 'tts-stop') ttsPlayer.cancel();
    else {
      const voice = args.match(/^voice=(?:"([^"]+)"|'([^']+)'|(\S+))\s+/);
      const text = (voice ? args.slice(voice[0].length) : args).trim();
      if (text) ttsPlayer.speakText(text, voice?.[1] ?? voice?.[2] ?? voice?.[3]);
      else pushToast('用法：/speak [voice="角色名"] 要朗读的文本', 'info');
    }
    return { status: 'handled' };
  }
  if (name === 'chars') {
    pushToast(store.characters.map(c => c.name).join('\n') || '（无角色卡）', 'info');
    return { status: 'handled' };
  }
  if (name === 'char' || name === 'character') {
    const hit = args && store.characters.find(c => c.name.toLowerCase().includes(args.toLowerCase()));
    if (hit) await store.selectCharacter(hit.avatar);
    else pushToast('没有匹配的角色', 'error');
    return { status: 'handled' };
  }
  if (name === 'worlds') {
    const worlds = await rpc.call<string[]>('worlds.list', {});
    pushToast(worlds.join('\n') || '（无世界书）', 'info');
    return { status: 'handled' };
  }
  if (name === 'go' && !store.activeGroupId) {
    const target = args && store.chatList.find(f => f !== store.activeChatName && f.toLowerCase().includes(args.toLowerCase()));
    if (target && store.activeAvatar) await store.openChat(store.activeAvatar, target);
    else pushToast('没有匹配的会话', 'error');
    return { status: 'handled' };
  }
  const conversation = currentConversation();
  if (!conversation || store.generating) return { status: 'handled' };
  useStore.setState({ generating: true, streamingText: '', streamingReasoning: '' });
  try {
    const result = await rpc.call<any>('commands.execute', { conversation, input });
    if (result.text && result.saved !== true) {
      if (name === 'impersonate' || name === 'imp') helpers.setInput(result.text);
      else pushToast(result.text, result.status === 'unknown' ? 'error' : 'info');
    }
    if (conversation.kind === 'group') await useStore.getState().openGroup(conversation.group_id!);
    else if (result.result?.file_name) await store.openChat(conversation.avatar!, result.result.file_name);
    else if ((name === 'name' || name === 'renamechat') && result.result?.name) await store.openChat(conversation.avatar!, `${result.result.name}.jsonl`);
    else await store.reloadChat();
    return { status: result.status === 'unknown' ? 'unknown' : 'handled' };
  } finally {
    useStore.setState({ generating: false, streamingText: null, streamingReasoning: null });
  }
}

let pluginCmdCache: { at: number; list: string[] } | null = null;
/** 清单只用于帮助与补全，执行路径不依赖它。 */
export async function pluginCommands(): Promise<string[]> {
  if (pluginCmdCache && Date.now() - pluginCmdCache.at < 30_000) return pluginCmdCache.list;
  try {
    const r = await rpc.call<{ plugins: { commands: string[] }[] }>('plugins.list', {});
    const list = (r.plugins ?? []).flatMap(p => p.commands ?? []);
    pluginCmdCache = { at: Date.now(), list };
    return list;
  } catch { return []; }
}
