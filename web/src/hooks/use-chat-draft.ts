import { useCallback, useState } from 'react';

/** 视图按会话 key 挂载；同步写入原 key，异步失败回填也不会覆盖别的会话。 */
export function useChatDraft(key: string) {
  const [value, setValue] = useState(() => localStorage.getItem(key) ?? '');
  const update = useCallback(
    (next: string) => {
      if (next) localStorage.setItem(key, next);
      else localStorage.removeItem(key);
      setValue(next);
    },
    [key],
  );
  return [value, update] as const;
}
