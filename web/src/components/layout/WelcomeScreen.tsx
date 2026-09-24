import { useRef, useState } from 'react';
import { ArrowUpRight, MessageCircle, Plug, Upload } from 'lucide-react';
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { Button } from '@/components/ui/button';
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from '@/components/ui/empty';
import { Spinner } from '@/components/ui/spinner';
import { useStore } from '../../store';

export function WelcomeScreen({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { characters, connected, importFile, selectCharacter } = useStore();
  const fileRef = useRef<HTMLInputElement>(null);
  const [importing, setImporting] = useState(false);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="m-auto w-full max-w-2xl px-5 py-10 sm:px-10">
        <Empty className="flex-none gap-7 px-0 py-0 md:p-0">
          <div className="welcome-mark">
            <MessageCircle className="size-8" strokeWidth={1.5} />
          </div>
          <EmptyHeader className="max-w-md gap-3">
            <p className="text-xs font-medium tracking-[0.2em] text-primary">NOT A SILLYTAVERN</p>
            <EmptyTitle className="text-2xl font-semibold tracking-tight sm:text-3xl">
              故事，从一次对话开始。
            </EmptyTitle>
            <EmptyDescription>
              带上你喜欢的角色，继续未完的故事。
              <br />
              选择已有角色，或导入一张角色卡，开启新的篇章。
            </EmptyDescription>
          </EmptyHeader>
          <EmptyContent className="max-w-md">
            <div className="flex flex-wrap justify-center gap-3">
              <Button
                className="h-10"
                disabled={!connected || importing}
                onClick={() => fileRef.current?.click()}
              >
                {importing ? <Spinner /> : <Upload data-icon="inline-start" />}
                {importing ? '正在导入' : '导入角色卡'}
              </Button>
              <Button variant="outline" className="h-10" onClick={onOpenSettings}>
                <Plug data-icon="inline-start" />
                连接与设置
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              也可以拖入 PNG / JSON 文件，兼容 SillyTavern 角色卡
            </p>
          </EmptyContent>
        </Empty>
        {characters.length > 0 && (
          <section className="mt-12 flex flex-col gap-3" aria-label="选择角色">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>与你的角色继续</span>
              <span>{characters.length} 个角色</span>
            </div>
            <div className="grid gap-2 sm:grid-cols-2">
              {characters.slice(0, 4).map((character) => (
                <Button
                  key={character.avatar}
                  variant="outline"
                  className="h-auto min-w-0 justify-start gap-3 p-3"
                  disabled={!connected || Boolean(character.error)}
                  title={character.error}
                  onClick={() => void selectCharacter(character.avatar).catch(() => {})}
                >
                  <Avatar className="size-10 shrink-0">
                    <AvatarImage src={character.avatarUrl} alt={character.name} />
                    <AvatarFallback>{character.name.slice(0, 1)}</AvatarFallback>
                  </Avatar>
                  <span className="flex min-w-0 flex-1 flex-col gap-1 text-left">
                    <span className="truncate">{character.name}</span>
                    <span className="truncate text-xs font-normal text-muted-foreground">
                      {character.error ? '读取失败' : character.tags.slice(0, 2).join(' · ') || '开始对话'}
                    </span>
                  </span>
                  <ArrowUpRight data-icon="inline-end" />
                </Button>
              ))}
            </div>
          </section>
        )}
      </div>
      <input
        ref={fileRef}
        type="file"
        accept=".png,.json"
        multiple
        hidden
        onChange={async (e) => {
          const files = Array.from(e.target.files ?? []);
          e.target.value = '';
          setImporting(true);
          await Promise.allSettled(files.map(importFile));
          setImporting(false);
        }}
      />
    </div>
  );
}
