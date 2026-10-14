import { useState } from 'react';
import { useTheme } from 'next-themes';
import { Label } from '@/components/ui/label';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { Moon, Sun } from 'lucide-react';
import { SliderField } from '../fields';

export function AppearancePanel() {
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">外观</h3>
        <p className="text-xs text-muted-foreground">
          选择舒适的阅读方式。外观调整即时生效，并在此设备上保留。
        </p>
      </div>
      <ThemeField />
      <FontScaleField />
      <ChatWidthField />
    </div>
  );
}

function ThemeField() {
  const { theme, setTheme } = useTheme();
  return (
    <div className="flex flex-col gap-3">
      <Label>主题</Label>
      <ToggleGroup
        type="single"
        variant="outline"
        value={theme ?? 'dark'}
        onValueChange={(value) => {
          if (value) setTheme(value);
        }}
        className="grid grid-cols-2 gap-3"
        aria-label="界面主题"
      >
        <ToggleGroupItem
          value="light"
          className="h-auto flex-col items-stretch gap-2.5 rounded-xl p-2.5 data-[state=on]:border-primary data-[state=on]:bg-accent/60 data-[state=on]:ring-3 data-[state=on]:ring-primary/15"
          aria-label="浅色主题"
        >
          <span aria-hidden className="flex h-16 gap-1.5 rounded-lg border border-black/5 p-1.5" style={{ background: 'hsl(36 38% 96%)' }}>
            <span className="w-1/4 rounded-md" style={{ background: 'hsl(38 45% 99%)' }} />
            <span className="flex flex-1 flex-col justify-end gap-1 rounded-md p-1.5" style={{ background: 'hsl(38 45% 99%)' }}>
              <span className="h-1.5 w-3/4 rounded-full opacity-30" style={{ background: 'hsl(24 72% 46%)' }} />
              <span className="h-2.5 w-1/3 self-end rounded-full" style={{ background: 'hsl(24 72% 46%)' }} />
            </span>
          </span>
          <span className="flex items-center gap-1.5 px-0.5 text-sm"><Sun className="size-4" />Warm Light</span>
        </ToggleGroupItem>
        <ToggleGroupItem
          value="dark"
          className="h-auto flex-col items-stretch gap-2.5 rounded-xl p-2.5 data-[state=on]:border-primary data-[state=on]:bg-accent/60 data-[state=on]:ring-3 data-[state=on]:ring-primary/15"
          aria-label="深色主题"
        >
          <span aria-hidden className="flex h-16 gap-1.5 rounded-lg border border-black/5 p-1.5" style={{ background: 'hsl(24 16% 8%)' }}>
            <span className="w-1/4 rounded-md" style={{ background: 'hsl(24 14% 13%)' }} />
            <span className="flex flex-1 flex-col justify-end gap-1 rounded-md p-1.5" style={{ background: 'hsl(24 14% 13%)' }}>
              <span className="h-1.5 w-3/4 rounded-full opacity-30" style={{ background: 'hsl(32 78% 62%)' }} />
              <span className="h-2.5 w-1/3 self-end rounded-full" style={{ background: 'hsl(32 78% 62%)' }} />
            </span>
          </span>
          <span className="flex items-center gap-1.5 px-0.5 text-sm"><Moon className="size-4" />Warm Dark</span>
        </ToggleGroupItem>
      </ToggleGroup>
    </div>
  );
}

function FontScaleField() {
  const key = 'nast:font_scale';
  const [v, setV] = useState(() => Number(localStorage.getItem(key) ?? 100));
  const commit = (n: number) => {
    setV(n);
    localStorage.setItem(key, String(n));
    document.documentElement.style.setProperty('--chat-font-scale', String(n / 100));
  };
  return <SliderField label="字体缩放 %" value={v} min={75} max={150} step={5} onChange={commit} />;
}

function ChatWidthField() {
  const key = 'nast:chat_width';
  const [v, setV] = useState(() => Number(localStorage.getItem(key) ?? 100));
  const commit = (n: number) => {
    setV(n);
    localStorage.setItem(key, String(n));
    document.documentElement.style.setProperty('--chat-width', `${n}%`);
  };
  return (
    <SliderField label="聊天区宽度 %" value={v} min={50} max={100} step={5} onChange={commit} />
  );
}
