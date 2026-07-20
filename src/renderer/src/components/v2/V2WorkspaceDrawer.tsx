import { useEffect, useMemo, useState } from 'react';
import {
  FileCode2,
  Folder,
  FolderOpen,
  MessagesSquare,
  Network,
  Search,
  SquarePen,
  X
} from 'lucide-react';
import type { AppUserInfo, SessionInfo } from '../../../../types/shared';
import { useProject } from '../../lib/app-state-context';
import { useT } from '../../lib/i18n';
import { useSettingsValue } from '../../lib/settings-context';
import { V2_REQUEST_TEAM_SCENE_EVENT } from '../../lib/v2-runtime-controls';

/** Codex サイドバーの「タスク数行 + もっと表示する」に合わせた表示上限。 */
const SESSION_PREVIEW_COUNT = 5;
const SESSION_EXPANDED_COUNT = 15;
const RECENT_PROJECT_COUNT = 5;
const CHANGED_FILE_COUNT = 8;

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

/**
 * V2 シェルの左サイドメニュー (Issue #77)。
 * Codex アプリのサイドバーを参考に「ナビ項目 → プロジェクト (直近セッションを
 * ネスト表示) → 変更ファイル → ユーザー行」を縦に並べるリスト構造で描画する。
 *
 * - セッション一覧はドロワー表示のたびに `sessions.list` で取得する (開いた時
 *   だけ mount されるので常駐コストなし)。
 * - recentProjects の行クリックで `handleOpenRecent` によるプロジェクト切替。
 * - セッション行は V2 シェルに resume 経路がまだ無いため表示専用。
 */
export function V2WorkspaceDrawer({
  projectName,
  changedFiles,
  hasEntries,
  canvasAvailable,
  onClose,
  onOpenProject,
  onNewTask,
}: {
  projectName: string;
  changedFiles: Array<{ path: string }>;
  hasEntries: boolean;
  /** チームセッションが存在し「キャンバス」scene へ遷移できるか。 */
  canvasAvailable: boolean;
  onClose: () => void;
  onOpenProject: () => void;
  onNewTask: () => void;
}): JSX.Element {
  const t = useT();
  const { projectRoot, handleOpenRecent } = useProject();
  const recentProjects = useSettingsValue('recentProjects');
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [userInfo, setUserInfo] = useState<AppUserInfo | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [sessionsExpanded, setSessionsExpanded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (!projectRoot) {
      setSessions([]);
      return;
    }
    window.api.sessions
      .list(projectRoot)
      .then((list) => {
        if (!cancelled) setSessions(list);
      })
      .catch(() => {
        if (!cancelled) setSessions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [projectRoot]);

  useEffect(() => {
    let cancelled = false;
    void window.api.app.getUserInfo().then((info) => {
      if (!cancelled) setUserInfo(info);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const normalizedQuery = query.trim().toLowerCase();
  const matches = (text: string): boolean =>
    normalizedQuery === '' || text.toLowerCase().includes(normalizedQuery);

  const filteredSessions = useMemo(() => {
    if (normalizedQuery === '') return sessions;
    return sessions.filter((session) =>
      (session.title || '').toLowerCase().includes(normalizedQuery)
    );
  }, [sessions, normalizedQuery]);
  const visibleSessions = filteredSessions.slice(
    0,
    sessionsExpanded ? SESSION_EXPANDED_COUNT : SESSION_PREVIEW_COUNT
  );
  const otherProjects = useMemo(
    () =>
      (recentProjects ?? [])
        .filter(
          (path) =>
            path !== projectRoot &&
            (normalizedQuery === '' ||
              basename(path).toLowerCase().includes(normalizedQuery))
        )
        .slice(0, RECENT_PROJECT_COUNT),
    [recentProjects, projectRoot, normalizedQuery]
  );
  const visibleChangedFiles = changedFiles
    .filter((file) => matches(file.path))
    .slice(0, CHANGED_FILE_COUNT);

  const focusComposer = (): void => {
    onClose();
    window.dispatchEvent(new Event('vibe-editor2:focus-composer'));
  };

  const openCanvas = (): void => {
    onClose();
    window.dispatchEvent(new Event(V2_REQUEST_TEAM_SCENE_EVENT));
  };

  return (
    <>
      <div className="v2-sidebar-backdrop" onClick={onClose} aria-hidden="true" />
      <aside className="v2-sidebar" aria-label={t('v2.drawer.left')}>
        <header className="v2-sidebar__header">
          <strong className="v2-sidebar__brand">vibe-editor</strong>
          <button
            type="button"
            aria-label={t('v2.drawer.search')}
            aria-expanded={searchOpen}
            onClick={() =>
              setSearchOpen((open) => {
                if (open) setQuery('');
                return !open;
              })
            }
          >
            <Search size={17} strokeWidth={1.8} />
          </button>
          <button type="button" aria-label={t('common.close')} onClick={onClose}>
            <X size={18} strokeWidth={1.8} />
          </button>
        </header>

        {searchOpen && (
          <div className="v2-sidebar__search">
            <input
              type="text"
              value={query}
              placeholder={t('v2.drawer.searchPlaceholder')}
              autoFocus
              onChange={(event) => setQuery(event.target.value)}
            />
          </div>
        )}

        <nav className="v2-sidebar__nav" aria-label={t('v2.shell.navigation')}>
          <button
            type="button"
            className="v2-sidebar__row"
            onClick={() => {
              onNewTask();
              onClose();
            }}
          >
            <SquarePen size={17} strokeWidth={1.8} />
            <span>{t('v2.shell.newTask')}</span>
          </button>
          <button type="button" className="v2-sidebar__row" onClick={focusComposer}>
            <MessagesSquare size={17} strokeWidth={1.8} />
            <span>{t('v2.drawer.chat')}</span>
          </button>
          {canvasAvailable && (
            <button type="button" className="v2-sidebar__row" onClick={openCanvas}>
              <Network size={17} strokeWidth={1.8} />
              <span>{t('v2.drawer.canvas')}</span>
            </button>
          )}
        </nav>

        <div className="v2-sidebar__scroll">
          <h2 className="v2-sidebar__label">{t('v2.drawer.projects')}</h2>
          <button
            type="button"
            className="v2-sidebar__row is-active"
            onClick={onOpenProject}
            title={projectRoot || undefined}
          >
            <FolderOpen size={17} strokeWidth={1.8} />
            <span>{projectName}</span>
          </button>
          {hasEntries && (
            <div className="v2-sidebar__session v2-sidebar__session--live">
              <i aria-hidden="true" />
              <span>{t('v2.drawer.currentSession')}</span>
            </div>
          )}
          {visibleSessions.map((session) => (
            <div
              className="v2-sidebar__session"
              key={session.id}
              title={session.title || undefined}
            >
              <span>{session.title || t('v2.drawer.untitledSession')}</span>
            </div>
          ))}
          {!hasEntries && filteredSessions.length === 0 && (
            <p className="v2-sidebar__empty">{t('v2.drawer.noSessions')}</p>
          )}
          {filteredSessions.length > SESSION_PREVIEW_COUNT && (
            <button
              type="button"
              className="v2-sidebar__more"
              onClick={() => setSessionsExpanded((expanded) => !expanded)}
            >
              {sessionsExpanded ? t('v2.drawer.showLess') : t('v2.drawer.showMore')}
            </button>
          )}

          {otherProjects.map((path) => (
            <button
              type="button"
              className="v2-sidebar__row"
              key={path}
              title={path}
              onClick={() => {
                onClose();
                void handleOpenRecent(path);
              }}
            >
              <Folder size={17} strokeWidth={1.8} />
              <span>{basename(path)}</span>
            </button>
          ))}

          {visibleChangedFiles.length > 0 && (
            <>
              <h2 className="v2-sidebar__label">
                {t('v2.drawer.changedFiles')}
                <span className="v2-sidebar__count">{changedFiles.length}</span>
              </h2>
              {visibleChangedFiles.map((file) => (
                <div className="v2-sidebar__file" key={file.path} title={file.path}>
                  <FileCode2 size={16} strokeWidth={1.8} />
                  <span>{file.path}</span>
                </div>
              ))}
            </>
          )}
        </div>

        <footer className="v2-sidebar__footer">
          <span className="v2-sidebar__avatar" aria-hidden="true">
            {(userInfo?.username ?? '?').slice(0, 2).toUpperCase()}
          </span>
          <span className="v2-sidebar__user">{userInfo?.username ?? '…'}</span>
          <span className="v2-sidebar__version">
            {userInfo ? `v${userInfo.version}` : ''}
          </span>
        </footer>
      </aside>
    </>
  );
}
