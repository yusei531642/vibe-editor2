import { describe, expect, it } from 'vitest';
import { runtimePermissionForMode } from '../v2-permission-mode';

describe('runtimePermissionForMode', () => {
  it('会話の3モードを native runtime permission へ変換する', () => {
    expect(runtimePermissionForMode('full')).toBe('full');
    expect(runtimePermissionForMode('agent')).toBe('workspace');
    expect(runtimePermissionForMode('ask')).toBe('ask');
  });
});
