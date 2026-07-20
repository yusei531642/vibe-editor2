/**
 * HandoffEdge — team_send の hand-off を可視化するアニメーション付き edge。
 *
 * Rust 側 TeamHub が `team:handoff` event を emit すると、Canvas が一時的に
 * このエッジを追加 → 10 秒で自動 fade out (#379)。
 *
 * 表現 (Issue #79: team-morph デモ準拠の 2 層構成):
 *   - base 層: 淡色の bezier path が pathLength 正規化 + stroke-dashoffset で
 *     「描画される」(path reveal)
 *   - flow 層: 発信者ロールカラーの細い点線が glow 付きで流れ続ける
 *   - メッセージ preview はニュートラルな浮遊タグとして遅れて fade in
 *
 * 色は inline の `--handoff-color` だけ渡し、レイアウト・アニメは
 * canvas.css の `.canvas-handoff-edge__*` に集約する。
 */
import { memo, type CSSProperties } from 'react';
import { EdgeLabelRenderer, getBezierPath, type EdgeProps } from '@xyflow/react';

export interface HandoffEdgeData extends Record<string, unknown> {
  color?: string;
  preview?: string;
  fromRole?: string;
  semantic?: 'delegation' | 'report';
}

function HandoffEdgeImpl({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data
}: EdgeProps): JSX.Element {
  const [path, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition
  });
  const d = data as HandoffEdgeData | undefined;
  const color = d?.color ?? 'var(--accent, #d97757)';
  const preview = d?.preview ?? '';
  const semantic = d?.semantic;
  const colorStyle = { ['--handoff-color' as string]: color } as CSSProperties;

  return (
    <>
      <path
        id={id}
        d={path}
        pathLength={1}
        className="canvas-handoff-edge__base"
        data-semantic={semantic}
        style={colorStyle}
      />
      <path
        d={path}
        className="canvas-handoff-edge__flow"
        data-semantic={semantic}
        style={colorStyle}
      />
      {preview && (
        <EdgeLabelRenderer>
          <div
            className="canvas-handoff-edge__label"
            data-semantic={semantic}
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
              // Glass CSS contract (`.tc__hud` 以外で backdrop-filter 禁止) に従い
              // インライン style 側で blur をかける。一時 edge ラベルなので影響軽微。
              backdropFilter: 'blur(12px)'
            }}
          >
            {preview}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}

export default memo(HandoffEdgeImpl);
