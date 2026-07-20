import { describe, expect, it } from 'vitest';
import { normalizeVisibleTeamRequest, requestsVisibleTeam } from '../v2-runtime-controls';

describe('requestsVisibleTeam', () => {
  it.each([
    'teamでやりたい',
    'TeamでAIについて議論して',
    'このチームでAIについて議論して',
    'チームで実装して',
    'チームを組んで調査して',
    'Use a team and work in parallel',
    'workerを含むteam体制で進めて',
    '/team workerを採用して',
    '／ＴＥＡＭ workerを採用して'
  ])('Team 起動要求を検出する: %s', (input) => {
    expect(requestsVisibleTeam(input)).toBe(true);
  });

  it.each([
    'このアプリのteam機能を説明して',
    'teamという単語を翻訳して',
    'teamではどんなことができますか',
    'teamでの作業を説明して',
    '通常どおり修正して'
  ])('説明・通常会話は起動しない: %s', (input) => {
    expect(requestsVisibleTeam(input)).toBe(false);
  });

  it.each([
    ['/team workerを採用して', 'workerを採用して'],
    ['／ＴＥＡＭ   ＡＢＣを調査して', 'ＡＢＣを調査して'],
    ['/team', ''],
    ['teamでＡＢＣを調査して', 'teamでＡＢＣを調査して']
  ])('slash directiveだけをruntime入力から除去する: %s', (input, expected) => {
    expect(normalizeVisibleTeamRequest(input)).toBe(expected);
  });
});
