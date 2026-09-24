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
          className="h-20 flex-col gap-2 rounded-xl"
          aria-label="浅色主题"
        >
          <Sun className="size-5" />
          浅色
        </ToggleGroupItem>
        <ToggleGroupItem
          value="dark"
          className="h-20 flex-col gap-2 rounded-xl"
          aria-label="深色主题"
        >
          <Moon className="size-5" />
          深色
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
