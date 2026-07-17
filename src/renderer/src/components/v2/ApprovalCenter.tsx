import { useEffect, useRef, useState } from 'react';
import { Check, CheckCheck, CircleX, X } from 'lucide-react';
import type { RuntimeApprovalDecision } from '../../../../types/agent-runtime';
import { useT } from '../../lib/i18n';
import { useTeamProjection } from './TeamProjectionProvider';

const DECISIONS: Array<{
  decision: RuntimeApprovalDecision;
  labelKey: string;
  icon: typeof Check;
}> = [
  { decision: 'accept', labelKey: 'v2.approval.accept', icon: Check },
  { decision: 'acceptForSession', labelKey: 'v2.approval.acceptSession', icon: CheckCheck },
  { decision: 'decline', labelKey: 'v2.approval.decline', icon: CircleX },
  { decision: 'cancel', labelKey: 'v2.approval.cancel', icon: X }
];

export function ApprovalCenter(): JSX.Element | null {
  const t = useT();
  const { approvalsOpen, setApprovalsOpen, projection, respondApproval } = useTeamProjection();
  const [activeIndex, setActiveIndex] = useState(0);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const itemRefs = useRef<Array<HTMLElement | null>>([]);

  useEffect(() => {
    if (!approvalsOpen) return;
    setActiveIndex((current) => Math.min(current, Math.max(0, projection.approvals.length - 1)));
    window.requestAnimationFrame(() => itemRefs.current[0]?.focus());
  }, [approvalsOpen, projection.approvals.length]);

  if (!approvalsOpen) return null;

  const focusAt = (index: number): void => {
    const next = Math.max(0, Math.min(index, projection.approvals.length - 1));
    setActiveIndex(next);
    itemRefs.current[next]?.focus();
  };

  const decide = async (
    endpointId: string,
    requestId: string,
    decision: RuntimeApprovalDecision
  ): Promise<void> => {
    const id = `${endpointId}:${requestId}`;
    setBusyId(id);
    setError(null);
    try {
      await respondApproval(endpointId, requestId, decision);
    } catch (responseError) {
      setError(responseError instanceof Error ? responseError.message : String(responseError));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <aside className="approval-center glass-surface" aria-label={t('v2.approval.center')}>
      <header>
        <div>
          <CheckCheck size={20} strokeWidth={1.75} aria-hidden="true" />
          <strong>{t('v2.approval.center')}</strong>
          <span>{projection.approvals.length}</span>
        </div>
        <button
          type="button"
          onClick={() => setApprovalsOpen(false)}
          aria-label={t('common.close')}
        >
          <X size={20} strokeWidth={1.75} aria-hidden="true" />
        </button>
      </header>
      {projection.approvals.length === 0 ? (
        <p className="approval-center__empty">{t('v2.approval.empty')}</p>
      ) : (
        <div className="approval-center__list" role="list" aria-label={t('v2.approval.pending')}>
          {projection.approvals.map((approval, index) => {
            const approvalId = `${approval.endpointId}:${approval.requestId}`;
            return (
              <article
                key={approvalId}
                ref={(element) => {
                  itemRefs.current[index] = element;
                }}
                role="listitem"
                tabIndex={index === activeIndex ? 0 : -1}
                onFocus={() => setActiveIndex(index)}
                onKeyDown={(event) => {
                  if (event.key === 'ArrowDown') {
                    event.preventDefault();
                    focusAt((index + 1) % projection.approvals.length);
                  } else if (event.key === 'ArrowUp') {
                    event.preventDefault();
                    focusAt((index - 1 + projection.approvals.length) % projection.approvals.length);
                  } else if (event.key === 'Home') {
                    event.preventDefault();
                    focusAt(0);
                  } else if (event.key === 'End') {
                    event.preventDefault();
                    focusAt(projection.approvals.length - 1);
                  }
                }}
              >
                <div>
                  <strong>{approval.agentTitle}</strong>
                  <span>{approval.method}</span>
                </div>
                <p>{approval.reason ?? t('v2.approval.noReason')}</p>
                {approval.command ? <code>{approval.command}</code> : null}
                <div className="approval-center__actions">
                  {DECISIONS.map(({ decision, labelKey, icon: Icon }) => (
                    <button
                      key={decision}
                      type="button"
                      disabled={busyId !== null}
                      onClick={() => void decide(approval.endpointId, approval.requestId, decision)}
                    >
                      <Icon size={16} strokeWidth={1.75} aria-hidden="true" />
                      {t(labelKey)}
                    </button>
                  ))}
                </div>
              </article>
            );
          })}
        </div>
      )}
      {error ? <p className="approval-center__error" role="status">{error}</p> : null}
    </aside>
  );
}
