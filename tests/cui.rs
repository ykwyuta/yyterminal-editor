#![cfg(unix)]
use anyhow::{Context, Result, ensure};
use portable_pty::{CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn interactive_editor_save_preview_shell_resize_and_exit() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("日本 語.md");
    std::fs::write(&path, b"# Heading\noriginal\n")?;
    let pair = portable_pty::native_pty_system().openpty(PtySize {
        cols: 100,
        rows: 30,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_yyte"));
    command.arg(&path);
    command.cwd(dir.path());
    command.env("TERM", "xterm-256color");
    command.env("SHELL", "/bin/sh");
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut buf = [0; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let result = (|| -> Result<()> {
        let mut screen = yy_term::Terminal::new(100, 30, 100);
        let mut wait = |needle: &str| -> Result<()> {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if screen.screen_text().contains(needle) {
                    return Ok(());
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                let bytes = rx.recv_timeout(remaining).with_context(|| {
                    format!("Waiting for {needle:?}; screen:\n{}", screen.screen_text())
                })?;
                screen.feed(&bytes);
                let replies = screen.take_responses();
                if !replies.is_empty() {
                    writer.write_all(&replies)?;
                    writer.flush()?;
                }
            }
        };
        wait("Opened")?;
        // End the temporary closure borrow before sending input.
        drop(wait);
        writer.write_all("\x1b[200~日本語\x1b[201~\x13".as_bytes())?;
        writer.flush()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let bytes = rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
            screen.feed(&bytes);
            if screen.screen_text().contains("Saved") {
                break;
            }
        }
        ensure!(
            std::fs::read_to_string(&path)? == "日本語# Heading\noriginal\n",
            "Saved bytes differ"
        );
        // F5 preview, F4 shell, then assemble a marker so input echo cannot pass.
        writer.write_all(
            b"\x1b[15~\x1bOSprintf '\\131\\131\\123\\110\\105\\114\\114\\117\\113\\n'\r",
        )?;
        writer.flush()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            screen.feed(&rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))?);
            if screen.screen_text().contains("YYSHELLOK") {
                break;
            }
        }
        pair.master.resize(PtySize {
            cols: 60,
            rows: 15,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        screen.resize(60, 15);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !screen.screen_row(13).text().contains('┘') {
            screen.feed(&rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))?);
        }
        writer.write_all(b"\x1bOQ")?;
        writer.flush()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !screen.screen_text().contains("日本 語.md") {
            screen.feed(
                &rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .with_context(|| format!("Editor switch: {}", screen.screen_text()))?,
            );
        }
        writer.write_all(b"\x11")?;
        writer.flush()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait()? {
                ensure!(status.success(), "Application failed: {status:?}");
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Application did not exit; screen: {}",
                screen.screen_text()
            );
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(bytes) => screen.feed(&bytes),
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    drop(writer);
    drop(rx);
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("Reader panicked"))?;
    result
}
