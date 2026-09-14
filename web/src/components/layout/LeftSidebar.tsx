import { useRef } from 'react';
import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent,
  SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuButton, SidebarMenuItem,
} from '@/components/ui/sidebar';
import { Button } from '@/components/ui/button';
import { Avatar, AvatarFallback } from '@/components/ui/avatar';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Upload, Settings, Users } from 'lucide-react';
import { useStore } from '../../store';

/** 聊天文件名内嵌时间戳 "…@13h05m05s393ms.jsonl" → 分组标签。 */
function chatGroup(fileName: string, others: string[]): string {
  // 后端 humanizedDateTime 按生成顺序递增；文件名排序即时间排序。
  // 分组依据：与最新聊天的间隔无法精确得知日期，简化为 Newest / Earlier。
  const sorted = [...others].sort().reverse();
  const newest = sorted.slice(0, 1);
  return newest.includes(fileName) ? '最新' : '更早';
}

export function LeftSidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { connected, characters, activeAvatar, chatList, activeChatName, selectCharacter, openChat, importFile } =
    useStore();
  const fileRef = useRef<HTMLInputElement>(null);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1.5">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary font-bold text-primary-foreground">
            N
          </div>
          <div className="flex flex-col overflow-hidden group-data-[collapsible=icon]:hidden">
            <span className="text-sm font-semibold leading-none">nast</span>
            <span
              className={
                'text-[10px] leading-tight ' + (connected ? 'text-emerald-500' : 'text-destructive')
              }
            >
              {connected ? '已连接' : '连接中…'}
            </span>
          </div>
        </div>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Characters</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {characters.map((c) => (
                <SidebarMenuItem key={c.avatar}>
                  <SidebarMenuButton
                    isActive={activeAvatar === c.avatar}
                    onClick={() => selectCharacter(c.avatar)}
                    tooltip={c.name}
                  >
                    <Avatar className="size-7">
                      <AvatarFallback className="bg-primary/20 text-xs text-primary">
                        {c.name.slice(0, 2)}
                      </AvatarFallback>
                    </Avatar>
                    <span className="truncate">{c.name}</span>
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
                        tooltip={file.replace(/.jsonl$/, '')}
                      >
                        <span className="truncate text-xs">{file.replace(/.jsonl$/, '')}</span>
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
      </SidebarFooter>
    </Sidebar>
  );
}
