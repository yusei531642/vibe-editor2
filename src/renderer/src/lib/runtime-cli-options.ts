import { findModelOverride } from './parse-args';
import type { RuntimePermission } from '../../../types/agent-runtime';

/** Native runtime が実行直前に PTY fallback しても V2 の選択値を失わないよう CLI 化する。 */
export function appendRuntimeCliOptions(
  args: string[],
  engine: 'claude' | 'codex',
  model?: string,
  effort?: string,
  permission?: RuntimePermission
): void {
  if (model && findModelOverride(args) === null) {
    args.push('--model', model);
  }
  if (effort) {
    if (engine === 'claude' && !args.some((arg) => arg === '--effort' || arg.startsWith('--effort='))) {
      args.push('--effort', effort);
    }
    if (engine === 'codex' && !args.some((arg) => arg.startsWith('model_reasoning_effort='))) {
      args.push('-c', `model_reasoning_effort="${effort}"`);
    }
  }
  if (permission === 'full') {
    const flag = engine === 'claude'
      ? '--dangerously-skip-permissions'
      : '--dangerously-bypass-approvals-and-sandbox';
    if (!args.includes(flag)) args.push(flag);
  } else if (permission === 'workspace' && engine === 'codex') {
    if (!args.includes('--sandbox')) args.push('--sandbox', 'workspace-write');
    if (!args.includes('--ask-for-approval')) args.push('--ask-for-approval', 'on-request');
  }
}
