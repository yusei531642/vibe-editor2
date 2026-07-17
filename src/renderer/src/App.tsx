/**
 * App — IDE モードのルート。
 *
 * Issue #731: 旧 App.tsx は 1136 行の god component で、`useProjectLoader` /
 * `useFileTabs` / `useTerminalTabs` / `useTeamManagement` を逐次呼び出しながら
 * hook 間の循環参照を 5 連発の `useRef` ブリッジで先送りしていた。
 *
 * これを 2 つに分解した:
 *   - `AppStateProvider` (lib/app-state-context.tsx) — hook 統合層と ref ブリッジ
 *     を内包し、`useProject()` / `useTabs()` / `useTeam()` の 3 consumer hook で
 *     必要な slice だけを公開する。
 *   - `AppShell` (components/AppShell.tsx) — 画面本体 (巨大 JSX + 画面ローカル
 *     state / derived / handler)。3 consumer hook で状態を購読する。
 *
 * App は Provider tree と IDE / Canvas の両画面をマウントする。両画面を同じ
 * AppStateProvider の子に置くため、ref ブリッジは完全に Provider 内部へ閉じ込められ、
 * 両者は同じ project / tabs / team state を購読する。
 */
import { useWindowFrameInsets } from './lib/use-window-frame-insets';
import { AppStateProvider } from './lib/app-state-context';
import { V2Shell } from './components/v2/V2Shell';

export function App(): JSX.Element {
  // Issue #307: Windows 11 フレームレス最大化時の不可視リサイズ境界を CSS 変数で補正。
  useWindowFrameInsets();

  return (
    <AppStateProvider>
      <V2Shell />
    </AppStateProvider>
  );
}
