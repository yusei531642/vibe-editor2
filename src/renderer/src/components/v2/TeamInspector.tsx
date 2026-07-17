import { useEffect, useId, useRef, useState } from 'react';
import { FileCode2, GitBranch, GitCommit, RotateCcw, TerminalSquare, TestTube2, X } from 'lucide-react';
import { useT } from '../../lib/i18n';
import { useTeamProjection } from './TeamProjectionProvider';
import { MergeQueuePanel } from './MergeQueuePanel';

type InspectorTab = 'diff' | 'test' | 'artifact' | 'worktree' | 'raw';
const TABS: InspectorTab[] = ['diff', 'test', 'artifact', 'worktree', 'raw'];

export function TeamInspector({ embedded = false }: { embedded?: boolean }): JSX.Element | null {
  const t = useT();
  const {
    inspectorOpen,
    setInspectorOpen,
    selectedAgent,
    projection,
    reconnect,
    openTerminal,
    runWorktreeCommand
  } = useTeamProjection();
  const tabIdPrefix = useId();
  const [tab, setTab] = useState<InspectorTab>('diff');
  const [reconnecting, setReconnecting] = useState(false);
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    if (inspectorOpen) window.requestAnimationFrame(() => tabRefs.current[0]?.focus());
  }, [inspectorOpen]);

  if (!inspectorOpen && !embedded) return null;

  const selectTab = (next: InspectorTab): void => {
    setTab(next);
    tabRefs.current[TABS.indexOf(next)]?.focus();
  };

  return (
    <aside
      className={`team-inspector glass-surface${embedded ? ' team-inspector--embedded' : ''}`}
      aria-label={t('v2.drawer.inspector')}
    >
      <header>
        <div>
          <strong>{t('v2.drawer.inspector')}</strong>
          <span>{selectedAgent?.title ?? t('v2.inspector.noAgent')}</span>
        </div>
        {!embedded ? (
          <button
            type="button"
            aria-label={t('common.close')}
            onClick={() => setInspectorOpen(false)}
          >
            <X size={20} strokeWidth={1.75} aria-hidden="true" />
          </button>
        ) : null}
      </header>
      {selectedAgent ? (
        <>
          <dl className="team-inspector__summary">
            <div><dt>{t('v2.inspector.state')}</dt><dd>{t(`v2.team.status.${selectedAgent.status}`)}</dd></div>
            <div><dt>{t('v2.inspector.task')}</dt><dd>{selectedAgent.task?.description ?? '—'}</dd></div>
            <div><dt>{t('v2.inspector.endpoint')}</dt><dd>{selectedAgent.endpoint?.endpointId ?? '—'}</dd></div>
            <div><dt>{t('v2.inspector.backend')}</dt><dd>{selectedAgent.endpoint?.backend ?? '—'}</dd></div>
            <div><dt>{t('v2.inspector.thread')}</dt><dd>{selectedAgent.endpoint?.sessionId ?? '—'}</dd></div>
            <div><dt>{t('v2.inspector.worktree')}</dt><dd>{selectedAgent.worktree?.branchName ?? t('v2.worktree.unassigned')}</dd></div>
          </dl>
          <div className="team-inspector__files">
            <FileCode2 size={16} strokeWidth={1.75} aria-hidden="true" />
            <span>{selectedAgent.changedFiles.join(', ') || t('v2.inspector.noFiles')}</span>
          </div>
        </>
      ) : null}
      <div className="v2-inspector-tabs" role="tablist" aria-label={t('v2.inspector.tabs')}>
        {TABS.map((name, index) => (
          <button
            key={name}
            ref={(element) => {
              tabRefs.current[index] = element;
            }}
            type="button"
            role="tab"
            id={`${tabIdPrefix}-tab-${name}`}
            aria-selected={tab === name}
            aria-controls={`${tabIdPrefix}-panel-${name}`}
            tabIndex={tab === name ? 0 : -1}
            onClick={() => setTab(name)}
            onKeyDown={(event) => {
              const current = TABS.indexOf(name);
              if (event.key === 'ArrowRight') {
                event.preventDefault();
                selectTab(TABS[(current + 1) % TABS.length]);
              } else if (event.key === 'ArrowLeft') {
                event.preventDefault();
                selectTab(TABS[(current - 1 + TABS.length) % TABS.length]);
              } else if (event.key === 'Home') {
                event.preventDefault();
                selectTab(TABS[0]);
              } else if (event.key === 'End') {
                event.preventDefault();
                selectTab(TABS.at(-1) ?? 'raw');
              }
            }}
          >
            {t(`v2.inspector.tab.${name}`)}
          </button>
        ))}
      </div>
      <section
        className="team-inspector__panel"
        id={`${tabIdPrefix}-panel-${tab}`}
        role="tabpanel"
        aria-labelledby={`${tabIdPrefix}-tab-${tab}`}
      >
        {!selectedAgent ? (
          <p>{t('v2.inspector.noAgent')}</p>
        ) : tab === 'diff' ? (
          selectedAgent.runtime?.diffs.length ? (
            selectedAgent.runtime.diffs.map((diff, index) => <pre key={index}>{diff}</pre>)
          ) : <p>{t('v2.inspector.resultsEmpty')}</p>
        ) : tab === 'test' ? (
          selectedAgent.task?.doneEvidence?.length ? (
            <ul>{selectedAgent.task.doneEvidence.map((item) => <li key={item.criterion}><strong>{item.criterion}</strong><span>{item.evidence}</span></li>)}</ul>
          ) : <p><TestTube2 size={18} aria-hidden="true" />{t('v2.inspector.resultsEmpty')}</p>
        ) : tab === 'artifact' ? (
          selectedAgent.task?.artifactPath ? (
            <p><GitBranch size={18} aria-hidden="true" />{selectedAgent.task.artifactPath}</p>
          ) : <p>{t('v2.inspector.artifactEmpty')}</p>
        ) : tab === 'worktree' ? (
          <div className="worktree-inspector">
            {selectedAgent.worktree ? (
              <>
                <dl>
                  <div><dt>{t('v2.worktree.branch')}</dt><dd>{selectedAgent.worktree.branchName}</dd></div>
                  <div><dt>{t('v2.worktree.baseBranch')}</dt><dd>{selectedAgent.worktree.baseBranch}</dd></div>
                  <div><dt>{t('v2.worktree.baseCommit')}</dt><dd><code>{selectedAgent.worktree.baseCommit}</code></dd></div>
                  <div><dt>{t('v2.worktree.headCommit')}</dt><dd><code>{selectedAgent.worktree.headCommit}</code></dd></div>
                  <div><dt>{t('v2.worktree.clean')}</dt><dd>{t(selectedAgent.worktree.clean ? 'v2.worktree.status.clean' : 'v2.worktree.status.dirty')}</dd></div>
                </dl>
                <button type="button" onClick={() => void runWorktreeCommand({ action: 'resume', agentId: selectedAgent.agentId })}>
                  <GitBranch size={16} aria-hidden="true" />{t('v2.worktree.resume')}
                </button>
                <CandidateForm agentId={selectedAgent.agentId} />
                <button
                  type="button"
                  disabled={!selectedAgent.worktree.cleanupEligible}
                  onClick={() => {
                    if (window.confirm(t('v2.worktree.cleanupConfirm'))) {
                      void runWorktreeCommand({ action: 'cleanup', agentId: selectedAgent.agentId });
                    }
                  }}
                >
                  {t('v2.worktree.cleanup')}
                </button>
              </>
            ) : (
              <button type="button" onClick={() => void runWorktreeCommand({ action: 'create', agentId: selectedAgent.agentId })}>
                <GitBranch size={16} aria-hidden="true" />{t('v2.worktree.create')}
              </button>
            )}
            <MergeQueuePanel />
          </div>
        ) : (
          <ol className="team-inspector__raw">
            {(selectedAgent.runtime?.eventHistory ?? []).map((event) => (
              <li key={`${event.endpointId}:${event.sequence}`}>
                <time dateTime={event.timestamp}>{event.timestamp}</time>
                <code>{JSON.stringify(event)}</code>
              </li>
            ))}
            {selectedAgent.runtime?.eventTruncatedCount ? (
              <li>{t('v2.inspector.truncated', { count: selectedAgent.runtime.eventTruncatedCount })}</li>
            ) : null}
          </ol>
        )}
      </section>
      {selectedAgent ? (
        selectedAgent.endpoint?.restoreState === 'reconnectable' ? (
          <button
            type="button"
            className="v2-compat-terminal"
            disabled={reconnecting}
            onClick={() => {
              setReconnecting(true);
              void reconnect(selectedAgent.agentId)
                .catch((error) => console.warn('[session-restore] reconnect failed:', error))
                .finally(() => setReconnecting(false));
            }}
          >
            <RotateCcw size={18} strokeWidth={1.75} aria-hidden="true" />
            {t('v2.team.card.reconnect')}
          </button>
        ) : selectedAgent.endpoint?.restoreState === 'terminated' ? (
          <p className="team-inspector__terminated" role="status">
            {t('v2.team.card.terminated')}
          </p>
        ) : (
          <button
            type="button"
            className="v2-compat-terminal"
            onClick={() => openTerminal(selectedAgent.agentId)}
          >
            <TerminalSquare size={18} strokeWidth={1.75} aria-hidden="true" />
            {t('v2.inspector.openTerminal')}
          </button>
        )
      ) : null}
      {projection.runtimeDroppedCount > 0 ? (
        <small>{t('v2.inspector.runtimeDropped', { count: projection.runtimeDroppedCount })}</small>
      ) : null}
    </aside>
  );
}

export function CandidateForm({ agentId }: { agentId: string }): JSX.Element {
  const t = useT();
  const { runWorktreeCommand } = useTeamProjection();
  const [evidence, setEvidence] = useState('');
  const [pending, setPending] = useState(false);
  return (
    <form onSubmit={async (event) => {
      event.preventDefault();
      setPending(true);
      try {
        const succeeded = await runWorktreeCommand({ action: 'enqueue', agentId, evidence });
        if (succeeded) setEvidence('');
      } finally {
        setPending(false);
      }
    }}>
      <label htmlFor={`candidate-evidence-${agentId}`}>{t('v2.worktree.evidence')}</label>
      <textarea id={`candidate-evidence-${agentId}`} value={evidence} disabled={pending} onChange={(event) => setEvidence(event.target.value)} />
      <button type="submit" disabled={pending}><GitCommit size={16} aria-hidden="true" />{t('v2.worktree.enqueue')}</button>
    </form>
  );
}
