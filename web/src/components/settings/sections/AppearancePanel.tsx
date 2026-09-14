import { SliderField } from '../fields';

export function AppearancePanel() {
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="mb-1 text-sm font-semibold">外观</h3>
        <p className="text-xs text-muted-foreground">聊天区排版（localStorage 存储，即时生效）。</p>
      </div>
      <FontScaleField />
      <ChatWidthField />
    </div>
  );
}

function FontScaleField() {
  const key = 'nast:font_scale';
  const get = () => Number(localStorage.getItem(key) ?? 100);
  const set = (v: number) => {
    localStorage.setItem(key, String(v));
    document.documentElement.style.setProperty('--chat-font-scale', String(v / 100));
  };
  return (
    <div className="flex flex-col gap-1">
      <LocalSlider label="字体缩放 %" initial={get()} min={75} max={150} step={5} onCommit={set} />
    </div>
  );
}

function ChatWidthField() {
  const key = 'nast:chat_width';
  const get = () => Number(localStorage.getItem(key) ?? 100);
  const set = (v: number) => {
    localStorage.setItem(key, String(v));
    document.documentElement.style.setProperty('--chat-width', `${v}%`);
  };
  return (
    <div className="flex flex-col gap-1">
      <LocalSlider label="聊天区宽度 %" initial={get()} min={50} max={100} step={5} onCommit={set} />
    </div>
  );
}

function LocalSlider({
  label,
  initial,
  min,
  max,
  step,
  onCommit,
}: {
  label: string;
  initial: number;
  min: number;
  max: number;
  step: number;
  onCommit: (v: number) => void;
}) {
  const [v, setV] = [initial, (n: number) => onCommit(n)] as const;
  void setV;
  return <SliderField label={label} value={v} min={min} max={max} step={step} onChange={onCommit} />;
}
