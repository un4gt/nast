import { useStore } from './store';

export interface Route {
  id: string; provider: string; protocol: string; upstream_model: string; priority: number; enabled: boolean;
  credential_configured?: boolean;
  config: { endpoint: string; credential_ref?: string | null; context_limit?: number | null; output_limit?: number | null;
    connect_timeout_secs: number; first_token_timeout_secs: number; idle_timeout_secs: number;
    headers: Record<string, string>; parameters: Record<string, unknown>; remove_parameters: string[] };
}
export interface LogicalModel { id: string; display_name: string; routes: Route[] }
export interface Catalog { version: number; default_model: string; models: LogicalModel[] }
export function currentConversation() {
  const state = useStore.getState();
  if (state.activeGroupId) {
    const group = state.groups.find(g => g.id === state.activeGroupId);
    return group ? { kind: 'group', group_id: group.id, chat_id: group.chat_id } : null;
  }
  return state.activeAvatar && state.activeChatName ? { kind: 'private', avatar: state.activeAvatar, chat_file: state.activeChatName } : null;
}
