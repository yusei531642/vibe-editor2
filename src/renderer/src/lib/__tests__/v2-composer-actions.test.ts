import { describe, expect, it } from 'vitest';
import { attachmentName, buildV2RuntimeInput } from '../v2-composer-actions';

describe('v2 composer actions', () => {
  it('WindowsとUnixのパスから表示名を取り出す', () => {
    expect(attachmentName('/workspace/src/App.tsx')).toBe('App.tsx');
    expect(attachmentName('C:\\workspace\\仕様書.md')).toBe('仕様書.md');
  });

  it('選択されたファイルパスを安全な引用形式でruntime入力へ渡す', () => {
    const input = buildV2RuntimeInput({
      text: '確認して',
      intent: 'message',
      activeGoal: null,
      attachments: [{ name: 'odd.md', path: '/tmp/line\nbreak.md' }],
    });
    expect(input).toContain('確認して');
    expect(input).toContain(JSON.stringify('/tmp/line\nbreak.md'));
  });

  it('作成したGoalを後続ターンの文脈として保持する', () => {
    const goal = buildV2RuntimeInput({
      text: 'リリースを完成させる',
      intent: 'goal',
      activeGoal: null,
      attachments: [],
    });
    const followUp = buildV2RuntimeInput({
      text: '残件を確認して',
      intent: 'message',
      activeGoal: 'リリースを完成させる',
      attachments: [],
    });
    expect(goal).toContain('active goal');
    expect(followUp).toContain('リリースを完成させる');
  });

  it('Team intentをruntimeへ明示する', () => {
    expect(buildV2RuntimeInput({
      text: '並列でレビューして',
      intent: 'team',
      activeGoal: null,
      attachments: [],
    })).toContain('Create a team');
  });
});
