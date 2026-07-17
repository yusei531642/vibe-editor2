import type { TeamProjectionSnapshot, TeamRuntimeEventCursor } from '../../../types/shared';
import { useRuntimeStore } from '../stores/runtime';

export function projectBufferedEvents(
  snapshot: TeamProjectionSnapshot,
  replayedEvents: Set<string>
): void {
  const store = useRuntimeStore.getState();
  // Rust の buffer 順は epoch をまたいだ発生順。sequence は登録 epoch ごとに 1 へ戻るため、
  // sequence sort すると旧/new epoch が混ざる。snapshot の canonical order を維持する。
  for (const event of snapshot.runtimeEvents) {
    const current = useRuntimeStore.getState().byEndpoint[event.endpointId];
    const eventKey = `${event.endpointId}\u0000${event.epoch}\u0000${event.sequence}\u0000${event.timestamp}`;
    const alreadyProjected = current?.eventHistory.some(
      (projected) =>
        projected.epoch === event.epoch &&
        projected.sequence === event.sequence &&
        projected.timestamp === event.timestamp
    );
    if (replayedEvents.has(eventKey) || alreadyProjected) {
      replayedEvents.add(eventKey);
      continue;
    }
    const startsEpoch =
      event.payload.type === 'lifecycle' && event.payload.state === 'spawning';
    if (
      !current ||
      event.epoch > current.epoch ||
      event.sequence > current.lastSequence ||
      startsEpoch
    ) {
      store.projectEvent(event);
    }
    replayedEvents.add(eventKey);
  }
}

function cursorKey(cursor: TeamRuntimeEventCursor): string {
  return `${cursor.endpointId}\u0000${cursor.epoch}\u0000${cursor.sequence}\u0000${cursor.timestamp}`;
}

export function pruneReplayedEvents(
  snapshot: TeamProjectionSnapshot,
  replayedEvents: Set<string>
): void {
  const retained = new Set(snapshot.retainedEventCursors.map(cursorKey));
  for (const key of replayedEvents) {
    if (!retained.has(key)) replayedEvents.delete(key);
  }
}

export function latestCursors(snapshot: TeamProjectionSnapshot): TeamRuntimeEventCursor[] {
  const cursors = new Map<string, TeamRuntimeEventCursor>();
  for (const cursor of snapshot.retainedEventCursors) cursors.set(cursor.endpointId, cursor);
  return [...cursors.values()];
}

export function snapshotsEqual(
  previous: TeamProjectionSnapshot | null,
  next: TeamProjectionSnapshot
): boolean {
  if (!previous) return false;
  return (
    previous.teamId === next.teamId &&
    previous.runtimeDroppedCount === next.runtimeDroppedCount &&
    JSON.stringify(previous.endpoints) === JSON.stringify(next.endpoints)
  );
}

export function valuesEqual<T>(previous: T, next: T): boolean {
  return JSON.stringify(previous) === JSON.stringify(next);
}
