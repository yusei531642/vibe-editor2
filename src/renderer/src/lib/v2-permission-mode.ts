import type { RuntimePermission } from '../../../types/agent-runtime';
import type { V2PermissionMode } from '../../../types/shared';

/**
 * 会話 UI の3モードを native runtime 契約へ変換する。
 * Team endpoint は backend 側で常に workspace 上限へ制約されるため、この変換は通常会話用。
 */
export function runtimePermissionForMode(mode: V2PermissionMode): RuntimePermission {
  if (mode === 'full') return 'full';
  if (mode === 'ask') return 'ask';
  return 'workspace';
}
