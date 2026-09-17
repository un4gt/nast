import { useEffect, useState } from 'react';
import { useTheme } from 'next-themes';
import { Label } from '@/components/ui/label';
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from '@/components/ui/select';
import { SliderField } from '../fields';

export function AppearancePanel() {
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">外观</h3>
        <p className="text-xs text-muted-foreground">主题与聊天区排版（localStorage 存储，即时生效）。</p>
      </div>
      <ThemeField />
      <FontScaleField />
      <ChatWidthField />
    </div>
  );
}

function ThemeField() {
  const { theme, setTheme } = useTheme();
  const [value, setValue] = useState(theme ?? 'dark');
  useEffect(() => {
    if (theme) setValue(theme);
  }, [theme]);
  return (
    <div className="flex flex-col gap-1.5">
      <Label>主题</Label>
      <Select value={value} onValueChange={(v) => setTheme(v)}>
        <SelectTrigger>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="dark">深色</SelectItem>
          <SelectItem value="light">浅色</SelectItem>
        </SelectContent>
      </Select>
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
  return <SliderField label="聊天区宽度 %" value={v} min={50} max={100} step={5} onChange={commit} />;
}
