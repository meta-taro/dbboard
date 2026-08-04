//! Worker-side backup (logical dump) plumbing (ADR-0049, slice e).
//!
//! The pure dump orchestrator and its preflight live in `dbboard-core`
//! ([`run_dump`], [`plan_dump`]). This module supplies the two worker-side
//! pieces they need — the ones that genuinely touch I/O and the UI channel,
//! and so cannot live in the domain layer:
//!
//! - [`ChannelControl`] — a [`DumpControl`] that forwards each
//!   [`DumpProgress`] to the UI as [`Reply::BackupProgress`] (waking the
//!   egui frame) and reads cancellation off a [`CancellationToken`], the
//!   same token a [`Command::CancelBackup`](crate::Command) cancels.
//! - [`FileSink`] — a buffered-file [`DumpSink`].
//!
//! [`run_backup`] wires them together for one dump and is the body of the
//! task the worker spawns, mirroring the AI streaming tasks: it emits
//! progress as it goes and a single terminal [`Reply`].

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use dbboard_core::{
    run_dump, DatabaseAdapter, DumpControl, DumpError, DumpPlan, DumpProgress, DumpResult,
    DumpSink, SqlDialect,
};
use eframe::egui;
use tokio_util::sync::CancellationToken;

use crate::Reply;

/// A [`DumpControl`] that bridges the orchestrator to the UI: every
/// progress snapshot becomes a [`Reply::BackupProgress`] and wakes the
/// frame; cancellation is read off the shared token.
struct ChannelControl {
    reply_tx: Sender<Reply>,
    ctx: egui::Context,
    cancel: CancellationToken,
}

impl DumpControl for ChannelControl {
    fn report(&self, progress: &DumpProgress) {
        // A closed channel means the UI is gone; the terminal reply send
        // will no-op too, and the dump finishes into the void harmlessly.
        let _ = self.reply_tx.send(Reply::BackupProgress {
            progress: progress.clone(),
        });
        self.ctx.request_repaint();
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// A buffered-file [`DumpSink`]. `write_str` maps any I/O failure to
/// [`DumpError::Sink`], which aborts the dump (a backup we cannot write is
/// worthless), and [`Self::finish`] flushes the buffer so a clean run
/// leaves a complete file.
struct FileSink {
    writer: BufWriter<File>,
}

impl FileSink {
    fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            writer: BufWriter::new(File::create(path)?),
        })
    }

    fn finish(mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl DumpSink for FileSink {
    fn write_str(&mut self, chunk: &str) -> DumpResult<()> {
        self.writer
            .write_all(chunk.as_bytes())
            .map_err(|e| DumpError::Sink(e.to_string()))
    }
}

/// Run one backup to `path` and answer with a single terminal [`Reply`].
///
/// Emits [`Reply::BackupProgress`] throughout via [`ChannelControl`]. A
/// cancellation observed mid-run is not an error: [`run_dump`] returns an
/// outcome with `cancelled == true`, surfaced as [`Reply::BackupComplete`]
/// so the UI can report the partial file honestly. Only an unopenable or
/// unwritable output is a [`Reply::BackupFailed`].
pub(crate) async fn run_backup(
    adapter: Arc<dyn DatabaseAdapter>,
    dialect: SqlDialect,
    plan: DumpPlan,
    path: PathBuf,
    cancel: CancellationToken,
    reply_tx: Sender<Reply>,
    ctx: egui::Context,
) {
    let mut sink = match FileSink::create(&path) {
        Ok(sink) => sink,
        Err(e) => {
            send(
                &reply_tx,
                &ctx,
                Reply::BackupFailed {
                    message: e.to_string(),
                },
            );
            return;
        }
    };

    let control = ChannelControl {
        reply_tx: reply_tx.clone(),
        ctx: ctx.clone(),
        cancel,
    };

    let reply = match run_dump(adapter.as_ref(), dialect, &plan, &mut sink, &control).await {
        Ok(outcome) => match sink.finish() {
            Ok(()) => Reply::BackupComplete { outcome },
            Err(e) => Reply::BackupFailed {
                message: e.to_string(),
            },
        },
        Err(DumpError::Sink(message)) => Reply::BackupFailed { message },
    };
    send(&reply_tx, &ctx, reply);
}

fn send(reply_tx: &Sender<Reply>, ctx: &egui::Context, reply: Reply) {
    let _ = reply_tx.send(reply);
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_core::{DumpControl, DumpSink};
    use std::sync::mpsc;

    #[test]
    fn file_sink_writes_and_flushes_a_complete_file() {
        let dir = std::env::temp_dir().join(format!("dbboard-backup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dump.sql");

        let mut sink = FileSink::create(&path).unwrap();
        sink.write_str("-- header\n").unwrap();
        sink.write_str("INSERT INTO t VALUES (1);\n").unwrap();
        sink.finish().unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "-- header\nINSERT INTO t VALUES (1);\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn file_sink_create_on_a_bad_path_is_an_io_error() {
        // A path whose parent directory does not exist cannot be created.
        let path = Path::new("no_such_dir_dbboard")
            .join("nested")
            .join("x.sql");
        assert!(FileSink::create(&path).is_err());
    }

    #[test]
    fn channel_control_forwards_progress_and_reads_cancellation() {
        let (tx, rx) = mpsc::channel();
        let cancel = CancellationToken::new();
        let control = ChannelControl {
            reply_tx: tx,
            ctx: egui::Context::default(),
            cancel: cancel.clone(),
        };

        assert!(!control.is_cancelled());
        control.report(&DumpProgress {
            tables_total: 2,
            tables_done: 1,
            rows_total: 100,
            rows_done: 40,
            current_table: Some("t".into()),
        });
        cancel.cancel();
        assert!(control.is_cancelled());

        match rx.try_recv().unwrap() {
            Reply::BackupProgress { progress } => {
                assert_eq!(progress.rows_done, 40);
                assert_eq!(progress.current_table.as_deref(), Some("t"));
            }
            other => panic!("expected BackupProgress, got {other:?}"),
        }
    }
}
