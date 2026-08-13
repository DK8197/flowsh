//! Low-level wrapper around a single persistent `bash` process running
//! inside a pseudo-terminal. This module only knows how to start bash,
//! write bytes to it, and read bytes back. Prompt-detection / command
//! framing lives in [`super::manager`].

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

pub struct BashProcess {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    reader: Option<Box<dyn Read + Send>>,
    pub child: Box<dyn Child + Send + Sync>,
}

impl BashProcess {
    /// Spawn a new persistent bash process attached to a fresh PTY.
    pub fn spawn() -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 40,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open pty")?;

        let mut cmd = CommandBuilder::new("bash");
        cmd.arg("--noprofile");
        cmd.arg("--norc");
        cmd.arg("--noediting");
        // A minimal, predictable prompt so the manager can detect completion.
        cmd.env("PS1", "");
        cmd.env("TERM", "xterm-256color");
        // Point this session's history at /dev/null so it's fully
        // isolated from the user's real shell history. Without this,
        // HISTFILE was unset here, meaning the internal bash session
        // inherited whatever HISTFILE the *user's own outer terminal*
        // has — the same ~/.bash_history a normal interactive shell
        // uses. The two sessions ended up reading and writing the same
        // file, interleaving shdev's (already ignorespace-hidden, but
        // still file-shared) history with the user's real one. Reading
        // /dev/null returns EOF immediately (empty history at startup);
        // writing to it discards everything — no persistence, no
        // sharing, no interleaving, in either direction.
        cmd.env("HISTFILE", "/dev/null");

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn bash")?;

        let writer = pair.master.take_writer().context("failed to take pty writer")?;
        let reader = pair.master.try_clone_reader().context("failed to clone pty reader")?;

        Ok(Self {
            master: pair.master,
            writer,
            reader: Some(reader),
            child,
        })
    }

    pub fn write_line(&mut self, line: &str) -> Result<()> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Take ownership of the reader half so it can be moved into a
    /// dedicated background thread. May only be called once.
    pub fn take_reader(&mut self) -> Box<dyn Read + Send> {
        self.reader.take().expect("reader already taken")
    }

    pub fn kill(&mut self) -> Result<()> {
        let _ = self.child.kill();
        Ok(())
    }
}
