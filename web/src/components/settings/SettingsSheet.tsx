import { useEffect, useState } from 'react';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';
import {
  Plug, SlidersHorizontal, Sparkles, UserCircle, Palette, BookOpen, Info,
} from 'lucide-react';
import { ConnectionPanel } from './sections/ConnectionPanel';
import { SamplingPanel } from './sections/SamplingPanel';
import { AiResponsePanel } from './sections/AiResponsePanel';
import { PersonaPanel } from './sections/PersonaPanel';
import { AppearancePanel } from './sections/AppearancePanel';
import { WorldInfoGlobalPanel } from './sections/WorldInfoGlobalPanel';
import { AboutPanel } from './sections/AboutPanel';

const SECTIONS = [
  { id: 'connection', icon: Plug, label: '连接' },
  { id: 'sampling', icon: SlidersHorizontal, label: '采样参数' },
  { id: 'ai-response', icon: Sparkles, label: 'AI 回复' },
  { id: 'persona', icon: UserCircle, label: '用户 / Persona' },
  { id: 'appearance', icon: Palette, label: '外观' },
  { id: 'world-info', icon: BookOpen, label: '世界书全局' },
  { id: 'about', icon: Info, label: '关于' },
] as const;

type SectionId = (typeof SECTIONS)[number]['id'];

export function SettingsSheet({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const { settings, saveSettings } = useStore();
  const [section, setSection] = useState<SectionId>('connection');
  const [draft, setDraft] = useState<any>({});
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    if (open && settings) {
      setDraft(structuredClone(settings));
      setDirty(false);
    }
  }, [open, settings]);

  const patch = (path: string, value: unknown) => {
    setDraft((d: any) => {
      const next = structuredClone(d);
      const keys = path.split('.');
      let cur = next;
      for (let i = 0; i < keys.length - 1; i++) {
        cur[keys[i]] = cur[keys[i]] ?? {};
        cur = cur[keys[i]];
      }
      cur[keys[keys.length - 1]] = value;
      return next;
    });
    setDirty(true);
  };

  const patchWi = (key: string, value: unknown) => patch(`world_info.${key}`, value);
  const patchOai = (key: string, value: unknown) => patch(`oai_settings.${key}`, value);

  const save = async () => {
    try {
      await saveSettings(draft);
      pushToast('设置已保存', 'success');
      setDirty(false);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    }
  };

  const oai = draft.oai_settings ?? {};
  const wi = draft.world_info ?? {};

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="w-[720px] max-w-[92vw] gap-0 p-0 sm:max-w-[92vw]">
        <SheetHeader className="shrink-0 border-b px-6 py-4">
          <SheetTitle className="flex items-center gap-2">
            设置
            {dirty && (
              <button
                onClick={() => void save()}
                className="rounded-md bg-primary px-2.5 py-1 text-xs font-medium text-primary-foreground hover:opacity-90"
              >
                保存
              </button>
            )}
          </SheetTitle>
          <SheetDescription className="sr-only">nast 设置中心</SheetDescription>
        </SheetHeader>

        <div className="flex min-h-0 flex-1">
          {/* 左侧分区导航 */}
          <nav className="flex w-40 shrink-0 flex-col gap-0.5 border-r p-2">
            {SECTIONS.map((s) => (
              <button
                key={s.id}
                onClick={() => setSection(s.id)}
                className={cn(
                  'flex items-center gap-2 rounded-lg px-3 py-2 text-left text-sm transition-colors hover:bg-accent',
                  section === s.id && 'bg-accent font-medium text-accent-foreground',
                )}
              >
                <s.icon className="size-4 shrink-0" />
                <span className="truncate">{s.label}</span>
              </button>
            ))}
            <div className="flex-1" />
            {dirty && (
              <button
                onClick={() => void save()}
                className="flex items-center gap-2 rounded-lg bg-primary/10 px-3 py-2 text-left text-xs text-primary hover:bg-primary/20"
              >
                <span className="size-1.5 rounded-full bg-primary" />
                未保存的更改
              </button>
            )}
          </nav>

          {/* 右侧内容 */}
          <ScrollArea className="min-w-0 flex-1">
            <div className="p-6">
              {section === 'connection' && <ConnectionPanel oai={oai} patchOai={patchOai} />}
              {section === 'sampling' && <SamplingPanel oai={oai} patchOai={patchOai} />}
              {section === 'ai-response' && <AiResponsePanel oai={oai} patchOai={patchOai} />}
              {section === 'persona' && <PersonaPanel draft={draft} patch={patch} />}
              {section === 'appearance' && <AppearancePanel />}
              {section === 'world-info' && <WorldInfoGlobalPanel wi={wi} patchWi={patchWi} />}
              {section === 'about' && <AboutPanel />}
            </div>
          </ScrollArea>
        </div>
      </SheetContent>
    </Sheet>
  );
}
