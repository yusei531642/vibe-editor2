import { useEffect, useId, useRef, useState } from 'react';
import { FileCode2, GitBranch, TerminalSquare, TestTube2, X } from 'lucide-react';
import { useT } from '../../lib/i18n';
import { useTeamProjection } from './TeamProjectionProvider';

type InspectorTab = 'diff' | 'test' | 'artifact' | 'raw';
const TABS: InspectorTab[] = ['diff', 'test', 'artifact', 'raw'];

export function TeamInspector({ embedded = false }: { embedded?: boolean }): JSX.Element | null {
  const t = useT();
  const {
    inspectorOpen,
    setInspectorOpen,
    selectedAgent,
    projection,
    openTerminal
  } = useTeamProjection();
  const tabIdPrefix = useId();
  const [tab, setTab] = useState<InspectorTab>('diff');
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
            <div><dt>{t('v2.inspector.worktree')}</dt><dd>{selectedAgent.worktree.label}</dd></div>
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
        <button
          type="button"
          className="v2-compat-terminal"
          onClick={() => openTerminal(selectedAgent.agentId)}
        >
          <TerminalSquare size={18} strokeWidth={1.75} aria-hidden="true" />
          {t('v2.inspector.openTerminal')}
        </button>
      ) : null}
      {projection.runtimeDroppedCount > 0 ? (
        <small>{t('v2.inspector.runtimeDropped', { count: projection.runtimeDroppedCount })}</small>
      ) : null}
    </aside>
  );
}
