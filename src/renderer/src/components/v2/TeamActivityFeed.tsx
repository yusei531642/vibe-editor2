import { Activity, AlertCircle, CheckCheck, ListTodo, MessageSquareReply, UserPlus } from 'lucide-react';
import { useT } from '../../lib/i18n';
import { useTeamProjection } from './TeamProjectionProvider';

const ICONS = {
  recruit: UserPlus,
  task: ListTodo,
  report: MessageSquareReply,
  approval: CheckCheck,
  error: AlertCircle
};

export function TeamActivityFeed(): JSX.Element {
  const t = useT();
  const { projection, selectAgent, openInspector } = useTeamProjection();
  return (
    <aside className="team-activity-feed glass-surface" aria-label={t('v2.team.activity')}>
      <header>
        <Activity size={18} strokeWidth={1.75} aria-hidden="true" />
        <strong>{t('v2.team.activity')}</strong>
      </header>
      {projection.activity.length === 0 ? (
        <p>{t('v2.team.activityEmpty')}</p>
      ) : (
        <ol>
          {projection.activity.slice(0, 30).map((item) => {
            const Icon = ICONS[item.kind];
            return (
              <li key={item.id} data-kind={item.kind}>
                <Icon size={16} strokeWidth={1.75} aria-hidden="true" />
                <button
                  type="button"
                  disabled={!item.agentId}
                  onClick={() => {
                    if (!item.agentId) return;
                    selectAgent(item.agentId);
                    openInspector(item.agentId);
                  }}
                  aria-label={item.message}
                >
                  <span>{item.message}</span>
                  <time dateTime={item.timestamp}>
                    {new Date(item.timestamp).toLocaleTimeString([], {
                      hour: '2-digit',
                      minute: '2-digit'
                    })}
                  </time>
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </aside>
  );
}
