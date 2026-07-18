import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { V2Shell } from './V2Shell';

const harness = vi.hoisted(() => ({
  launchTeam: vi.fn(), addCard: vi.fn(), selectTeam: vi.fn(),
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
    onSubmit: () => void; onStop: () => void;
  }) => (
    <div data-testid="composer" data-running={String(props.running)}>
      <input aria-label="prompt" value={props.prompt}
        onChange={(event) => props.onPromptChange(event.target.value)} />
      <button type="button" onClick={props.onSubmit} disabled={props.running}>submit</button>
      <button type="button" onClick={props.onStop}>stop</button>
    </div>
  )
}));

describe('V2Shell Team launch controls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
  });
});
