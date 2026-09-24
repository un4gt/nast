import { useEffect, useState } from 'react';
import { worldInfoPath, worldInfoView } from '@/lib/world-settings';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Spinner } from '@/components/ui/spinner';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { useStore } from '../../store';
import { pushToast } from '../../toasts';
import {
  Plug,
  SlidersHorizontal,
  Sparkles,
  UserCircle,
  Palette,
  BookOpen,
  Info,
  ListOrdered,
  FolderOpen,
  Regex,
  Puzzle,
  TerminalSquare,
  Check,
  Save,
  Headphones,
} from 'lucide-react';
import { ConnectionPanel } from './sections/ConnectionPanel';
import { SamplingPanel } from './sections/SamplingPanel';
import { AiResponsePanel } from './sections/AiResponsePanel';
import { PersonaPanel } from './sections/PersonaPanel';
import { AppearancePanel } from './sections/AppearancePanel';
import { WorldInfoGlobalPanel } from './sections/WorldInfoGlobalPanel';
import { AboutPanel } from './sections/AboutPanel';
import { PromptManagerPanel } from './sections/PromptManagerPanel';
import { PresetPanel } from './sections/PresetPanel';
import { RegexPanel } from './sections/RegexPanel';
import { PluginsPanel } from './sections/PluginsPanel';
import { CustomCommandsPanel } from './sections/CustomCommandsPanel';
import { TtsPanel } from './sections/TtsPanel';

const SECTIONS = [
  { id: 'connection', icon: Plug, label: '连接' },
  { id: 'tts', icon: Headphones, label: '语音朗读' },
  { id: 'preset', icon: FolderOpen, label: '预设' },
  { id: 'prompts', icon: ListOrdered, label: '提示词管理' },
  { id: 'sampling', icon: SlidersHorizontal, label: '采样参数' },
  { id: 'ai-response', icon: Sparkles, label: 'AI 回复' },
  { id: 'regex', icon: Regex, label: '正则脚本' },
  { id: 'persona', icon: UserCircle, label: '用户 / Persona' },
  { id: 'appearance', icon: Palette, label: '外观' },
  { id: 'world-info', icon: BookOpen, label: '世界书全局' },
  { id: 'commands', icon: TerminalSquare, label: '自定义命令' },
  { id: 'plugins', icon: Puzzle, label: '插件' },
  { id: 'about', icon: Info, label: '关于' },
] as const;

type SectionId = (typeof SECTIONS)[number]['id'];

export function SettingsSheet({
  open,
  onOpenChange,
  initialSection,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  initialSection?: string;
}) {
  const { settings, saveSettings } = useStore();
  const [section, setSection] = useState<SectionId>('connection');
  const [draft, setDraft] = useState<any>({});
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open && initialSection && SECTIONS.some((s) => s.id === initialSection)) setSection(initialSection as SectionId);
  }, [open, initialSection]);

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

  const patchWi = (key: string, value: unknown) => patch(worldInfoPath(key), value);
  const patchOai = (key: string, value: unknown) => patch(`oai_settings.${key}`, value);

  const save = async () => {
    if (saving) return;
    setSaving(true);
    try {
      await saveSettings(draft);
      pushToast('设置已保存', 'success');
      setDirty(false);
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setSaving(false);
    }
  };

  const oai = draft.oai_settings ?? {};
  const wi = worldInfoView(draft);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        side="right"
        className="w-full max-w-full gap-0 p-0 sm:w-[780px] sm:max-w-[94vw]"
      >
        <SheetHeader className="shrink-0 border-b px-5 py-5 sm:px-6">
          <SheetTitle>设置</SheetTitle>
          <SheetDescription>调整连接、角色与偏好，让对话更合心意。</SheetDescription>
        </SheetHeader>

        <div className="shrink-0 border-b px-4 py-3 sm:hidden">
          <Select value={section} onValueChange={(value) => setSection(value as SectionId)}>
            <SelectTrigger aria-label="设置分类">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {SECTIONS.map((s) => (
                  <SelectItem key={s.id} value={s.id}>
                    {s.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>

        <div className="flex min-h-0 flex-1">
          {/* 左侧分区导航 */}
          <nav
            aria-label="设置分类"
            className="hidden w-44 shrink-0 flex-col gap-1 overflow-y-auto border-r bg-sidebar p-3 sm:flex"
          >
            {SECTIONS.map((s) => (
              <Button
                key={s.id}
                variant={section === s.id ? 'secondary' : 'ghost'}
                className="h-10 w-full shrink-0 justify-start px-3"
                aria-current={section === s.id ? 'page' : undefined}
                onClick={() => setSection(s.id)}
              >
                <s.icon data-icon="inline-start" />
                <span className="truncate">{s.label}</span>
              </Button>
            ))}
            <div className="flex-1" />
          </nav>

          {/* 右侧内容 */}
          <ScrollArea className="min-w-0 flex-1" key={section}>
            <fieldset disabled={saving} className="min-w-0 border-0 p-5 sm:p-7">
              {section === 'connection' && <ConnectionPanel oai={oai} patchOai={patchOai} />}
              {section === 'tts' && <TtsPanel draft={draft} patch={patch} />}
              {section === 'preset' && (
                <PresetPanel oai={oai} applyToOai={(m) => patch('oai_settings', m)} />
              )}
              {section === 'prompts' && <PromptManagerPanel oai={oai} patchOai={patchOai} />}
              {section === 'regex' && <RegexPanel draft={draft} patch={patch} />}
              {section === 'commands' && <CustomCommandsPanel draft={draft} patch={patch} />}
              {section === 'plugins' && <PluginsPanel />}
              {section === 'sampling' && <SamplingPanel oai={oai} patchOai={patchOai} />}
              {section === 'ai-response' && <AiResponsePanel oai={oai} patchOai={patchOai} />}
              {section === 'persona' && <PersonaPanel draft={draft} patch={patch} />}
              {section === 'appearance' && <AppearancePanel />}
              {section === 'world-info' && <WorldInfoGlobalPanel wi={wi} patchWi={patchWi} />}
              {section === 'about' && <AboutPanel />}
            </fieldset>
          </ScrollArea>
        </div>
        <footer className="safe-bottom flex shrink-0 items-center justify-between gap-3 border-t bg-card px-5 pt-3">
          <span
            className={cn(
              'flex items-center gap-2 text-xs',
              dirty ? 'text-primary' : 'text-muted-foreground',
            )}
            role="status"
          >
            {dirty ? (
              <span className="size-1.5 rounded-full bg-primary" />
            ) : (
              <Check className="size-3.5" />
            )}
            {saving ? '正在保存…' : dirty ? '有尚未保存的更改' : '更改已保存'}
          </span>
          <Button disabled={!dirty || saving} onClick={() => void save()}>
            {saving ? <Spinner /> : <Save data-icon="inline-start" />}保存更改
          </Button>
        </footer>
      </SheetContent>
    </Sheet>
  );
}
