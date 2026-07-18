import { describe, expect, it, vi } from 'vitest';
import { reportV2RuntimeActionError } from '../v2-runtime-action';

describe('reportV2RuntimeActionError', () => {
  it('runtime action の reject を会話エラーへ変換する', async () => {
    const onError = vi.fn();

    await reportV2RuntimeActionError(
      Promise.reject(new Error('runtime_approval_not_pending')),
      'claude',
      onError
    );

    expect(onError).toHaveBeenCalledWith('runtime_approval_not_pending', 'claude');
  });
});
