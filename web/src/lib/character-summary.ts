/** characters.all also returns { avatar, error } when a card cannot be read. */
export interface CharacterListEntry {
  avatar: string;
  name?: string;
  description?: string;
  tags?: string[];
  fav?: boolean;
  chat?: string | null;
  error?: string;
}

export interface CharacterSummary {
  avatar: string;
  avatarUrl: string;
  name: string;
  description: string;
  tags: string[];
  fav: boolean;
  chat: string | null;
  error?: string;
}

export function normalizeCharacterSummary(card: CharacterListEntry): CharacterSummary {
  const hasName = typeof card.name === 'string' && card.name.trim().length > 0;
  return {
    avatar: card.avatar,
    avatarUrl: `/thumbnail?file=${encodeURIComponent(card.avatar)}`,
    name: hasName ? card.name! : card.avatar.replace(/\.png$/i, '') || '未命名角色',
    description: typeof card.description === 'string' ? card.description : '',
    tags: Array.isArray(card.tags) ? card.tags.filter((tag) => typeof tag === 'string') : [],
    fav: card.fav === true,
    chat: typeof card.chat === 'string' ? card.chat : null,
    error: card.error || (hasName ? undefined : '角色卡缺少名称'),
  };
}
