use super::handle::{test_support::handle_with, SessionHandle};
use std::io::{Result as IoResult, Write};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

struct RecordingWriter(Arc<Mutex<Vec<u8>>>);

impl Write for RecordingWriter {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

pub(crate) fn recording_handle(
    agent_id: &str,
    team_id: &str,
    kills: Arc<AtomicUsize>,
) -> (SessionHandle, Arc<Mutex<Vec<u8>>>) {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let mut handle = handle_with(Some(agent_id), None, Some(team_id), kills);
    handle.writer = Mutex::new(Box::new(RecordingWriter(writes.clone())));
    (handle, writes)
}
