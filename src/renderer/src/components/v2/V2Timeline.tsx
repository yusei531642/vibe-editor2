import type { RuntimeApprovalDecision } from '../../../../types/agent-runtime';
import type { V2PendingApproval } from '../../lib/hooks/use-v2-runtime-session';
import { useT } from '../../lib/i18n';
import { engineLabel } from '../../lib/v2-runtime-controls';
import type { V2Engine } from './UnifiedComposer';
import type { V2ComposerAttachment, V2ComposerIntent } from '../../lib/v2-composer-actions';

export interface V2TimelineEntry {
  id: string;
  role: 'user' | 'agent';
  text: string;
  engine: V2Engine;
  attachments?: V2ComposerAttachment[];
  intent?: V2ComposerIntent;
}

export function V2Timeline({
  projectName,
  engine,
  modelLabel,
  effort,
  entries,
  running,
  pendingApproval,
  onApproval
}: {
  projectName: string;
  engine: V2Engine;
  modelLabel: string;
  effort: string;
  entries: V2TimelineEntry[];
  running: boolean;
  pendingApproval: V2PendingApproval | null;
  onApproval: (decision: RuntimeApprovalDecision) => void;
}): JSX.Element {
  const t = useT();
  return (
    <section className="v2-timeline" aria-live="polite" data-workspace-focus-frame="">
      <header>
        <div>
          <span className={`v2-engine-dot v2-engine-dot--${engine}`} />
          <strong>{projectName}</strong>
        </div>
        <span>{engineLabel(engine)} · {modelLabel} · {effort || 'default'}</span>
      </header>
      <div className="v2-timeline__body">
        {entries.map((entry) => (
          <article key={entry.id} className={`v2-message v2-message--${entry.role}`}>
            <span>
              {entry.role === 'user'
                ? t('v2.timeline.you')
                : engineLabel(entry.engine)}
            </span>
            <p>{entry.text}</p>
            {entry.attachments?.length ? (
              <div className="v2-message__attachments">
                {entry.attachments.map((attachment) => (
                  <span key={attachment.path} title={attachment.path}>{attachment.name}</span>
                ))}
              </div>
            ) : null}
          </article>
        ))}
        {running ? (
          <div className="v2-thinking" aria-label={t('v2.timeline.running')}>
            <i />
            {t('v2.timeline.exploring')}
          </div>
        ) : null}
        {pendingApproval ? (
          <aside className="v2-approval-request" role="alert">
            <strong>{t('v2.approval.title')}</strong>
            <p>{pendingApproval.reason || pendingApproval.method}</p>
            <div>
              <button type="button" onClick={() => onApproval('accept')}>
                {t('v2.approval.accept')}
              </button>
              <button type="button" onClick={() => onApproval('decline')}>
                {t('v2.approval.decline')}
              </button>
            </div>
          </aside>
        ) : null}
      </div>
    </section>
  );
}
