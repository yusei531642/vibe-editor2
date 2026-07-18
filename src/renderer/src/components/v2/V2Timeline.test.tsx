import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { V2Timeline } from './V2Timeline';

vi.mock('../../lib/i18n', () => ({ useT: () => (key: string) => key }));

describe('V2Timeline approval details', () => {
  it('承認前に実コマンドと作業ディレクトリを表示する', () => {
    render(<V2Timeline
      projectName="project"
      engine="claude"
      modelLabel="Fable"
      effort="high"
      entries={[]}
      running={false}
      pendingApproval={{
        endpointId: 'endpoint-1', requestId: 'approval-1', method: 'Bash',
        reason: 'テストを実行します', command: 'rm -rf ./generated', cwd: '/tmp/project'
      }}
      onApproval={vi.fn()}
    />);

    expect(screen.getByText('テストを実行します')).toBeInTheDocument();
    expect(screen.getByText('rm -rf ./generated')).toBeInTheDocument();
    expect(screen.getByText('/tmp/project')).toBeInTheDocument();
  });
});
