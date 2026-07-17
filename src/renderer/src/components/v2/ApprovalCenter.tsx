import { useEffect, useId, useRef, useState } from 'react';
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
  const [pendingFocus, setPendingFocus] = useState<{
    endpointId: string;
    requestId: string;
    index: number;
  } | null>(null);
  const itemRefs = useRef<Array<HTMLElement | null>>([]);
  const dialogRef = useRef<HTMLElement | null>(null);
  const closeButtonRef = useRef<HTMLButtonElement | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const wasOpenRef = useRef(false);
  const titleId = useId();

  useEffect(() => {
    if (approvalsOpen) {
      if (!wasOpenRef.current) {
        previousFocusRef.current =
          document.activeElement instanceof HTMLElement ? document.activeElement : null;
      }
      wasOpenRef.current = true;
      const frame = window.requestAnimationFrame(() => {
        (itemRefs.current[0] ?? closeButtonRef.current)?.focus();
      });
      return () => window.cancelAnimationFrame(frame);
    }
    if (wasOpenRef.current) previousFocusRef.current?.focus();
    wasOpenRef.current = false;
  }, [approvalsOpen]);

  useEffect(() => {
    setActiveIndex((current) =>
      Math.min(current, Math.max(0, projection.approvals.length - 1))
    );
  }, [projection.approvals.length]);

  useEffect(() => {
    if (
      !pendingFocus ||
      projection.approvals.some(
        (approval) =>
          approval.endpointId === pendingFocus.endpointId &&
          approval.requestId === pendingFocus.requestId
      )
    ) {
      return;
    }
    const nextIndex = Math.min(pendingFocus.index, projection.approvals.length - 1);
    setActiveIndex(Math.max(0, nextIndex));
    const frame = window.requestAnimationFrame(() => {
      if (nextIndex >= 0) itemRefs.current[nextIndex]?.focus();
      else closeButtonRef.current?.focus();
      setPendingFocus(null);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [pendingFocus, projection.approvals]);

  if (!approvalsOpen) return null;

  const focusAt = (index: number): void => {
    const next = Math.max(0, Math.min(index, projection.approvals.length - 1));
    setActiveIndex(next);
    itemRefs.current[next]?.focus();
  };

  const decide = async (
    agentId: string,
    endpointId: string,
    requestId: string,
    decision: RuntimeApprovalDecision
  ): Promise<void> => {
    const id = `${endpointId}:${requestId}`;
    setBusyId(id);
    setError(null);
    const index = projection.approvals.findIndex(
      (approval) => approval.endpointId === endpointId && approval.requestId === requestId
    );
    try {
      await respondApproval(agentId, endpointId, requestId, decision);
      setPendingFocus({ endpointId, requestId, index: Math.max(0, index) });
    } catch (responseError) {
      setError(responseError instanceof Error ? responseError.message : String(responseError));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <aside
      ref={dialogRef}
      className="approval-center glass-surface"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault();
          setApprovalsOpen(false);
          return;
        }
        if (event.key !== 'Tab') return;
        const focusable = Array.from(
          dialogRef.current?.querySelectorAll<HTMLElement>(
            'button:not([disabled]), [tabindex="0"]'
          ) ?? []
        );
        const first = focusable[0];
        const last = focusable.at(-1);
        if (!first || !last) return;
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }}
    >
      <header>
        <div>
          <CheckCheck size={20} strokeWidth={1.75} aria-hidden="true" />
          <strong id={titleId}>{t('v2.approval.center')}</strong>
          <span>{projection.approvals.length}</span>
        </div>
        <button
          ref={closeButtonRef}
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
                      onClick={() =>
                        void decide(
                          approval.agentId,
                          approval.endpointId,
                          approval.requestId,
                          decision
                        )
                      }
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
