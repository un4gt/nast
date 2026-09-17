// 显示侧文本处理（对齐 ST messageFormatting 的 fixMarkdown(text, true)，
// power-user.js:429-469）：仅渲染前补齐奇数个 * / "，不落盘。

/** 逐行补齐奇数个 * / "（渲染前调用）。 */
export function fixMarkdownQuotes(text: string): string {
  return text
    .split('\n')
    .map((line) => {
      let out = line;
      for (const ch of ['*', '"'] as const) {
        const count = out.split(ch).length - 1;
        if (count % 2 === 1) {
          out = out.trimEnd() + ch;
        }
      }
      return out;
    })
    .join('\n');
}
