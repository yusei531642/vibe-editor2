import { useEffect, useState } from 'react';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useUiStore } from '../../stores/ui';
import { WorkspaceTransitionRoot } from './WorkspaceTransitionRoot';

const shellLifecycle = vi.hoisted(() => ({ mounts: 0, unmounts: 0 }));
const teamContext = vi.hoisted(() => ({
  teams: [{ id: 'team-1', name: 'Issue 25 Team' }] as Array<{ id: string; name: string }>
}));

vi.mock('../../lib/settings-context', () => ({
  useSettingsValue: () => true
}));

vi.mock('../../lib/app-state-context', () => ({
  useTeam: () => ({ teams: teamContext.teams })
}));

vi.mock('../../lib/i18n', () => ({
  useT: () => (key: string) =>
    ({
      'v2.scene.switcher': 'Team session view',
      'v2.scene.conversation': 'Conversation',
      'v2.scene.canvas': 'Canvas'
    })[key] ?? key
}));

vi.mock('../../lib/use-recruit-listener', () => ({
  useRecruitListener: vi.fn()
}));

vi.mock('./V2Shell', () => ({
  V2Shell: () => {
    const [draft, setDraft] = useState('');
    useEffect(() => {
      shellLifecycle.mounts += 1;
      return () => {
        shellLifecycle.unmounts += 1;
      };
    }, []);
    return (
      <div>
        <div data-workspace-focus-frame="">Timeline</div>
        <label>
          Draft
          <textarea aria-label="Draft" value={draft} onChange={(event) => setDraft(event.target.value)} />
        </label>
      </div>
    );
  }
}));

vi.mock('./TeamWorkspaceScene', () => ({
  TeamWorkspaceScene: () => <div data-workspace-leader="">Leader</div>
}));

describe('WorkspaceTransitionRoot', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useUiStore.setState({ workspaceScene: 'focus' });
    shellLifecycle.mounts = 0;
    shellLifecycle.unmounts = 0;
    teamContext.teams = [{ id: 'team-1', name: 'Issue 25 Team' }];
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement
    ) {
      const leader = this.hasAttribute('data-workspace-leader');
      return {
        x: leader ? 640 : 120,
        y: leader ? 160 : 100,
        left: leader ? 640 : 120,
        top: leader ? 160 : 100,
        right: leader ? 1120 : 920,
        bottom: leader ? 440 : 620,
        width: leader ? 480 : 800,
        height: leader ? 280 : 520,
        toJSON: () => ({})
      } as DOMRect;
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it('scene switch後もConversation subtreeの入力状態を維持する', () => {
    const { container } = render(<WorkspaceTransitionRoot forceTeamSession />);
    const draft = screen.getByRole('textbox', { name: 'Draft' });
    fireEvent.change(draft, { target: { value: 'preserved prompt' } });

    fireEvent.click(screen.getByRole('button', { name: 'Canvas' }));
    act(() => vi.advanceTimersByTime(500));
    fireEvent.click(screen.getByRole('button', { name: 'Conversation' }));
    act(() => vi.advanceTimersByTime(500));

    expect(container.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe('preserved prompt');
    expect(shellLifecycle.mounts).toBe(1);
    expect(shellLifecycle.unmounts).toBe(0);
  });

  it('transition中の連続操作では最後に要求されたsceneへ収束する', () => {
    const { container } = render(<WorkspaceTransitionRoot forceTeamSession />);
    fireEvent.click(screen.getByRole('button', { name: 'Canvas' }));
    fireEvent.click(screen.getByRole('button', { name: 'Conversation' }));
    fireEvent.click(screen.getByRole('button', { name: 'Canvas' }));
    act(() => vi.advanceTimersByTime(500));

    expect(container.querySelector('.workspace-transition-root')).toHaveAttribute('data-scene', 'team');
    expect(container.querySelector('.workspace-scene--focus')).toHaveAttribute('inert');
    expect(container.querySelector('.workspace-scene--team')).not.toHaveAttribute('inert');
  });

  it('完了後だけ非active sceneをinertかつaria-hiddenにする', () => {
    const { container } = render(<WorkspaceTransitionRoot forceTeamSession />);
    const focus = container.querySelector('.workspace-scene--focus');
    const team = container.querySelector('.workspace-scene--team');
    expect(team).toHaveAttribute('inert');
    expect(team).toHaveAttribute('aria-hidden', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'Canvas' }));
    expect(focus).not.toHaveAttribute('inert');
    expect(team).not.toHaveAttribute('inert');
    act(() => vi.advanceTimersByTime(500));
    expect(focus).toHaveAttribute('inert');
    expect(focus).toHaveAttribute('aria-hidden', 'true');
    expect(team).not.toHaveAttribute('aria-hidden');
  });

  it('Team scene表示中にTeam sessionが消えたらFocusをactiveへ同期する', () => {
    const { container, rerender } = render(<WorkspaceTransitionRoot />);
    fireEvent.click(screen.getByRole('button', { name: 'Canvas' }));
    act(() => vi.advanceTimersByTime(500));
    expect(container.querySelector('.workspace-scene--focus')).toHaveAttribute('inert');

    teamContext.teams = [];
    rerender(<WorkspaceTransitionRoot />);

    expect(container.querySelector('.workspace-transition-root')).toHaveAttribute(
      'data-scene',
      'focus'
    );
    expect(container.querySelector('.workspace-scene--focus')).not.toHaveAttribute('inert');
    expect(container.querySelector('.workspace-scene--focus')).not.toHaveAttribute('aria-hidden');
    expect(container.querySelector('.workspace-scene--team')).toHaveAttribute('inert');
  });

  it('reduced motionではFLIP移動を作らずcross-fade時間で完了する', () => {
    const { container } = render(
      <WorkspaceTransitionRoot forceTeamSession motionPreference="reduced" />
    );
    fireEvent.click(screen.getByRole('button', { name: 'Canvas' }));
    expect(container.querySelector('.workspace-transition-root')).toHaveAttribute(
      'data-reduced-motion',
      'true'
    );
    expect(container.querySelector('.workspace-flip-frame')).toBeNull();
    act(() => vi.advanceTimersByTime(120));
    expect(container.querySelector('.workspace-scene--focus')).toHaveAttribute('inert');
  });
});
