import { useMemo, useRef, useState } from 'react';
import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent,
  SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuButton, SidebarMenuItem,
} from '@/components/ui/sidebar';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from '@/components/ui/dialog';
import { Spinner } from '@/components/ui/spinner';
import {
  Search, Settings, Star, Upload, UserPlus, Users,
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
  const { characters, activeAvatar, chatList, activeChatName, selectCharacter, openChat, importFile, loadAll } =
    useStore();
  const fileRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState('');
  const [favOnly, setFavOnly] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [createName, setCreateName] = useState('');
  const [creating, setCreating] = useState(false);

  const filtered = useMemo(() => {
    let list = characters;
    if (favOnly) list = list.filter((c) => c.fav);
    const q = query.trim().toLowerCase();
    if (q) {
      list = list.filter(
        (c) =>
          c.name.toLowerCase().includes(q) ||
          c.tags.some((t) => t.toLowerCase().includes(q)),
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
    if (!createName.trim()) return;
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

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1.5">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary font-bold text-primary-foreground">
            N
          </div>
          <div className="flex flex-col overflow-hidden group-data-[collapsible=icon]:hidden">
            <span className="text-sm font-semibold leading-none">nast</span>
            <span className="text-[10px] leading-tight text-muted-foreground">
              {characters.length} 个角色
            </span>
          </div>
        </div>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Characters</SidebarGroupLabel>
          <SidebarGroupContent>
            {/* 搜索 + 收藏筛选（收起态隐藏） */}
            <div className="mb-1 flex items-center gap-1 px-2 group-data-[collapsible=icon]:hidden">
              <div className="relative flex-1">
                <Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="搜索角色 / 标签…"
                  className="h-7 pl-7 text-xs"
                />
              </div>
              <Button
                variant={favOnly ? 'default' : 'ghost'}
                size="icon"
                className="size-7 shrink-0"
                onClick={() => setFavOnly((v) => !v)}
                title="只看收藏"
              >
                <Star className={favOnly ? 'size-3.5 fill-current' : 'size-3.5'} />
              </Button>
            </div>
            <SidebarMenu>
              {filtered.map((c) => (
                <SidebarMenuItem key={c.avatar}>
                  <SidebarMenuButton
                    isActive={activeAvatar === c.avatar}
                    onClick={() => selectCharacter(c.avatar)}
                    tooltip={c.name}
                  >
                    <Avatar className="size-7">
                      <img src={c.avatarUrl} alt={c.name} className="size-full object-cover" />
                      <AvatarFallback className="bg-primary/20 text-xs text-primary">
                        {c.name.slice(0, 2)}
                      </AvatarFallback>
                    </Avatar>
                    <span className="truncate">{c.name}</span>
                    {c.fav && (
                      <Star className="ml-auto size-3 shrink-0 fill-primary text-primary" />
                    )}
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
            {characters.length === 0 && (
              <div className="px-2 py-4 text-center text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
                拖入卡片导入
              </div>
            )}
          </SidebarGroupContent>
        </SidebarGroup>

        {activeAvatar && chatList.length > 0 && (
          <SidebarGroup>
            <SidebarGroupLabel>Chats</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {chatList.map((file) => {
                  const group = chatGroup(file, chatList);
                  return (
                    <SidebarMenuItem key={file}>
                      <SidebarMenuButton
                        isActive={activeChatName === file}
                        onClick={() => openChat(activeAvatar, file)}
                        tooltip={file.replace(/\.jsonl$/, '')}
                      >
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

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <Tooltip>
              <TooltipTrigger asChild>
                <div>
                  <SidebarMenuButton disabled tooltip="Groups（即将支持）">
                    <Users />
                    <span>Groups</span>
                  </SidebarMenuButton>
                </div>
              </TooltipTrigger>
              <TooltipContent side="right">即将支持</TooltipContent>
            </Tooltip>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton onClick={() => setCreateOpen(true)} tooltip="新建角色">
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
        <Button
          variant="outline"
          className="mx-2 mb-1 group-data-[collapsible=icon]:hidden"
          onClick={() => fileRef.current?.click()}
        >
          <Upload data-icon="inline-start" />
          导入角色卡
        </Button>
        <input
          ref={fileRef}
          type="file"
          accept=".png,.json"
          multiple
          hidden
          onChange={(e) => {
            Array.from(e.target.files ?? []).forEach((f) => importFile(f));
            e.target.value = '';
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
                if (e.key === 'Enter') void createCharacter();
              }}
              placeholder="角色名"
            />
            <DialogFooter>
              <Button variant="ghost" onClick={() => setCreateOpen(false)}>取消</Button>
              <Button onClick={() => void createCharacter()} disabled={creating || !createName.trim()}>
                {creating ? <Spinner /> : null}
                创建
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </SidebarFooter>
    </Sidebar>
  );
}
