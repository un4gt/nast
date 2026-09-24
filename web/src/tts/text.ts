import type { TtsSettings } from './config';

export type Segment = { type: 'dialogue' | 'action' | 'other'; text: string };
const pairs: Record<string, string> = { '„': '“', '“': '”', '«': '»', '»': '«', '‘': '’', '‚': '‘', '「': '」', '『': '』', '"': '"', '＂': '＂' };

// Same outermost-pair matching and no-quotes fallback as ST joinQuotedBlocks.
export function quotedBlocks(text: string): string[] {
  const stack: { end: string; start: number }[] = [];
  const result: string[] = [];
  for (let i = 0; i < text.length; i++) {
    if (stack.length && text[i] === stack[stack.length - 1].end) {
      const closed = stack.pop()!;
      if (!stack.length) result.push(text.slice(closed.start, i + 1));
    } else if (pairs[text[i]]) stack.push({ end: pairs[text[i]], start: i });
  }
  return result;
}
export function parseFilter(pattern: string): RegExp {
  const match = pattern.match(/^\/(.*)\/([dgimsuvy]*)$/s);
  return match ? new RegExp(match[1], match[2]) : new RegExp(pattern, 'g');
}
export function splitLongText(text: string, max: number): string[] {
  const chars = Array.from(text);
  const output: string[] = [];
  while (chars.length > max) {
    let end = max;
    for (let i = max - 1; i >= Math.floor(max / 2); i--) {
      if (/[\s。！？.!?;；]/u.test(chars[i])) { end = i + 1; break; }
    }
    output.push(chars.splice(0, end).join('').trim());
  }
  if (chars.length) output.push(chars.join('').trim());
  return output.filter(Boolean);
}
export function prepareText(raw: string, tts: TtsSettings, name: string, user: string, allowName = false): Segment[] {
  let text = raw.replace(/\{\{char\}\}/gi, () => name).replace(/\{\{user\}\}/gi, () => user);
  // Reasoning can arrive inline during streaming; it is never speech content.
  text = text.replace(/<(think|analysis)>[\s\S]*?(?:<\/\1>|$)/gi, '');
  if (tts.skip_codeblocks) text = text.replace(/```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)/g, '');
  if (tts.skip_tags) text = text.replace(/<([\w-]+)\b[^>]*>[\s\S]*?<\/\1\s*>/g, '').replace(/<[^>]+\/>/g, '');
  if (!tts.pass_asterisks) text = tts.narrate_dialogues_only ? text.replace(/\*[^*]*?(?:\*|$)/g, '') : text.replaceAll('*', '');
  if (tts.apply_regex && tts.regex_pattern) text = text.replace(parseFilter(tts.regex_pattern), '');
  if (tts.narrate_quoted_only) {
    const quotes = quotedBlocks(text);
    if (quotes.length) text = quotes.join(tts.currentProvider === 'Kokoro' ? ' ... ... ... ' : ' ... ');
  }
  text = text.replace(/!\[.*?\]\([^)]*\)/g, '');
  if (tts.currentProvider === 'Kokoro') text = text.replace(/~/g, '.');
  if (tts.currentProvider === 'Volcengine') text = text.replaceAll('...', '');
  if (!allowName && name) text = text.replace(new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}:`, 'gm'), '');
  const paragraphs = tts.narrate_by_paragraphs ? text.split(/\n+/) : [text];
  return paragraphs.flatMap((paragraph): Segment[] => {
    const content = paragraph.replace(/\s+/g, ' ').trim();
    if (!content) return [];
    if (!tts.multi_voice_enabled) return [{ type: 'other', text: content }];
    const matches = /\*([^*]*?)\*|"(.*?)"|“(.*?)”|«(.*?)»|「(.*?)」|『(.*?)』|＂(.*?)＂/g;
    const output: Segment[] = [];
    let last = 0;
    for (const match of content.matchAll(matches)) {
      const at = match.index!;
      if (at > last) output.push({ type: 'other', text: content.slice(last, at).trim() });
      output.push({ type: match[1] !== undefined ? 'action' : 'dialogue', text: match[0].slice(1, -1).trim() });
      last = at + match[0].length;
    }
    if (last < content.length) output.push({ type: 'other', text: content.slice(last).trim() });
    return output.filter((part) => part.text);
  });
}

// Hold partial fenced blocks, tags and quotes until a complete paragraph exists.
export function streamingBoundary(text: string, tts: TtsSettings): number {
  const end = text.lastIndexOf('\n') + 1;
  if (!end) return 0;
  const part = text.slice(0, end);
  if ((part.match(/```/g)?.length ?? 0) % 2 || (part.match(/~~~/g)?.length ?? 0) % 2) return 0;
  const openTags: string[] = [];
  for (const match of part.matchAll(/<(\/?)([\w-]+)\b[^>]*>/g)) {
    if (!tts.skip_tags && !['think', 'analysis'].includes(match[2].toLowerCase())) continue;
    if (/\/>$/.test(match[0]) || ['br', 'hr', 'img'].includes(match[2])) continue;
    if (match[1]) { if (openTags[openTags.length - 1] === match[2]) openTags.pop(); }
    else openTags.push(match[2]);
  }
  if (openTags.length) return 0;
  if (tts.multi_voice_enabled || tts.narrate_quoted_only) {
    const stack: string[] = [];
    for (const char of part) {
      if (stack.length && stack[stack.length - 1] === char) stack.pop();
      else if (pairs[char]) stack.push(pairs[char]);
    }
    if (stack.length || (part.match(/\*/g)?.length ?? 0) % 2) return 0;
  }
  return end;
}
