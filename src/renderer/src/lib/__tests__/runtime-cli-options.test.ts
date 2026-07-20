import { describe, expect, it } from 'vitest';
import { appendRuntimeCliOptions } from '../runtime-cli-options';

describe('appendRuntimeCliOptions permission modes', () => {
  it('Codex ask mode を workspace-write + untrusted へ変換する', () => {
    const args: string[] = [];
    appendRuntimeCliOptions(args, 'codex', undefined, undefined, 'ask');
    expect(args).toEqual([
      '--sandbox', 'workspace-write',
      '--ask-for-approval', 'untrusted'
    ]);
  });
});
