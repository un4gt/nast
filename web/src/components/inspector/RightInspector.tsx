import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { User, BookOpen, PenLine, Brain, SlidersHorizontal, PanelRight } from 'lucide-react';
import { CharacterPanel } from './CharacterPanel';
import { WorldInfoPanel } from './WorldInfoPanel';
import { AuthorNotePanel } from './AuthorNotePanel';
import { GenerationPanel } from './GenerationPanel';
import { PlaceholderPanel } from './PlaceholderPanel';

type Tab = 'character' | 'world' | 'note' | 'memory' | 'generation';

const TABS: { id: Tab; icon: typeof User; label: string }[] = [
  { id: 'character', icon: User, label: '角色' },
  { id: 'world', icon: BookOpen, label: '世界书' },
  { id: 'note', icon: PenLine, label: '作者注记' },
  { id: 'memory', icon: Brain, label: '记忆' },
  { id: 'generation', icon: SlidersHorizontal, label: '生成' },
];

function InspectorBody({
  tab,
  setTab,
  onOpenWorldEditor,
}: {
  tab: Tab;
  setTab: (t: Tab) => void;
  onOpenWorldEditor: () => void;
}) {
  return (
    <Tabs
      value={tab}
      onValueChange={(value) => setTab(value as Tab)}
      className="flex min-h-0 flex-1 flex-col"
    >
      <TabsList
        aria-label="详情分类"
        className="mx-3 my-3 grid h-auto shrink-0 grid-cols-5 gap-0.5"
      >
        {TABS.map((t) => (
          <TabsTrigger
            key={t.id}
            value={t.id}
            className="flex flex-col gap-1.5 px-0 py-2 text-[11px]"
          >
            <t.icon className="size-4" />
            {t.label}
          </TabsTrigger>
        ))}
      </TabsList>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-5">
        <TabsContent value="character" className="mt-0">
          <CharacterPanel />
        </TabsContent>
        <TabsContent value="world" className="mt-0">
          <WorldInfoPanel onOpenEditor={onOpenWorldEditor} />
        </TabsContent>
        <TabsContent value="note" className="mt-0">
          <AuthorNotePanel />
        </TabsContent>
        <TabsContent value="memory" className="mt-0">
          <PlaceholderPanel title="记忆" />
        </TabsContent>
        <TabsContent value="generation" className="mt-0">
          <GenerationPanel />
        </TabsContent>
      </div>
    </Tabs>
  );
}

export function RightInspector({
  collapsed,
  onToggleCollapse,
  onOpenWorldEditor,
}: {
  collapsed: boolean;
  onToggleCollapse: () => void;
  onOpenWorldEditor: () => void;
}) {
  const [tab, setTab] = useState<Tab>('character');

  if (collapsed) {
    return (
      <aside
        aria-label="对话详情"
        className="hidden h-full w-14 shrink-0 flex-col items-center gap-2 border-l bg-sidebar py-3 xl:flex"
      >
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-9"
              onClick={onToggleCollapse}
              aria-label="展开对话详情"
            >
              <PanelRight />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="left">展开对话详情</TooltipContent>
        </Tooltip>
        <Separator className="my-1 w-6" />
        {TABS.map((t) => (
          <Tooltip key={t.id}>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="size-9"
                aria-label={t.label}
                onClick={() => {
                  setTab(t.id);
                  onToggleCollapse();
                }}
              >
                <t.icon />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="left">{t.label}</TooltipContent>
          </Tooltip>
        ))}
      </aside>
    );
  }

  return (
    <aside
      aria-label="对话详情"
      className="hidden h-full w-80 shrink-0 flex-col border-l bg-sidebar xl:flex"
    >
      <div className="flex h-16 shrink-0 items-center gap-1 border-b px-4">
        <span className="flex-1 text-sm font-medium">对话详情</span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-8"
              onClick={onToggleCollapse}
              aria-label="收起对话详情"
            >
              <PanelRight />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="left">收起</TooltipContent>
        </Tooltip>
      </div>
      <InspectorBody tab={tab} setTab={setTab} onOpenWorldEditor={onOpenWorldEditor} />
    </aside>
  );
}

/** 移动端 Inspector（Sheet）。 */
export function InspectorMobileSheet({
  open,
  onOpenChange,
  onOpenWorldEditor,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onOpenWorldEditor: () => void;
}) {
  const [tab, setTab] = useState<Tab>('character');
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="w-80 max-w-[88vw] gap-0 p-0 sm:max-w-[88vw]">
        <SheetHeader className="shrink-0 border-b px-4 py-3">
          <SheetTitle className="text-sm">对话详情</SheetTitle>
          <SheetDescription className="sr-only">角色、世界书、作者注记与生成参数</SheetDescription>
        </SheetHeader>
        <div className="flex min-h-0 flex-1 flex-col">
          <InspectorBody tab={tab} setTab={setTab} onOpenWorldEditor={onOpenWorldEditor} />
        </div>
      </SheetContent>
    </Sheet>
  );
}
