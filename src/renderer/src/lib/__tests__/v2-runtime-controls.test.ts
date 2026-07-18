import { describe, expect, it } from 'vitest';
import { requestsVisibleTeam } from '../v2-runtime-controls';

describe('requestsVisibleTeam', () => {
  it.each([
    'teamでやりたい',
    'チームで実装して',
    'チームを組んで調査して',
    'Use a team and work in parallel',
    'workerを含むteam体制で進めて'
  ])('Team 起動要求を検出する: %s', (input) => {
    expect(requestsVisibleTeam(input)).toBe(true);
  });

  it.each([
    'このアプリのteam機能を説明して',
    'teamという単語を翻訳して',
    '通常どおり修正して'
  ])('説明・通常会話は起動しない: %s', (input) => {
    expect(requestsVisibleTeam(input)).toBe(false);
  });
});
