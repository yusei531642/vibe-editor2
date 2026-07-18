import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RuntimeEventEnvelope } from '../../../../../types/agent-runtime';
import { useV2RuntimeSession } from '../use-v2-runtime-session';

const ENDPOINT_ID = 'v2-claude-00000000-0000-4000-8000-000000000000';

function envelope(payload: RuntimeEventEnvelope['payload'], sequence: number): RuntimeEventEnvelope {
  return {
    endpointId: ENDPOINT_ID,
    epoch: 1,
    sequence,
    kind: payload.type,
    payload,
    timestamp: '2026-07-18T00:00:00Z'
  };
}

describe('useV2RuntimeSession', () => {
  let onEvent: ((event: RuntimeEventEnvelope) => void) | null;
  const registerClaudeEndpoint = vi.fn(async () => ({ endpointId: ENDPOINT_ID }));
  const spawnTurn = vi.fn(async () => ({ endpointId: ENDPOINT_ID }));
  const dispose = vi.fn(async () => ({ endpointId: ENDPOINT_ID }));
  const respondApproval = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    onEvent = null;
    vi.spyOn(crypto, 'randomUUID').mockReturnValue('00000000-0000-4000-8000-000000000000');
    Object.defineProperty(window, 'api', {
      configurable: true,
      value: {
        agentRuntime: {
          onEventReady: vi.fn(async (_endpointId, callback) => {
            onEvent = callback;
            return vi.fn();
          }),
          registerClaudeEndpoint,
          registerCodexEndpoint: vi.fn(),
          spawnTurn,
          interrupt: vi.fn(async () => ({ endpointId: ENDPOINT_ID })),
          respondApproval,
          dispose
        }
      }
    });
  });

  it('選択した model/effort/permission で実 endpoint を起動し stream 完了を投影する', async () => {
    const onDelta = vi.fn();
    const onComplete = vi.fn();
    const onError = vi.fn();
    const { result, unmount } = renderHook(() => useV2RuntimeSession({
      onDelta,
      onComplete,
      onError
    }));

    await act(async () => {
      await result.current.send({
        input: '実装して',
        engine: 'claude',
        model: 'fable',
        effort: 'max',
        permission: 'full'
      });
    });

    expect(registerClaudeEndpoint).toHaveBeenCalledWith(expect.objectContaining({
      model: 'fable', effort: 'max', permission: 'full'
    }));
    expect(spawnTurn).toHaveBeenCalledWith(expect.objectContaining({
      input: '実装して', model: 'fable', effort: 'max', permission: 'full'
    }));
    expect(result.current.running).toBe(true);

    act(() => {
      onEvent?.(envelope({ type: 'messageDelta', delta: '途中' }, 1));
      onEvent?.(envelope({ type: 'messageComplete', message: '完了' }, 2));
      onEvent?.(envelope({ type: 'turnComplete', interrupted: false }, 3));
    });
    expect(onDelta).toHaveBeenCalledWith('途中', 'claude');
    expect(onComplete).toHaveBeenCalledWith('完了', 'claude');
    await waitFor(() => expect(result.current.running).toBe(false));
    expect(onError).not.toHaveBeenCalled();

    unmount();
    await waitFor(() => expect(dispose).toHaveBeenCalled());
  });

  it('先の承認応答中に届いた新しい承認要求を消さない', async () => {
    let finishResponse!: () => void;
    respondApproval.mockImplementationOnce(() => new Promise<void>((resolve) => {
      finishResponse = resolve;
    }));
    const { result } = renderHook(() => useV2RuntimeSession({
      onDelta: vi.fn(), onComplete: vi.fn(), onError: vi.fn()
    }));
    await act(async () => {
      await result.current.send({
        input: '実装して', engine: 'claude', model: 'fable', effort: 'high', permission: 'workspace'
      });
    });
    act(() => {
      onEvent?.(envelope({
        type: 'approvalRequest', requestId: 'approval-a', method: 'Bash', reason: 'first',
        command: 'npm test', cwd: null
      }, 1));
    });
    expect(result.current.pendingApproval?.requestId).toBe('approval-a');

    let response: Promise<void>;
    act(() => {
      response = result.current.respondApproval('accept');
    });
    act(() => {
      onEvent?.(envelope({
        type: 'approvalRequest', requestId: 'approval-b', method: 'Bash', reason: 'second',
        command: 'npm run build:vite', cwd: null
      }, 2));
    });
    expect(result.current.pendingApproval?.requestId).toBe('approval-b');

    finishResponse();
    await act(async () => { await response; });
    expect(result.current.pendingApproval?.requestId).toBe('approval-b');
  });

  it('runtime error 後に応答不能な承認要求を残さない', async () => {
    const onError = vi.fn();
    const { result } = renderHook(() => useV2RuntimeSession({
      onDelta: vi.fn(), onComplete: vi.fn(), onError
    }));
    await act(async () => {
      await result.current.send({
        input: '実装して', engine: 'claude', model: 'fable', effort: 'high', permission: 'workspace'
      });
    });
    act(() => {
      onEvent?.(envelope({
        type: 'approvalRequest', requestId: 'approval-a', method: 'Bash', reason: 'confirm',
        command: 'npm test', cwd: null
      }, 1));
    });
    expect(result.current.pendingApproval?.requestId).toBe('approval-a');

    act(() => {
      onEvent?.(envelope({
        type: 'error', code: 'runtime_failed', message: 'runtime failed', recoverable: true
      }, 2));
    });

    expect(onError).toHaveBeenCalledWith('runtime failed', 'claude');
    expect(result.current.running).toBe(false);
    expect(result.current.pendingApproval).toBeNull();
  });

  it('endpoint 終了後の次回送信で binding を再登録する', async () => {
    const { result } = renderHook(() => useV2RuntimeSession({
      onDelta: vi.fn(), onComplete: vi.fn(), onError: vi.fn()
    }));
    await act(async () => {
      await result.current.send({
        input: 'first', engine: 'claude', model: 'fable', effort: 'high', permission: 'workspace'
      });
    });
    expect(registerClaudeEndpoint).toHaveBeenCalledTimes(1);

    act(() => {
      onEvent?.(envelope({ type: 'lifecycle', state: 'failed', detail: 'sidecar crashed' }, 1));
    });

    await act(async () => {
      await result.current.send({
        input: 'retry', engine: 'claude', model: 'fable', effort: 'high', permission: 'workspace'
      });
    });
    expect(registerClaudeEndpoint).toHaveBeenCalledTimes(2);
    expect(spawnTurn).toHaveBeenLastCalledWith(expect.objectContaining({ input: 'retry' }));
  });
});
