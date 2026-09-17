import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  User, BookOpen, PenLine, Brain, SlidersHorizontal, PanelRight,
} from 'lucide-react';
import { CharacterPanel } from './CharacterPanel';
import { WorldInfoPanel } from './WorldInfoPanel';
import { AuthorNotePanel } from './AuthorNotePanel';
import { GenerationPanel } from './GenerationPanel';
import { PlaceholderPanel } from './PlaceholderPanel';

type Tab = 'character' | 'world' | 'note' | 'memory' | 'generation';

const TABS: { id: Tab; icon: typeof User; label: string }[] = [
  { id: 'character', icon: User, label: 'Character' },
  { id: 'world', icon: BookOpen, label: 'World Info' },
  { id: 'note', icon: PenLine, label: 'Author Note' },
  { id: 'memory', icon: Brain, label: 'Memory（即将支持）' },
  { id: 'generation', icon: SlidersHorizontal, label: 'Generation' },
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
    <>
      <div className="flex shrink-0 items-center gap-0.5 border-b px-2 py-1">
        {TABS.map((t) => (
          <Tooltip key={t.id}>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className={'size-7' + (tab === t.id ? ' bg-accent text-accent-foreground' : '')}
                onClick={() => setTab(t.id)}
              >
                <t.icon className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{t.label}</TooltipContent>
          </Tooltip>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        {tab === 'character' && <CharacterPanel />}
        {tab === 'world' && <WorldInfoPanel onOpenEditor={onOpenWorldEditor} />}
        {tab === 'note' && <AuthorNotePanel />}
        {tab === 'memory' && <PlaceholderPanel title="Memory" />}
        {tab === 'generation' && <GenerationPanel />}
      </div>
    </>
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
      <div className="hidden h-full w-12 shrink-0 flex-col items-center gap-1 border-l bg-sidebar py-2 md:flex">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="size-8" onClick={onToggleCollapse}>
              <PanelRight />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="left">展开 Inspector</TooltipContent>
        </Tooltip>
        <Separator className="my-1 w-6" />
        {TABS.map((t) => (
          <Tooltip key={t.id}>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className={'size-8' + (tab === t.id ? ' bg-accent text-accent-foreground' : '')}
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
      </div>
    );
  }

  return (
    <div className="hidden h-full w-72 shrink-0 flex-col border-l bg-sidebar md:flex">
      <div className="flex h-10 shrink-0 items-center gap-1 border-b px-2">
        <span className="flex-1 px-1 text-xs font-medium text-muted-foreground">Inspector</span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="size-7" onClick={onToggleCollapse}>
              <PanelRight />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="left">收起</TooltipContent>
        </Tooltip>
      </div>
      <InspectorBody tab={tab} setTab={setTab} onOpenWorldEditor={onOpenWorldEditor} />
    </div>
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
          <SheetTitle className="text-sm">Inspector</SheetTitle>
        </SheetHeader>
        <div className="flex min-h-0 flex-1 flex-col">
          <InspectorBody tab={tab} setTab={setTab} onOpenWorldEditor={onOpenWorldEditor} />
        </div>
      </SheetContent>
    </Sheet>
  );
}
