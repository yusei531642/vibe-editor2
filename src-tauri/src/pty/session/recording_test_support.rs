use super::handle::{test_support::handle_with, SessionHandle};
use std::io::{Result as IoResult, Write};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

struct RecordingWriter(Arc<Mutex<Vec<u8>>>);

struct FailAfterWriter {
    writes: Arc<Mutex<Vec<u8>>>,
    successful_writes: usize,
    fail_after: usize,
}

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

impl Write for FailAfterWriter {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        if self.successful_writes >= self.fail_after {
            return Err(std::io::Error::other("recording writer failure"));
        }
        self.successful_writes += 1;
        self.writes
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

pub(crate) fn failing_recording_handle(
    agent_id: &str,
    team_id: &str,
    kills: Arc<AtomicUsize>,
    fail_after: usize,
) -> (SessionHandle, Arc<Mutex<Vec<u8>>>) {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let mut handle = handle_with(Some(agent_id), None, Some(team_id), kills);
    handle.writer = Mutex::new(Box::new(FailAfterWriter {
        writes: writes.clone(),
        successful_writes: 0,
        fail_after,
    }));
    (handle, writes)
}
