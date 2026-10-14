import { useEffect, useState } from 'react';
import { worldInfoPath, worldInfoView } from '@/lib/world-settings';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Spinner } from '@/components/ui/spinner';
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
  ChevronLeft,
  ChevronRight,
} from 'lucide-react';
import { ModelsPanel } from './sections/ModelsPanel';
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
  { id: 'connection', icon: Plug, label: '模型', group: '对话', hint: '逻辑模型、路由与凭据' },
  { id: 'preset', icon: FolderOpen, label: '预设', group: '对话', hint: '保存与切换整套生成配置' },
  { id: 'prompts', icon: ListOrdered, label: '提示词管理', group: '对话', hint: '提示词顺序与内容' },
  { id: 'sampling', icon: SlidersHorizontal, label: '采样参数', group: '对话', hint: '温度、Top P 与惩罚项' },
  { id: 'ai-response', icon: Sparkles, label: 'AI 回复', group: '对话', hint: '上下文、长度与流式输出' },
  { id: 'regex', icon: Regex, label: '正则脚本', group: '对话', hint: '对输入输出做文本替换' },
  { id: 'persona', icon: UserCircle, label: '用户 / Persona', group: '角色与世界', hint: '你在故事里的身份' },
  { id: 'world-info', icon: BookOpen, label: '世界书全局', group: '角色与世界', hint: '全局启用的书与扫描规则' },
  { id: 'appearance', icon: Palette, label: '外观', group: '体验', hint: '主题、字号与聊天宽度' },
  { id: 'tts', icon: Headphones, label: '语音朗读', group: '体验', hint: '朗读服务与音色' },
  { id: 'commands', icon: TerminalSquare, label: '自定义命令', group: '扩展', hint: '快捷斜杠命令' },
  { id: 'plugins', icon: Puzzle, label: '插件', group: '扩展', hint: '已加载的扩展脚本' },
  { id: 'about', icon: Info, label: '关于', group: '扩展', hint: '版本与运行信息' },
] as const;
const GROUPS = ['对话', '角色与世界', '体验', '扩展'] as const;

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
  // 移动端：先显示分类列表，选择后进入分区
  const [mobileList, setMobileList] = useState(true);

  useEffect(() => {
    if (open && initialSection && SECTIONS.some((s) => s.id === initialSection)) { setSection(initialSection as SectionId); setMobileList(false); }
    else if (open && !initialSection) setMobileList(true);
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

  const current = SECTIONS.find((x) => x.id === section)!;
  const nav = (
    <>
      {GROUPS.map((group) => (
        <div key={group} className="flex flex-col gap-0.5">
          <p className="px-3 pb-1 pt-3 text-[11px] font-medium tracking-wide text-muted-foreground/80">{group}</p>
          {SECTIONS.filter((x) => x.group === group).map((s) => (
            <button
              key={s.id}
              type="button"
              aria-current={section === s.id ? 'page' : undefined}
              onClick={() => { setSection(s.id); setMobileList(false); }}
              className={cn(
                'group/nav flex h-control-md w-full shrink-0 items-center gap-2.5 rounded-lg px-3 text-left text-sm transition-colors max-sm:h-control-lg',
                section === s.id ? 'bg-card font-medium text-foreground shadow-sm ring-1 ring-border/70 sm:[&_svg]:text-primary' : 'text-muted-foreground hover:bg-card/60 hover:text-foreground',
              )}
            >
              <s.icon className="size-4 shrink-0" />
              <span className="flex-1 truncate">{s.label}</span>
              <ChevronRight className="size-4 shrink-0 opacity-50 sm:hidden" />
            </button>
          ))}
        </div>
      ))}
    </>
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-dvh max-h-dvh w-full max-w-full flex-col gap-0 overflow-hidden rounded-none p-0 sm:h-[min(92dvh,900px)] sm:w-[calc(100%-2rem)] sm:max-w-6xl sm:rounded-2xl sm:p-0">
        <DialogHeader className="sr-only">
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>调整模型、角色与偏好，让对话更合心意。</DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1">
          {/* 左侧分区导航；移动端作为分类列表页 */}
          <nav
            aria-label="设置分类"
            className={cn(
              'w-full shrink-0 flex-col overflow-y-auto bg-muted/30 p-3 sm:flex sm:w-60 sm:border-r sm:border-border/70',
              mobileList ? 'flex' : 'hidden',
            )}
          >
            <div className="px-3 pb-1 pt-2">
              <p className="font-display text-2xl font-semibold">设置</p>
              <p className="mt-0.5 text-xs text-muted-foreground">让每次对话都更合心意</p>
            </div>
            {nav}
          </nav>

          {/* 右侧内容 */}
          <div className={cn('min-w-0 flex-1 flex-col', mobileList ? 'hidden sm:flex' : 'flex')}>
            <header className="flex shrink-0 items-center gap-3 border-b border-border/70 px-4 py-4 pr-12 sm:px-7">
              <Button variant="ghost" size="icon-sm" className="sm:hidden" aria-label="返回设置分类" onClick={() => setMobileList(true)}>
                <ChevronLeft />
              </Button>
              <div className="hidden size-9 items-center justify-center rounded-xl bg-accent text-primary sm:flex"><current.icon className="size-4.5" /></div>
              <div className="min-w-0">
                <h2 className="truncate font-display text-lg font-semibold leading-tight">{current.label}</h2>
                <p className="truncate text-xs text-muted-foreground">{current.hint}</p>
              </div>
            </header>
            <ScrollArea className="min-h-0 min-w-0 flex-1">
              <fieldset disabled={saving} className="mx-auto min-w-0 max-w-3xl border-0 p-5 sm:p-7">
                <div hidden={section !== 'connection'}><ModelsPanel /></div>
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
            {section !== 'connection' && <footer className="safe-bottom flex shrink-0 items-center justify-between gap-3 border-t border-border/70 bg-card/60 px-5 pt-3 sm:px-7">
              <span
                className={cn(
                  'flex items-center gap-2 text-xs',
                  dirty ? 'text-warning' : 'text-muted-foreground',
                )}
                role="status"
              >
                {dirty ? (
                  <span className="size-1.5 rounded-full bg-warning" />
                ) : (
                  <Check className="size-3.5" />
                )}
                {saving ? '正在保存…' : dirty ? '有尚未保存的更改' : '更改已保存'}
              </span>
              <Button disabled={!dirty || saving} onClick={() => void save()}>
                {saving ? <Spinner /> : <Save data-icon="inline-start" />}保存更改
              </Button>
            </footer>}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
