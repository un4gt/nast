// Canonical ST settings with a read-only fallback for earlier nast data.
export function worldInfoView(settings: any): Record<string, any> {
  const legacy = settings?.world_info ?? {};
  const canonical = settings?.world_info_settings ?? {};
  const selection = canonical.world_info ?? {};
  return {
    ...legacy, ...canonical,
    global_select: selection.globalSelect ?? canonical.globalSelect ?? legacy.global_select ?? [],
    char_lore: selection.charLore ?? canonical.charLore ?? legacy.char_lore ?? [],
  };
}

export function worldInfoPath(key: string): string {
  if (key === 'global_select') return 'world_info_settings.world_info.globalSelect';
  if (key === 'char_lore') return 'world_info_settings.world_info.charLore';
  return `world_info_settings.${key}`;
}

export function withCharacterBooks(settings: any, name: string, extraBooks: string[]) {
  const lore = [...worldInfoView(settings).char_lore].filter((item) => item.name !== name);
  if (extraBooks.length) lore.push({ name, extraBooks });
  return {
    ...settings,
    world_info_settings: {
      ...settings.world_info_settings,
      world_info: { ...settings.world_info_settings?.world_info, charLore: lore },
    },
  };
}
