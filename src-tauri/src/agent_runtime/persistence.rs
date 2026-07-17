//! Append-only runtime event history. SQLite access is isolated on one writer thread.

use super::RuntimeEventEnvelope;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

const SCHEMA_VERSION: i64 = 2;
const DATABASE_NAME: &str = "runtime-events.db";

#[derive(Clone, Debug)]
pub struct PersistedRuntimeBinding {
    pub project_root: String,
    pub team_id: String,
    pub agent_id: String,
    pub endpoint_id: String,
    pub epoch: u64,
    pub provider: String,
    pub resume_id: Option<String>,
    pub resumable: bool,
}

pub struct RuntimeTeamBinding<'a> {
    pub project_root: Option<&'a str>,
    pub team_id: &'a str,
    pub agent_id: &'a str,
    pub endpoint_id: &'a str,
    pub provider: &'a str,
    pub resume_id: Option<String>,
    pub resumable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeRestoreSnapshot {
    pub team_id: Option<String>,
    pub bindings: Vec<PersistedRuntimeBinding>,
    pub events: Vec<RuntimeEventEnvelope>,
}

enum WriterMessage {
    Append(RuntimeEventEnvelope),
    Bind(PersistedRuntimeBinding),
    Restore {
        project_root: String,
        reply: Sender<Result<RuntimeRestoreSnapshot, String>>,
    },
}

#[derive(Clone)]
pub struct RuntimeEventPersistence {
    sender: Sender<WriterMessage>,
}

impl RuntimeEventPersistence {
    #[cfg_attr(test, allow(dead_code))]
    pub fn start_default() -> Result<Self> {
        let base = dirs::home_dir()
            .ok_or_else(|| anyhow!("home directory is unavailable"))?
            .join(".vibe-editor2");
        Self::start(base.join(DATABASE_NAME))
    }

    pub fn start(path: PathBuf) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("runtime-event-writer".to_string())
            .spawn(move || writer_loop(path, receiver))
            .context("spawn runtime event writer")?;
        Ok(Self { sender })
    }

    pub fn append(&self, event: RuntimeEventEnvelope) {
        if self.sender.send(WriterMessage::Append(event)).is_err() {
            tracing::warn!("[runtime-persistence] writer is unavailable");
        }
    }

    pub fn bind(&self, binding: PersistedRuntimeBinding) {
        if self.sender.send(WriterMessage::Bind(binding)).is_err() {
            tracing::warn!("[runtime-persistence] writer is unavailable");
        }
    }

    pub fn restore_latest(&self, project_root: &str) -> Result<RuntimeRestoreSnapshot, String> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(WriterMessage::Restore {
                project_root: project_root.to_string(),
                reply: sender,
            })
            .map_err(|_| "runtime event writer is unavailable".to_string())?;
        receiver
            .recv()
            .map_err(|_| "runtime event writer stopped during restore".to_string())?
    }
}

fn writer_loop(path: PathBuf, receiver: Receiver<WriterMessage>) {
    let connection = match open_with_recovery(&path) {
        Ok(connection) => connection,
        Err(error) => {
            let message = format!("runtime database startup failed: {error:#}");
            tracing::warn!("[runtime-persistence] {message}");
            while let Ok(pending) = receiver.recv() {
                if let WriterMessage::Restore { reply, .. } = pending {
                    let _ = reply.send(Err(message.clone()));
                }
            }
            return;
        }
    };
    while let Ok(message) = receiver.recv() {
        let result = match message {
            WriterMessage::Append(event) => append_event(&connection, &event),
            WriterMessage::Bind(binding) => bind_session(&connection, &binding),
            WriterMessage::Restore {
                project_root,
                reply,
            } => {
                let result = read_latest(&connection, &project_root)
                    .map_err(|error| error.to_string());
                let _ = reply.send(result);
                continue;
            }
        };
        if let Err(error) = result {
            tracing::warn!("[runtime-persistence] writer operation failed: {error:#}");
        }
    }
}

fn open_with_recovery(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    match open_database(path) {
        Ok(connection) => Ok(connection),
        Err(first_error) => {
            quarantine(path).with_context(|| {
                format!("quarantine unreadable runtime database: {first_error:#}")
            })?;
            tracing::warn!(
                path = %path.display(),
                "[runtime-persistence] corrupt database quarantined; creating a fresh database"
            );
            open_database(path)
        }
    }
}

fn open_database(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)
        .with_context(|| format!("open runtime database {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    let integrity: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .context("runtime database quick_check")?;
    if integrity != "ok" {
        return Err(anyhow!(
            "runtime database integrity check failed: {integrity}"
        ));
    }
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS schema_version(version INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS runtime_events(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           endpoint_id TEXT NOT NULL,
           epoch INTEGER NOT NULL,
           sequence INTEGER NOT NULL,
           kind TEXT NOT NULL,
           envelope_json TEXT NOT NULL,
           timestamp TEXT NOT NULL,
           team_id TEXT,
           UNIQUE(endpoint_id, epoch, sequence)
         );
         CREATE INDEX IF NOT EXISTS runtime_events_team_order
           ON runtime_events(team_id, id);
         CREATE TABLE IF NOT EXISTS runtime_sessions(
           project_root TEXT NOT NULL,
           team_id TEXT NOT NULL,
           agent_id TEXT NOT NULL,
           endpoint_id TEXT NOT NULL,
           epoch INTEGER NOT NULL,
           provider TEXT NOT NULL,
           resume_id TEXT,
           resumable INTEGER NOT NULL,
           started_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           PRIMARY KEY(endpoint_id, epoch)
         );
         CREATE INDEX IF NOT EXISTS runtime_sessions_latest
           ON runtime_sessions(project_root, updated_at DESC);",
    )?;
    let version: Option<i64> = connection
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    match version {
        None => {
            connection.execute(
                "INSERT INTO schema_version(version) VALUES (?1)",
                [SCHEMA_VERSION],
            )?;
        }
        Some(version) if version == SCHEMA_VERSION => {}
        Some(version) => return Err(anyhow!("unsupported runtime database schema {version}")),
    }
    validate_event_rows(&connection)?;
    Ok(connection)
}

fn validate_event_rows(connection: &Connection) -> Result<()> {
    // Startup cost must not grow with append-only history. `quick_check` above validates the
    // SQLite structure; bounded recent-row parsing catches malformed envelopes without a full
    // table scan. Older malformed rows remain isolated to the scoped restore that reads them.
    let mut statement = connection.prepare(
        "SELECT id, envelope_json FROM runtime_events ORDER BY id DESC LIMIT 256",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, raw) = row?;
        crate::commands::safe_load::parse_persisted_container_json::<RuntimeEventEnvelope>(
            raw.as_bytes(),
        )
        .with_context(|| format!("runtime event row {id} is invalid"))?;
    }
    Ok(())
}

fn quarantine(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let suffix = chrono::Utc::now().format("%Y%m%dT%H%M%S%.fZ");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(DATABASE_NAME);
    let quarantined = path.with_file_name(format!("{file_name}.corrupt-{suffix}"));
    std::fs::rename(path, quarantined)?;
    for sidecar in ["-wal", "-shm"] {
        let source = PathBuf::from(format!("{}{sidecar}", path.display()));
        if source.exists() {
            let target = PathBuf::from(format!("{}.corrupt-{suffix}", source.display()));
            let _ = std::fs::rename(source, target);
        }
    }
    Ok(())
}

fn append_event(connection: &Connection, event: &RuntimeEventEnvelope) -> Result<()> {
    let envelope = serde_json::to_string(event)?;
    let kind = serde_json::to_value(event.kind)?
        .as_str()
        .ok_or_else(|| anyhow!("runtime event kind did not serialize as a string"))?
        .to_string();
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO runtime_events
         (endpoint_id, epoch, sequence, kind, envelope_json, timestamp, team_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6,
           (SELECT team_id FROM runtime_sessions WHERE endpoint_id=?1 AND epoch=?2))",
        params![
            event.endpoint_id,
            event.epoch,
            event.sequence,
            kind,
            envelope,
            event.timestamp
        ],
    )?;
    if inserted > 0 {
        connection.execute(
            "UPDATE runtime_sessions SET updated_at=?3 WHERE endpoint_id=?1 AND epoch=?2",
            params![event.endpoint_id, event.epoch, event.timestamp],
        )?;
    }
    Ok(())
}

fn bind_session(connection: &Connection, binding: &PersistedRuntimeBinding) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let started_at: Option<String> = connection
        .query_row(
            "SELECT MIN(timestamp) FROM runtime_events WHERE endpoint_id=?1 AND epoch=?2",
            params![binding.endpoint_id, binding.epoch],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "INSERT INTO runtime_sessions
         (project_root, team_id, agent_id, endpoint_id, epoch, provider, resume_id, resumable, started_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(endpoint_id, epoch) DO UPDATE SET
           project_root=excluded.project_root, team_id=excluded.team_id,
           agent_id=excluded.agent_id,
           provider=excluded.provider, resume_id=excluded.resume_id,
           resumable=excluded.resumable, updated_at=excluded.updated_at",
        params![
            binding.project_root,
            binding.team_id,
            binding.agent_id,
            binding.endpoint_id,
            binding.epoch,
            binding.provider,
            binding.resume_id,
            binding.resumable,
            started_at.unwrap_or_else(|| now.clone()),
            now
        ],
    )?;
    transaction.execute(
        "UPDATE runtime_events SET team_id=?1 WHERE endpoint_id=?2 AND epoch=?3",
        params![binding.team_id, binding.endpoint_id, binding.epoch],
    )?;
    transaction.commit()?;
    Ok(())
}

fn read_latest(connection: &Connection, project_root: &str) -> Result<RuntimeRestoreSnapshot> {
    let team_id: Option<String> = connection
        .query_row(
            "SELECT team_id FROM runtime_sessions
             WHERE project_root=?1 ORDER BY updated_at DESC LIMIT 1",
            [project_root],
            |row| row.get(0),
        )
        .optional()?;
    let Some(team_id) = team_id else {
        return Ok(RuntimeRestoreSnapshot::default());
    };
    let mut binding_statement = connection.prepare(
        "SELECT project_root, team_id, agent_id, endpoint_id, epoch, provider, resume_id, resumable
         FROM runtime_sessions WHERE project_root=?1 AND team_id=?2 ORDER BY epoch DESC",
    )?;
    let rows = binding_statement.query_map(params![project_root, &team_id], |row| {
        Ok(PersistedRuntimeBinding {
            project_root: row.get(0)?,
            team_id: row.get(1)?,
            agent_id: row.get(2)?,
            endpoint_id: row.get(3)?,
            epoch: row.get(4)?,
            provider: row.get(5)?,
            resume_id: row.get(6)?,
            resumable: row.get(7)?,
        })
    })?;
    let mut bindings = Vec::new();
    let mut seen_agents = std::collections::HashSet::new();
    for row in rows {
        let binding = row?;
        if seen_agents.insert(binding.agent_id.clone()) {
            bindings.push(binding);
        }
    }
    bindings.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    let mut event_statement = connection
        .prepare(
            "SELECT event.envelope_json FROM runtime_events event
             INNER JOIN runtime_sessions session
               ON session.endpoint_id=event.endpoint_id AND session.epoch=event.epoch
             WHERE session.project_root=?1 AND event.team_id=?2 ORDER BY event.id",
        )?;
    let event_rows = event_statement
        .query_map(params![project_root, &team_id], |row| row.get::<_, String>(0))?;
    let mut events = Vec::new();
    for row in event_rows {
        let raw = row?;
        events.push(
            crate::commands::safe_load::parse_persisted_container_json::<RuntimeEventEnvelope>(
                raw.as_bytes(),
            )?,
        );
    }
    Ok(RuntimeRestoreSnapshot {
        team_id: Some(team_id),
        bindings,
        events,
    })
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
