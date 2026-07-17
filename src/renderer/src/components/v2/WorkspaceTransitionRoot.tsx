import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties
} from 'react';
import { MessagesSquare, Network } from 'lucide-react';
import { V2Shell } from './V2Shell';
import { TeamWorkspaceScene } from './TeamWorkspaceScene';
import { TeamProjectionProvider } from './TeamProjectionProvider';
import { useTeam } from '../../lib/app-state-context';
import { useT } from '../../lib/i18n';
import { useRecruitListener } from '../../lib/use-recruit-listener';
import {
  useReducedMotion,
  type MotionPreference
} from '../../lib/use-reduced-motion';
import {
  useUiStore,
  type WorkspaceScene as WorkspaceSceneName
} from '../../stores/ui';
import { useSettingsValue } from '../../lib/settings-context';
import type { Team } from '../../../../types/shared';

const SCENE_DURATION_MS = 500;
const REDUCED_SCENE_DURATION_MS = 120;

interface FrameRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

interface FlipFrame {
  key: number;
  rect: FrameRect;
  translateX: number;
  translateY: number;
  scaleX: number;
  scaleY: number;
}

export interface WorkspaceTransitionRootProps {
  /** Test/Story harness only. Production derives this from useTeam(). */
  forceTeamSession?: boolean;
  motionPreference?: MotionPreference;
}

function toFrameRect(rect: DOMRect): FrameRect {
  return {
    left: rect.left,
    top: rect.top,
    width: rect.width,
    height: rect.height
  };
}

function frameStyle(frame: FlipFrame): CSSProperties {
  return {
    left: frame.rect.left,
    top: frame.rect.top,
    width: frame.rect.width,
    height: frame.rect.height,
    ['--workspace-flip-x' as string]: `${frame.translateX}px`,
    ['--workspace-flip-y' as string]: `${frame.translateY}px`,
    ['--workspace-flip-scale-x' as string]: frame.scaleX,
    ['--workspace-flip-scale-y' as string]: frame.scaleY
  };
}

function WorkspaceScene({
  scene,
  active,
  inactive,
  sceneRef,
  children
}: {
  scene: WorkspaceSceneName;
  active: boolean;
  inactive: boolean;
  sceneRef: React.RefObject<HTMLDivElement | null>;
  children: React.ReactNode;
}): JSX.Element {
  return (
    <div
      ref={sceneRef}
      className={`workspace-scene workspace-scene--${scene}`}
      data-active={active}
      inert={inactive ? true : undefined}
      aria-hidden={inactive ? 'true' : undefined}
      tabIndex={-1}
    >
      {children}
    </div>
  );
}

function EnabledWorkspaceTransitionRoot({
  forceTeamSession,
  motionPreference
}: WorkspaceTransitionRootProps): JSX.Element {
  const t = useT();
  const { teams } = useTeam();
  const reducedMotion = useReducedMotion(motionPreference);
  const persistedScene = useUiStore((state) => state.workspaceScene);
  const setPersistedScene = useUiStore((state) => state.setWorkspaceScene);
  const hasTeamSession = forceTeamSession ?? teams.length > 0;
  const team: Team = teams[0] ?? { id: 'pending-team', name: t('v2.team.defaultName') };
  const desiredScene = hasTeamSession ? persistedScene : 'focus';
  const [committedScene, setCommittedScene] = useState<WorkspaceSceneName>(desiredScene);
  const [transitioning, setTransitioning] = useState(false);
  const [flipFrame, setFlipFrame] = useState<FlipFrame | null>(null);
  const focusRef = useRef<HTMLDivElement>(null);
  const teamRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<number | null>(null);
  const currentSceneRef = useRef<WorkspaceSceneName>(desiredScene);
  const flipKeyRef = useRef(0);

  // V2 App path previously never mounted this singleton. It keeps canonical Canvas cards in sync.
  useRecruitListener();

  const clearTransitionTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  useEffect(() => clearTransitionTimer, [clearTransitionTimer]);

  // 起動直後は teams が非同期 populate 前で hasTeamSession=false になるため、無条件に
  // persist を focus へ書き戻すと「Canvas を開いたまま再起動 → 必ず会話 scene」になる。
  // 一度 team session を観測した後に消えた場合だけ persist を戻す (PR #35 レビュー)。
  const sawTeamSessionRef = useRef(hasTeamSession);
  useEffect(() => {
    if (hasTeamSession) {
      sawTeamSessionRef.current = true;
      return;
    }
    if (sawTeamSessionRef.current && persistedScene !== 'focus') {
      setPersistedScene('focus');
    }
  }, [hasTeamSession, persistedScene, setPersistedScene]);

  const measure = useCallback((scene: WorkspaceSceneName): DOMRect | null => {
    const root = scene === 'focus' ? focusRef.current : teamRef.current;
    if (!root) return null;
    // 非 active scene には CSS transform (退避位置) が掛かっており、そのまま測ると
    // FLIP の移動量が常にずれる。計測中だけ transform を無効化する (PR #35 レビュー)。
    // 同期実行なので paint は挟まれず、ちらつきは発生しない。
    root.setAttribute('data-measuring', 'true');
    try {
      return measureWithin(root, scene, team.id);
    } finally {
      root.removeAttribute('data-measuring');
    }
  }, [team.id]);

  const measureWithin = (
    root: HTMLDivElement,
    scene: WorkspaceSceneName,
    teamId: string
  ): DOMRect | null => {
    const selector =
      scene === 'focus'
        ? '[data-workspace-focus-frame], .v2-composer'
        : '[data-workspace-leader]';
    if (scene === 'team') {
      const leader = Array.from(root.querySelectorAll<HTMLElement>(selector)).find(
        (element) => element.dataset.workspaceTeamId === teamId
      );
      return (leader ?? root).getBoundingClientRect();
    }
    return (root.querySelector(selector) ?? root).getBoundingClientRect();
  };

  const returnFocus = useCallback((scene: WorkspaceSceneName): void => {
    const root = scene === 'focus' ? focusRef.current : teamRef.current;
    root?.focus({ preventScroll: true });
    if (scene === 'focus') {
      window.dispatchEvent(new Event('vibe-editor2:focus-composer'));
    }
  }, []);

  useEffect(() => {
    if (desiredScene === currentSceneRef.current) return;
    clearTransitionTimer();
    currentSceneRef.current = desiredScene;
    setCommittedScene(desiredScene);
    setTransitioning(false);
    setFlipFrame(null);
    // inert の解除は state 反映後の再 render で DOM に届く。inert が付いたままの
    // 要素に focus() しても無効なため、次 frame で回帰させる (PR #35 レビュー)。
    requestAnimationFrame(() => returnFocus(desiredScene));
  }, [clearTransitionTimer, desiredScene, returnFocus]);

  const requestScene = useCallback(
    (nextScene: WorkspaceSceneName): void => {
      if (!hasTeamSession || nextScene === currentSceneRef.current) return;
      const source = measure(currentSceneRef.current);
      const target = measure(nextScene);
      clearTransitionTimer();
      setPersistedScene(nextScene);
      currentSceneRef.current = nextScene;
      setTransitioning(true);

      if (!reducedMotion && source && target && source.width > 0 && source.height > 0) {
        setFlipFrame({
          key: ++flipKeyRef.current,
          rect: toFrameRect(source),
          translateX: target.left - source.left,
          translateY: target.top - source.top,
          scaleX: target.width / source.width,
          scaleY: target.height / source.height
        });
      } else {
        setFlipFrame(null);
      }

      const duration = reducedMotion ? REDUCED_SCENE_DURATION_MS : SCENE_DURATION_MS;
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        setCommittedScene(nextScene);
        setTransitioning(false);
        setFlipFrame(null);
        // setCommittedScene の inert 解除が DOM に反映された後に focus を戻す。
        requestAnimationFrame(() => returnFocus(nextScene));
      }, duration);
    }, [clearTransitionTimer, hasTeamSession, measure, reducedMotion, returnFocus, setPersistedScene]
  );

  const focusInactive = !transitioning && committedScene !== 'focus';
  const teamInactive = !transitioning && committedScene !== 'team';

  const workspace = (
    <main
      className="workspace-transition-root"
      data-scene={desiredScene}
      data-transitioning={transitioning || undefined}
      data-reduced-motion={reducedMotion || undefined}
    >
      {hasTeamSession ? (
        <nav className="workspace-scene-switcher glass-surface" aria-label={t('v2.scene.switcher')}>
          <button
            type="button"
            aria-pressed={desiredScene === 'focus'}
            onClick={() => requestScene('focus')}
          >
            <MessagesSquare size={18} strokeWidth={1.75} aria-hidden="true" />
            {t('v2.scene.conversation')}
          </button>
          <button
            type="button"
            aria-pressed={desiredScene === 'team'}
            onClick={() => requestScene('team')}
          >
            <Network size={18} strokeWidth={1.75} aria-hidden="true" />
            {t('v2.scene.canvas')}
          </button>
        </nav>
      ) : null}

      <WorkspaceScene
        scene="focus"
        active={desiredScene === 'focus'}
        inactive={focusInactive}
        sceneRef={focusRef}
      >
        <V2Shell />
      </WorkspaceScene>
      <WorkspaceScene
        scene="team"
        active={desiredScene === 'team'}
        inactive={teamInactive}
        sceneRef={teamRef}
      >
        <TeamWorkspaceScene team={team} />
      </WorkspaceScene>

      {flipFrame ? (
        <div
          key={flipFrame.key}
          className="workspace-flip-frame"
          style={frameStyle(flipFrame)}
          aria-hidden="true"
        />
      ) : null}
    </main>
  );
  return hasTeamSession ? (
    <TeamProjectionProvider
      team={team}
      teamSceneCommitted={committedScene === 'team'}
    >
      {workspace}
    </TeamProjectionProvider>
  ) : (
    workspace
  );
}

/**
 * flag off は Phase 0 の zero-regression 契約どおり legacy の `<V2Shell />` DOM を直接返す。
 * そのため設定画面で teamSceneV2 を実行中に切り替えると V2Shell は remount される。
 * identity を維持するには flag off 時も workspace wrapper を常設する移行が必要であり、
 * legacy DOM 完全互換を外す専用変更として扱う。
 */
export function WorkspaceTransitionRoot(props: WorkspaceTransitionRootProps): JSX.Element {
  const enabled = useSettingsValue('teamSceneV2');
  if (!enabled) return <V2Shell />;
  return <EnabledWorkspaceTransitionRoot {...props} />;
}
