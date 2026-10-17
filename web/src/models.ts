import { useStore } from './store';

export interface Route {
  channel_id: string;
  id: string; provider: string; protocol: string; upstream_model: string; priority: number; enabled: boolean;
  credential_configured?: boolean;
  config: { endpoint: string; credential_ref?: string | null; context_limit?: number | null; input_limit?: number | null; output_limit?: number | null; supports_images?: boolean | null;
    connect_timeout_secs: number; first_token_timeout_secs: number; idle_timeout_secs: number;
    headers: Record<string, string>; parameters: Record<string, unknown>; remove_parameters: string[] };
}
export interface LogicalModel { id: string; display_name: string; routes: Route[] }
export interface Channel { id: string; name: string; protocol: string; endpoint: string; credential_ref?: string | null; credential_configured?: boolean; headers: Record<string, string>; enabled: boolean; discovered_models: string[] }
export interface Catalog { schema_version: number; version: number; default_model: string; models: LogicalModel[]; channels: Channel[] }
export function currentConversation() {
  const state = useStore.getState();
  if (state.activeGroupId) {
    const group = state.groups.find(g => g.id === state.activeGroupId);
    return group ? { kind: 'group', group_id: group.id, chat_id: group.chat_id } : null;
  }
  return state.activeAvatar && state.activeChatName ? { kind: 'private', avatar: state.activeAvatar, chat_file: state.activeChatName } : null;
}
