import { beforeEach, describe, expect, it } from 'vitest';
import type { RuntimeEventEnvelope, RuntimeEventPayload } from '../../../../types/agent-runtime';
import { RUNTIME_PROJECTION_HISTORY_LIMIT, useRuntimeStore } from '../runtime';

function envelope(
  epoch: number,
  sequence: number,
  payload: RuntimeEventPayload,
  endpointId = 'native-worker-1'
): RuntimeEventEnvelope {
  return {
    endpointId,
    epoch,
    sequence,
    kind: payload.type,
    payload,
    timestamp: `2026-07-17T00:00:${String(sequence % 60).padStart(2, '0')}.${sequence}Z`
  };
}

describe('durable runtime replay', () => {
  beforeEach(() => useRuntimeStore.getState().clear());

  it('rebuilds a projection identical to the live stream across registration epochs', () => {
    const events = [
      envelope(10, 1, { type: 'lifecycle', state: 'spawning', detail: null }),
      envelope(10, 2, { type: 'messageDelta', delta: 'hel' }),
      envelope(10, 3, { type: 'messageDelta', delta: 'lo' }),
      envelope(10, 5, { type: 'messageComplete', message: 'hello' }),
      envelope(11, 1, { type: 'lifecycle', state: 'spawning', detail: 'resume' }),
      envelope(11, 2, { type: 'lifecycle', state: 'ready', detail: null }),
      envelope(11, 4, { type: 'diagnostic', message: 'coalesced gap is expected' })
    ];
    for (const event of events) useRuntimeStore.getState().projectEvent(event);
    const live = structuredClone(useRuntimeStore.getState().byEndpoint);

    useRuntimeStore.getState().clear();
    useRuntimeStore.getState().projectEvents(events);
    expect(useRuntimeStore.getState().byEndpoint).toEqual(live);
    expect(live['native-worker-1'].missingSequences).toEqual([{ from: 3, to: 3 }]);
  });

  it('projects 10,000 events within the 100ms p95 target and keeps rendering bounded', () => {
    const events = Array.from({ length: 10_000 }, (_, index) =>
      envelope(20, index + 1, {
        type: 'diagnostic',
        message: `event-${index + 1}`
      })
    );
    const samples: number[] = [];
    for (let run = 0; run < 8; run += 1) {
      useRuntimeStore.getState().clear();
      const started = performance.now();
      useRuntimeStore.getState().projectEvents(events);
      samples.push(performance.now() - started);
    }
    samples.sort((left, right) => left - right);
    const p95 = samples[Math.ceil(samples.length * 0.95) - 1];
    const projection = useRuntimeStore.getState().byEndpoint['native-worker-1'];
    expect(projection.eventHistory).toHaveLength(RUNTIME_PROJECTION_HISTORY_LIMIT);
    expect(projection.diagnostics).toHaveLength(RUNTIME_PROJECTION_HISTORY_LIMIT);
    expect(p95).toBeLessThan(100);
  });

  it('keeps the diagnostic batch fast path equivalent across gaps and epochs', () => {
    const events = [
      envelope(30, 3, { type: 'diagnostic', message: 'coalesced first event' }),
      envelope(30, 5, { type: 'diagnostic', message: 'gap at four' }),
      envelope(30, 4, { type: 'diagnostic', message: 'out of order' }),
      envelope(31, 7, { type: 'diagnostic', message: 'new epoch first event' }),
      envelope(31, 9, { type: 'diagnostic', message: 'gap at eight' })
    ];
    for (const event of events) useRuntimeStore.getState().projectEvent(event);
    const streamed = structuredClone(useRuntimeStore.getState().byEndpoint);
    useRuntimeStore.getState().clear();
    useRuntimeStore.getState().projectEvents(events);
    expect(useRuntimeStore.getState().byEndpoint).toEqual(streamed);
  });
});
