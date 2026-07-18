import { describe, expect, it, vi } from 'vitest';
import { launchV2Team } from '../v2-team-launch';

describe('launchV2Team', () => {
  it('選択中runtime設定でLeaderを作り、Hub設定後にTeam sceneを要求する', async () => {
    vi.spyOn(crypto, 'randomUUID').mockReturnValue('00000000-0000-4000-8000-000000000000');
    const setupTeamMcp = vi.fn(async () => undefined);
    const addCard = vi.fn(() => 'card-1');
    const selectTeam = vi.fn();
    const requestTeamScene = vi.fn();

    const teamId = await launchV2Team({
      projectRoot: '/repo',
      teamName: '実装チーム',
      initialMessage: 'チームで実装して',
      engine: 'codex',
      model: 'gpt-fixture',
      effort: 'high',
      permission: 'workspace',
      setupTeamMcp,
      addCard,
      selectTeam,
      requestTeamScene
    });

    expect(teamId).toBe('team-00000000-0000-4000-8000-000000000000');
    expect(setupTeamMcp).toHaveBeenCalledWith('/repo', teamId, '実装チーム', [{
      agentId: `leader-0-${teamId}`, role: 'leader', agent: 'codex'
    }]);
    expect(addCard).toHaveBeenCalledWith(expect.objectContaining({
      type: 'agent',
      payload: expect.objectContaining({
        runtimeProvider: 'codex-native',
        runtimeModel: 'gpt-fixture',
        runtimeEffort: 'high',
        runtimePermission: 'workspace',
        roleProfileId: 'leader',
        initialMessage: 'チームで実装して'
      })
    }));
    expect(selectTeam).toHaveBeenCalledWith(teamId);
    expect(requestTeamScene).toHaveBeenCalledOnce();
    expect(setupTeamMcp.mock.invocationCallOrder[0]).toBeLessThan(addCard.mock.invocationCallOrder[0]);
  });

  it('TeamHub setup の失敗時はカードも scene も作らない', async () => {
    const addCard = vi.fn(() => 'card-1');
    const requestTeamScene = vi.fn();
    await expect(launchV2Team({
      projectRoot: '/repo', teamName: '失敗チーム', initialMessage: 'チームで確認',
      engine: 'claude', model: 'fable', effort: 'high', permission: 'workspace',
      setupTeamMcp: vi.fn(async () => ({ ok: false, error: 'Hub setup failed' })),
      addCard, selectTeam: vi.fn(), requestTeamScene
    })).rejects.toThrow('Hub setup failed');
    expect(addCard).not.toHaveBeenCalled();
    expect(requestTeamScene).not.toHaveBeenCalled();
  });
});
