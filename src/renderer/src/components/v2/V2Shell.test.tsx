import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { V2Shell } from './V2Shell';

const harness = vi.hoisted(() => ({
  launchTeam: vi.fn(), addCard: vi.fn(), selectTeam: vi.fn(),
  settings: { v2PermissionMode: 'agent' as 'full' | 'agent' | 'ask' },
  updateSettings: vi.fn().mockResolvedValue(undefined),
  runtime: {
    running: false, pendingApproval: null, send: vi.fn().mockResolvedValue(undefined),
    stop: vi.fn().mockResolvedValue(undefined), reset: vi.fn().mockResolvedValue(undefined),
    respondApproval: vi.fn().mockResolvedValue(undefined)
  }
}));

vi.mock('../../lib/app-state-context', () => ({
  useProject: () => ({
    projectRoot: '/tmp/project', handleOpenFolder: vi.fn(), gitStatus: { branch: 'main' }
  }),
  useTeam: () => ({ claudeCheck: { state: 'ready', error: null }, runClaudeCheck: vi.fn() })
}));
vi.mock('../../lib/i18n', () => ({ useT: () => (key: string) => key }));
vi.mock('../../lib/hooks/use-v2-runtime-catalog', () => ({
  useV2RuntimeCatalog: () => ({
    models: [{ id: 'fable', label: 'Fable', supportedEfforts: ['high'], defaultEffort: 'high' }]
  })
}));
vi.mock('../../lib/hooks/use-v2-runtime-session', () => ({
  useV2RuntimeSession: () => harness.runtime
}));
vi.mock('../../lib/hooks/use-v2-permission-setting', () => ({
  useV2PermissionSetting: () => ({
    permission: harness.settings.v2PermissionMode,
    runtimePermission: harness.settings.v2PermissionMode === 'full'
      ? 'full'
      : harness.settings.v2PermissionMode === 'ask' ? 'ask' : 'workspace',
    setPermission: (permission: 'full' | 'agent' | 'ask') => {
      void harness.updateSettings({ v2PermissionMode: permission });
    }
  })
}));
vi.mock('../../lib/v2-team-launch', () => ({ launchV2Team: harness.launchTeam }));
vi.mock('../../stores/canvas', () => ({
  useCanvasStore: (selector: (state: { addCard: typeof harness.addCard }) => unknown) =>
    selector({ addCard: harness.addCard })
}));
vi.mock('../../stores/ui', () => ({
  useUiStore: (selector: (state: { setWorkspaceTeamId: typeof harness.selectTeam }) => unknown) =>
    selector({ setWorkspaceTeamId: harness.selectTeam })
}));
vi.mock('./TeamProjectionProvider', () => ({
  useTeamProjection: () => ({
    sessionActive: false, inspectorOpen: false, setInspectorOpen: vi.fn(),
    projection: { agents: [] }
  })
}));
vi.mock('./UnifiedComposer', () => ({
  UnifiedComposer: (props: {
    prompt: string; running: boolean; onPromptChange: (value: string) => void;
    permission: 'full' | 'agent' | 'ask';
    onPermissionChange: (value: 'full' | 'agent' | 'ask') => void;
    onSubmit: () => void; onStop: () => void;
  }) => (
    <div data-testid="composer" data-running={String(props.running)}>
      <input aria-label="prompt" value={props.prompt}
        onChange={(event) => props.onPromptChange(event.target.value)} />
      <select aria-label="permission" value={props.permission}
        onChange={(event) => props.onPermissionChange(event.target.value as 'full' | 'agent' | 'ask')}>
        <option value="full">full</option>
        <option value="agent">agent</option>
        <option value="ask">ask</option>
      </select>
      <button type="button" onClick={props.onSubmit} disabled={props.running}>submit</button>
      <button type="button" onClick={props.onStop}>stop</button>
    </div>
  )
}));

describe('V2Shell Team launch controls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    harness.settings.v2PermissionMode = 'agent';
    Object.defineProperty(window, 'api', {
      configurable: true,
      value: { app: { setupTeamMcp: vi.fn() } }
    });
  });

  it('Team 起動中は停止や新規タスクで composer を再有効化しない', async () => {
    let finishLaunch!: () => void;
    harness.launchTeam.mockImplementationOnce(() => new Promise<string>((resolve) => {
      finishLaunch = () => resolve('team-1');
    }));
    render(<V2Shell shortcutsEnabled={false} />);

    fireEvent.change(screen.getByRole('textbox', { name: 'prompt' }), {
      target: { value: 'チームで実装して' }
    });
    fireEvent.click(screen.getByRole('button', { name: 'submit' }));
    await waitFor(() => expect(harness.launchTeam).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId('composer')).toHaveAttribute('data-running', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'stop' }));
    expect(harness.runtime.stop).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'v2.shell.newTask' })).toBeDisabled();
    expect(screen.getByTestId('composer')).toHaveAttribute('data-running', 'true');

    finishLaunch();
    await waitFor(() => expect(screen.getByTestId('composer')).toHaveAttribute('data-running', 'false'));
    expect(harness.launchTeam).toHaveBeenCalledWith(expect.objectContaining({ permission: 'workspace' }));
  });

  it('権限変更を settings へ保存し、通常会話 request へ実効値を渡す', async () => {
    harness.settings.v2PermissionMode = 'ask';
    render(<V2Shell shortcutsEnabled={false} />);

    fireEvent.change(screen.getByRole('combobox', { name: 'permission' }), {
      target: { value: 'full' }
    });
    expect(harness.updateSettings).toHaveBeenCalledWith({ v2PermissionMode: 'full' });

    fireEvent.change(screen.getByRole('textbox', { name: 'prompt' }), {
      target: { value: '通常会話で確認して' }
    });
    fireEvent.click(screen.getByRole('button', { name: 'submit' }));
    await waitFor(() => expect(harness.runtime.send).toHaveBeenCalledWith(expect.objectContaining({
      permission: 'ask'
    })));
  });

  it('/team directiveを除去してLeaderへ依頼本文だけを渡す', async () => {
    harness.launchTeam.mockResolvedValueOnce('team-1');
    render(<V2Shell shortcutsEnabled={false} />);

    fireEvent.change(screen.getByRole('textbox', { name: 'prompt' }), {
      target: { value: '/team workerを1名採用して' }
    });
    fireEvent.click(screen.getByRole('button', { name: 'submit' }));

    await waitFor(() => expect(harness.launchTeam).toHaveBeenCalledWith(expect.objectContaining({
      initialMessage: 'workerを1名採用して'
    })));
  });

  it('/team単独は空依頼を起動せずTeam作成モードへ切り替える', () => {
    render(<V2Shell shortcutsEnabled={false} />);

    fireEvent.change(screen.getByRole('textbox', { name: 'prompt' }), {
      target: { value: '/team' }
    });
    fireEvent.click(screen.getByRole('button', { name: 'submit' }));

    expect(harness.launchTeam).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox', { name: 'prompt' })).toHaveValue('');
  });
});
