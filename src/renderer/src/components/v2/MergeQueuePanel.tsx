import { useState } from 'react';
import { Check, GitMerge, RotateCcw, ShieldAlert, XCircle } from 'lucide-react';
import type { MergeCandidateSnapshot, WorktreeCommand } from '../../../../types/shared';
import { useT } from '../../lib/i18n';
import { useTeamProjection } from './TeamProjectionProvider';
import '../../styles/components/worktree-merge-queue.css';

function shortCommit(commit: string): string {
  return commit.slice(0, 10);
}

export function MergeQueuePanel(): JSX.Element {
  const t = useT();
  const { worktreeSnapshot, runWorktreeCommand } = useTeamProjection();
  const [pendingId, setPendingId] = useState<string | null>(null);
  const candidates = worktreeSnapshot?.candidates ?? [];

  const run = async (candidate: MergeCandidateSnapshot, command: WorktreeCommand): Promise<void> => {
    setPendingId(candidate.id);
    try {
      await runWorktreeCommand(command);
    } finally {
      setPendingId(null);
    }
  };

  return (
    <section className="merge-queue-panel" aria-label={t('v2.mergeQueue.title')}>
      <header>
        <div>
          <GitMerge size={17} aria-hidden="true" />
          <strong>{t('v2.mergeQueue.title')}</strong>
        </div>
        <small>{t('v2.mergeQueue.reviewRequired')}</small>
      </header>
      {candidates.length === 0 ? (
        <p className="merge-queue-panel__empty">{t('v2.mergeQueue.empty')}</p>
      ) : (
        <ol>
          {candidates.map((candidate) => {
            const busy = pendingId === candidate.id || candidate.status === 'integrating';
            return (
              <li key={candidate.id} data-status={candidate.status}>
                <div className="merge-queue-panel__heading">
                  <strong>#{candidate.queuePosition} · {candidate.agentId}</strong>
                  <span>{t(`v2.mergeQueue.status.${candidate.status}`)}</span>
                </div>
                <dl>
                  <div><dt>{t('v2.mergeQueue.base')}</dt><dd><code>{shortCommit(candidate.baseCommit)}</code></dd></div>
                  <div><dt>{t('v2.mergeQueue.candidate')}</dt><dd><code>{shortCommit(candidate.commit)}</code></dd></div>
                </dl>
                <p>{candidate.changedPaths.join(', ')}</p>
                {candidate.evidence ? <pre>{candidate.evidence}</pre> : null}
                {candidate.conflict ? (
                  <section className="merge-conflict-inspector" aria-label={t('v2.mergeQueue.conflict')}>
                    <ShieldAlert size={17} aria-hidden="true" />
                    <strong>{t('v2.mergeQueue.conflict')}</strong>
                    <dl>
                      <div><dt>{t('v2.mergeQueue.base')}</dt><dd><code>{candidate.conflict.baseCommit}</code></dd></div>
                      <div><dt>{t('v2.mergeQueue.candidate')}</dt><dd><code>{candidate.conflict.candidateCommit}</code></dd></div>
                      <div><dt>{t('v2.mergeQueue.paths')}</dt><dd>{candidate.conflict.paths.join(', ')}</dd></div>
                    </dl>
                  </section>
                ) : null}
                <div className="merge-queue-panel__actions">
                  {['pendingReview', 'changesRequested', 'conflict'].includes(candidate.status) ? (
                    <>
                      <button type="button" disabled={busy} onClick={() => void run(candidate, { action: 'review', candidateId: candidate.id, decision: 'approve' })}>
                        <Check size={15} aria-hidden="true" />{t('v2.mergeQueue.approve')}
                      </button>
                      <button type="button" disabled={busy} onClick={() => void run(candidate, { action: 'review', candidateId: candidate.id, decision: 'requestChanges' })}>
                        <RotateCcw size={15} aria-hidden="true" />{t('v2.mergeQueue.requestChanges')}
                      </button>
                    </>
                  ) : null}
                  <button
                    type="button"
                    disabled={busy || candidate.status !== 'approved' || Boolean(worktreeSnapshot?.integrationInProgress)}
                    onClick={() => void run(candidate, { action: 'integrate', candidateId: candidate.id })}
                  >
                    <GitMerge size={15} aria-hidden="true" />{t('v2.mergeQueue.integrate')}
                  </button>
                  {!['integrating', 'integrated', 'cancelled'].includes(candidate.status) ? (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void run(candidate, { action: 'cancel', candidateId: candidate.id })}
                    >
                      <XCircle size={15} aria-hidden="true" />{t('v2.mergeQueue.cancel')}
                    </button>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
