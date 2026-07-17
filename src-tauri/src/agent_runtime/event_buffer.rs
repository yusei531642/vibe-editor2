//! Bounded runtime event history with delta-first eviction.

use super::{RuntimeEventEnvelope, RuntimeEventPayload};
use std::collections::VecDeque;

pub const DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY: usize = 256;

pub struct RuntimeEventBuffer {
    capacity: usize,
    events: VecDeque<RuntimeEventEnvelope>,
    dropped_count: u64,
}

impl RuntimeEventBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            events: VecDeque::with_capacity(capacity.max(1)),
            dropped_count: 0,
        }
    }

    pub fn push(&mut self, event: RuntimeEventEnvelope) {
        if self.try_coalesce_delta(&event) {
            return;
        }
        if self.events.len() == self.capacity {
            if let Some(index) = self.events.iter().position(|candidate| {
                matches!(candidate.payload, RuntimeEventPayload::MessageDelta { .. })
            }) {
                self.events.remove(index);
                self.dropped_count = self.dropped_count.saturating_add(1);
            } else if matches!(event.payload, RuntimeEventPayload::MessageDelta { .. }) {
                self.dropped_count = self.dropped_count.saturating_add(1);
                return;
            } else {
                self.events.pop_front();
                self.dropped_count = self.dropped_count.saturating_add(1);
            }
        }
        self.events.push_back(event);
    }

    /// Consecutive deltas are represented by one stored envelope whose `sequence` and `timestamp`
    /// are rewritten to the latest input. A later snapshot replay can therefore report the
    /// coalesced sequence range as missing even though its text is present in the merged payload.
    fn try_coalesce_delta(&mut self, incoming: &RuntimeEventEnvelope) -> bool {
        let Some(last) = self.events.back_mut() else {
            return false;
        };
        if last.endpoint_id != incoming.endpoint_id {
            return false;
        }
        match (&mut last.payload, &incoming.payload) {
            (
                RuntimeEventPayload::MessageDelta { delta: current },
                RuntimeEventPayload::MessageDelta { delta: next },
            ) => {
                current.push_str(next);
                last.sequence = incoming.sequence;
                last.timestamp.clone_from(&incoming.timestamp);
                true
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn snapshot(&self) -> Vec<RuntimeEventEnvelope> {
        self.events.iter().cloned().collect()
    }

    #[allow(dead_code)]
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Default for RuntimeEventBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY)
    }
}
