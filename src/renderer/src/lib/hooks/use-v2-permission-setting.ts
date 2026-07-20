import { useCallback } from 'react';
import type { RuntimePermission, V2PermissionMode } from '../../../../types/agent-runtime';
import { useSettings } from '../settings-context';
import { runtimePermissionForMode } from '../v2-permission-mode';

export function useV2PermissionSetting(): {
  permission: V2PermissionMode;
  runtimePermission: RuntimePermission;
  setPermission: (permission: V2PermissionMode) => void;
} {
  const { settings, update } = useSettings();
  const permission = settings.v2PermissionMode;
  const setPermission = useCallback((next: V2PermissionMode): void => {
    void update({ v2PermissionMode: next });
  }, [update]);
  return { permission, runtimePermission: runtimePermissionForMode(permission), setPermission };
}
