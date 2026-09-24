import { useMemo, useRef, useState } from 'react';
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuAction,
  useSidebar,
} from '@/components/ui/sidebar';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Spinner } from '@/components/ui/spinner';
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty';
import { cn } from '@/lib/utils';
import {
  MessageSquare,
  PanelLeftClose,
  Search,
  Settings,
  Star,
  Upload,
  UserPlus,
  Users,
  X,
} from 'lucide-react';
import { rpc } from '../../rpc';
import { pushToast } from '../../toasts';
import { useStore } from '../../store';

/** 聊天文件名内嵌时间戳 "…@13h05m05s393ms.jsonl" → 分组标签。 */
function chatGroup(fileName: string, others: string[]): string {
  // 后端 humanizedDateTime 按生成顺序递增；文件名排序即时间排序。
  const sorted = [...others].sort().reverse();
  const newest = sorted.slice(0, 1);
  return newest.includes(fileName) ? '最新' : '更早';
}

export function LeftSidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const {
    characters,
    groups,
    activeGroupId,
    activeAvatar,
    chatList,
    activeChatName,
    selectCharacter,
    openChat,
    openGroup,
    createGroup,
    importFile,
    loadAll,
    connected,
    generating,
  } = useStore();
  const { isMobile, setOpenMobile } = useSidebar();
  const fileRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState('');
  const [favOnly, setFavOnly] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [createName, setCreateName] = useState('');
  const [creating, setCreating] = useState(false);
  const [groupOpen, setGroupOpen] = useState(false);
  const [groupName, setGroupName] = useState('');
  const [groupMembers, setGroupMembers] = useState<string[]>([]);
  const [groupBusy, setGroupBusy] = useState(false);
  const [importing, setImporting] = useState(false);

  const navigate = async (action: () => Promise<void>) => {
    try {
      await action();
      if (isMobile) setOpenMobile(false);
    } catch {
      /* RPC 错误由全局提示呈现。 */
    }
  };

  const filtered = useMemo(() => {
    let list = characters;
    if (favOnly) list = list.filter((c) => c.fav);
    const q = query.trim().toLowerCase();
    if (q) {
      list = list.filter(
        (c) => c.name.toLowerCase().includes(q) || c.tags.some((t) => t.toLowerCase().includes(q)),
      );
    }
    return list;
  }, [characters, query, favOnly]);

  const toggleFav = async (avatar: string, fav: boolean) => {
    try {
      await rpc.call('characters.edit', { avatar, data: { fav: !fav } });
      await loadAll();
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    }
  };

  const createCharacter = async () => {
    if (!createName.trim() || creating) return;
    setCreating(true);
    try {
      const card = {
        spec: 'chara_card_v2',
        spec_version: '2.0',
        data: {
          name: createName.trim(),
          description: '',
          personality: '',
          scenario: '',
          first_mes: '',
          mes_example: '',
          system_prompt: '',
          post_history_instructions: '',
          tags: [],
        },
      };
      await rpc.call('characters.import', {
        data_base64: btoa(unescape(encodeURIComponent(JSON.stringify(card)))),
      });
      await loadAll();
      pushToast('已创建角色', 'success');
      setCreateOpen(false);
      setCreateName('');
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setCreating(false);
    }
  };

  const doCreateGroup = async () => {
    if (!groupName.trim() || groupMembers.length === 0 || groupBusy) return;
    setGroupBusy(true);
    try {
      await createGroup(groupName.trim(), groupMembers);
      pushToast('已创建群组', 'success');
      setGroupOpen(false);
      setGroupName('');
      setGroupMembers([]);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setGroupBusy(false);
    }
  };

  const toggleGroupMember = (avatar: string, on: boolean) => {
    setGroupMembers((prev) => (on ? [...prev, avatar] : prev.filter((a) => a !== avatar)));
  };

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader className="gap-4 p-4 group-data-[collapsible=icon]:p-2">
        <div className="flex h-8 items-center gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-xl bg-primary text-lg font-semibold text-primary-foreground">
            n
          </div>
          <div className="flex flex-col overflow-hidden group-data-[collapsible=icon]:hidden">
            <span className="text-lg font-semibold leading-none tracking-tight">
              nast<span className="text-primary">.</span>
            </span>
            <span className="mt-1 text-[11px] leading-tight text-muted-foreground">
              你的角色，你的故事
            </span>
          </div>
          {isMobile && (
            <Button
              variant="ghost"
              size="icon"
              className="ml-auto shrink-0"
              aria-label="关闭侧栏"
              onClick={() => setOpenMobile(false)}
            >
              <PanelLeftClose />
            </Button>
          )}
        </div>
        <Button
          className="w-full group-data-[collapsible=icon]:hidden"
          disabled={!connected || importing}
          onClick={() => fileRef.current?.click()}
        >
          {importing ? <Spinner /> : <Upload data-icon="inline-start" />}导入角色卡
        </Button>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup className="px-3 group-data-[collapsible=icon]:px-2">
          <SidebarGroupLabel className="mb-2 flex justify-between">
            <span>角色库</span>
            <span className="tabular-nums">{characters.length}</span>
          </SidebarGroupLabel>
          <SidebarGroupContent>
            {/* 搜索 + 收藏筛选（收起态隐藏） */}
            <div className="mb-3 flex items-center gap-1.5 group-data-[collapsible=icon]:hidden">
              <div className="relative min-w-0 flex-1">
                <Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="搜索角色 / 标签…"
                  aria-label="搜索角色或标签"
                  className="h-9 pl-7 pr-7"
                />
                {query && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="absolute right-0.5 top-0.5 size-8"
                    aria-label="清空搜索"
                    onClick={() => setQuery('')}
                  >
                    <X />
                  </Button>
                )}
              </div>
              <Button
                variant={favOnly ? 'default' : 'ghost'}
                size="icon"
                className="size-9 shrink-0"
                onClick={() => setFavOnly((v) => !v)}
                title="只看收藏"
                aria-label="只看收藏"
                aria-pressed={favOnly}
              >
                <Star className={favOnly ? 'size-3.5 fill-current' : 'size-3.5'} />
              </Button>
            </div>
            <SidebarMenu>
              {filtered.map((c) => (
                <SidebarMenuItem key={c.avatar}>
                  <SidebarMenuButton
                    isActive={activeAvatar === c.avatar}
                    onClick={() => void navigate(() => selectCharacter(c.avatar))}
                    disabled={!connected || generating}
                    tooltip={c.name}
                    className="h-16 gap-3 rounded-xl pr-8"
                  >
                    <Avatar className="size-10 shrink-0 rounded-xl group-data-[collapsible=icon]:size-7">
                      <AvatarImage
                        src={c.avatarUrl}
                        alt={c.name}
                        className="size-full object-cover"
                      />
                      <AvatarFallback className="rounded-xl bg-accent text-primary">
                        {c.name.slice(0, 2)}
                      </AvatarFallback>
                    </Avatar>
                    <span className="flex min-w-0 flex-1 flex-col gap-1 group-data-[collapsible=icon]:hidden">
                      <span className="truncate font-medium">{c.name}</span>
                      <span className="truncate text-xs text-muted-foreground">
                        {c.tags.slice(0, 2).join(' · ') || '角色卡'}
                      </span>
                    </span>
                  </SidebarMenuButton>
                  <SidebarMenuAction
                    className="top-5"
                    showOnHover={!c.fav}
                    disabled={!connected}
                    aria-label={`${c.fav ? '取消收藏' : '收藏'} ${c.name}`}
                    aria-pressed={c.fav}
                    onClick={() => void toggleFav(c.avatar, c.fav)}
                  >
                    <Star className={cn(c.fav && 'fill-primary text-primary')} />
                  </SidebarMenuAction>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
            {filtered.length === 0 && (
              <Empty className="gap-3 px-2 py-8 group-data-[collapsible=icon]:hidden md:px-2 md:py-8">
                <EmptyHeader>
                  <EmptyTitle className="text-sm">
                    {characters.length ? '没有找到角色' : '角色库还是空的'}
                  </EmptyTitle>
                  <EmptyDescription className="text-xs">
                    {characters.length
                      ? '试试其他关键词，或取消收藏筛选。'
                      : '导入角色卡，或在下方创建一个角色。'}
                  </EmptyDescription>
                </EmptyHeader>
                {characters.length > 0 && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setQuery('');
                      setFavOnly(false);
                    }}
                  >
                    清除筛选
                  </Button>
                )}
              </Empty>
            )}
          </SidebarGroupContent>
        </SidebarGroup>

        {groups.length > 0 && (
          <SidebarGroup>
            <SidebarGroupLabel>群组</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {groups.map((g) => (
                  <SidebarMenuItem key={g.id}>
                    <SidebarMenuButton
                      isActive={activeGroupId === g.id}
                      onClick={() => void navigate(() => openGroup(g.id))}
                      disabled={!connected || generating}
                      className="h-11"
                      tooltip={g.name}
                    >
                      <Avatar className="size-7">
                        <AvatarFallback className="bg-secondary text-[10px] text-secondary-foreground">
                          {g.name.slice(0, 2)}
                        </AvatarFallback>
                      </Avatar>
                      <span className="truncate">{g.name}</span>
                      <span className="ml-auto shrink-0 text-[10px] text-muted-foreground group-data-[collapsible=icon]:hidden">
                        {g.members.length}
                      </span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        )}

        {activeAvatar && chatList.length > 0 && (
          <SidebarGroup>
            <SidebarGroupLabel>聊天记录</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {chatList.map((file) => {
                  const group = chatGroup(file, chatList);
                  return (
                    <SidebarMenuItem key={file}>
                      <SidebarMenuButton
                        isActive={activeChatName === file}
                        onClick={() => void navigate(() => openChat(activeAvatar, file))}
                        disabled={!connected || generating}
                        tooltip={file.replace(/\.jsonl$/, '')}
                      >
                        <MessageSquare />
                        <span className="truncate text-xs">{file.replace(/\.jsonl$/, '')}</span>
                        <span className="ml-auto shrink-0 text-[10px] text-muted-foreground group-data-[collapsible=icon]:hidden">
                          {group}
                        </span>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        )}
      </SidebarContent>

      <SidebarFooter className="border-t p-3 group-data-[collapsible=icon]:p-2">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              onClick={() => setGroupOpen(true)}
              tooltip="群组"
              disabled={!connected || generating}
            >
              <Users />
              <span>新建群组</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton
              onClick={() => setCreateOpen(true)}
              tooltip="新建角色"
              disabled={!connected}
            >
              <UserPlus />
              <span>新建角色</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton onClick={onOpenSettings} tooltip="设置">
              <Settings />
              <span>设置</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
        <p className="px-2 pt-1 text-[11px] text-muted-foreground group-data-[collapsible=icon]:hidden">
          Ctrl / ⌘ + B 收起侧栏
        </p>
        <input
          ref={fileRef}
          type="file"
          accept=".png,.json,.yaml,.yml,.charx,.byaf"
          multiple
          hidden
          onChange={async (e) => {
            const files = Array.from(e.target.files ?? []);
            e.target.value = '';
            setImporting(true);
            await Promise.allSettled(files.map(importFile));
            setImporting(false);
          }}
        />

        <Dialog open={createOpen} onOpenChange={setCreateOpen}>
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>新建角色</DialogTitle>
              <DialogDescription>创建空白角色卡，之后在右侧面板编辑详情。</DialogDescription>
            </DialogHeader>
            <Input
              value={createName}
              onChange={(e) => setCreateName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229)
                  void createCharacter();
              }}
              placeholder="角色名"
              aria-label="角色名"
            />
            <DialogFooter>
              <Button variant="ghost" onClick={() => setCreateOpen(false)}>
                取消
              </Button>
              <Button
                onClick={() => void createCharacter()}
                disabled={creating || !createName.trim()}
              >
                {creating ? <Spinner /> : null}
                创建
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={groupOpen} onOpenChange={setGroupOpen}>
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>新建群组</DialogTitle>
              <DialogDescription>
                选择参与对话的角色；激活策略与健谈度可在群设置中调整。
              </DialogDescription>
            </DialogHeader>
            <Input
              value={groupName}
              onChange={(e) => setGroupName(e.target.value)}
              placeholder="群名称"
              aria-label="群名称"
            />
            <div className="flex max-h-56 flex-col gap-1 overflow-y-auto rounded-md border p-2">
              {characters.map((c) => (
                <label
                  key={c.avatar}
                  className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent/50"
                >
                  <Checkbox
                    checked={groupMembers.includes(c.avatar)}
                    onCheckedChange={(v) => toggleGroupMember(c.avatar, v === true)}
                  />
                  <Avatar className="size-6">
                    <AvatarImage
                      src={c.avatarUrl}
                      alt={c.name}
                      className="size-full object-cover"
                    />
                    <AvatarFallback className="text-[10px]">{c.name.slice(0, 1)}</AvatarFallback>
                  </Avatar>
                  <span className="truncate">{c.name}</span>
                </label>
              ))}
              {characters.length === 0 && (
                <p className="px-1 text-xs text-muted-foreground">先导入角色卡</p>
              )}
            </div>
            {groupMembers.length > 0 && (
              <Label className="text-[10px] text-muted-foreground">
                已选 {groupMembers.length} 名成员
              </Label>
            )}
            <DialogFooter>
              <Button variant="ghost" onClick={() => setGroupOpen(false)}>
                取消
              </Button>
              <Button
                onClick={() => void doCreateGroup()}
                disabled={groupBusy || !groupName.trim() || groupMembers.length === 0}
              >
                {groupBusy ? <Spinner /> : null}
                创建
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </SidebarFooter>
    </Sidebar>
  );
}
