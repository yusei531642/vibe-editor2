import { describe, expect, it } from 'vitest';
// @ts-expect-error The production sidecar is intentionally plain ESM for Node execution.
import { claudePermissionMode, shouldAutoAllowTool } from '../../../../../src-sidecars/claude-agent/tool-approval-policy.mjs';

describe('shouldAutoAllowTool', () => {
  it.each([
    'team_info',
    'team_read',
    'team_get_tasks',
    'team_list_role_profiles',
    'team_diagnostics'
  ])('workspace で照会・自己報告系 %s を許可する', (tool) => {
    expect(shouldAutoAllowTool(`mcp__vibe-team2__${tool}`, 'workspace')).toBe(true);
  });

  it.each([
    'team_recruit',
    'team_dismiss',
    'team_send',
    'team_status',
    'team_report',
    'team_update_task',
    'team_assign_task',
    'team_lock_files',
    'team_unlock_files'
  ])('workspace で変更・注入系 %s を承認対象にする', (tool) => {
    expect(shouldAutoAllowTool(`mcp__vibe-team2__${tool}`, 'workspace')).toBe(false);
  });

  it('full permission では Team ツールを明示的に許可する', () => {
    expect(shouldAutoAllowTool('mcp__vibe-team2__team_recruit', 'full')).toBe(true);
  });

  it('Team 以外のツールを名前空間だけで許可しない', () => {
    expect(shouldAutoAllowTool('Bash', 'full')).toBe(false);
  });

  it('3つの会話権限を Claude SDK permissionMode へ変換する', () => {
    expect(claudePermissionMode('full')).toBe('bypassPermissions');
    expect(claudePermissionMode('workspace')).toBe('acceptEdits');
    expect(claudePermissionMode('ask')).toBe('default');
  });

  it('ask では Team の読み取りツールも自動許可しない', () => {
    expect(shouldAutoAllowTool('mcp__vibe-team2__team_info', 'ask')).toBe(false);
  });
});
