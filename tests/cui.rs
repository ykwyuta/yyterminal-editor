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

struct Harness {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,
    worker: Option<std::thread::JoinHandle<()>>,
    screen: yy_term::Terminal,
}
impl Harness {
    fn new(path: &std::path::Path) -> Result<Self> {
        Self::with_exe(path, env!("CARGO_BIN_EXE_yyte"))
    }
    fn with_exe(path: &std::path::Path, exe: &str) -> Result<Self> {
        let pair = portable_pty::native_pty_system().openpty(PtySize {
            cols: 100,
            rows: 30,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(exe);
        command.arg(path);
        command.cwd(path.parent().unwrap());
        command.env("TERM", "xterm-256color");
        command.env("SHELL", "/bin/sh");
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut buf = [0; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            _master: pair.master,
            writer,
            rx: Some(rx),
            worker: Some(worker),
            screen: yy_term::Terminal::new(100, 30, 100),
        })
    }
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }
    fn wait_for(
        &mut self,
        label: &str,
        predicate: impl Fn(&yy_term::Terminal) -> bool,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !predicate(&self.screen) {
            let bytes = self
                .rx
                .as_ref()
                .unwrap()
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .with_context(|| {
                    format!("Waiting for {label}; screen: {}", self.screen.screen_text())
                })?;
            self.screen.feed(&bytes);
            let replies = self.screen.take_responses();
            if !replies.is_empty() {
                self.send(&replies)?;
            }
        }
        Ok(())
    }
    fn wait(&mut self, text: &str) -> Result<()> {
        self.wait_for(text, |screen| screen.screen_text().contains(text))
    }
    fn command(&mut self, name: &str) -> Result<()> {
        self.send(b"\x1bOP")?;
        self.wait("Commands: arrows")?;
        let highlighted = |screen: &yy_term::Terminal| -> String {
            (0..screen.rows())
                .filter_map(|i| {
                    let row = screen.screen_row(i);
                    row.cells
                        .iter()
                        .any(|c| c.attr.bg == yy_term::Color::Indexed(4))
                        .then(|| row.text())
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        for _ in 0..128 {
            let current = highlighted(&self.screen);
            if current.contains(name) {
                self.send(b"\r")?;
                return Ok(());
            }
            self.send(b"\x1bOB")?;
            self.wait_for("menu selection", |screen| highlighted(screen) != current)?;
        }
        anyhow::bail!("Menu command missing: {name}")
    }
    fn finish(&mut self) -> Result<()> {
        self.send(b"\x11")?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait()? {
                ensure!(status.success(), "Exit failed: {status:?}");
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "Exit timed out: {}",
                self.screen.screen_text()
            );
            if let Ok(bytes) = self
                .rx
                .as_ref()
                .unwrap()
                .recv_timeout(Duration::from_millis(100))
            {
                self.screen.feed(&bytes);
            }
        }
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        self.rx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[test]
fn interactive_csv_table_edits_and_saves_quoted_multiline_cell() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("data.csv");
    std::fs::write(&path, b"id,note\r\n1,original\r\n")?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.send(b"\x1b[18~")?;
    app.wait("Table ready")?;
    app.send(b"\x1bOB\x1bOC\r")?;
    app.wait("Edit cell")?;
    app.send("\x01\x1b[200~changed,\nline\x1b[201~\r".as_bytes())?;
    app.wait_for("updated table", |screen| {
        let text = screen.screen_text();
        text.contains("Table ready") && !text.contains("Edit cell") && text.contains("changed,")
    })?;
    app.send(b"\x13")?;
    app.wait("Saved")?;
    ensure!(
        std::fs::read(&path)? == b"id,note\r\n1,\"changed,\nline\"\r\n",
        "CSV quoting or line endings changed"
    );
    app.finish()
}
#[test]
fn interactive_fixed_table_edits_packed_decimal_and_preserves_other_bytes() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("records.dat");
    let bytes = [b'A', b'B', b'C', b'D', 0x12, 0x34, 0x5c, 0xde, 0xad];
    std::fs::write(&path, bytes)?;
    std::fs::write(
        dir.path().join("record.cpy"),
        "01 REC.\n 05 ID PIC X(4).\n 05 AMOUNT PIC S9(5) COMP-3.\n",
    )?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.command("Fixed-length table")?;
    app.wait("COPYBOOK;RECORD_BYTES")?;
    app.send(b"record.cpy;9;MS932;none\r")?;
    app.wait("Table ready")?;
    app.wait("Fixed 9 bytes")?;
    app.send(b"\x1bOC\r")?;
    app.wait("Edit cell")?;
    app.send(b"\x01100000\r")?;
    app.wait("Value rejected")?;
    ensure!(
        std::fs::read(&path)? == bytes,
        "Rejected input wrote to disk"
    );
    app.send(b"\r")?;
    app.wait("Edit cell")?;
    app.send(b"\x0142\r")?;
    app.wait_for("packed decimal changed", |screen| {
        let text = screen.screen_text();
        text.contains("Table ready") && !text.contains("Edit cell") && text.contains("42")
    })?;
    app.send(b"\x13")?;
    app.wait("Saved")?;
    let result = std::fs::read(&path)?;
    ensure!(
        result[..4] == bytes[..4] && result[7..] == bytes[7..],
        "Untouched fields changed"
    );
    ensure!(
        result[4..7] == [0x00, 0x04, 0x2c],
        "Packed decimal was not encoded: {result:?}"
    );
    app.command("Export fixed-length")?;
    app.wait("Export fixed-length data")?;
    app.send(b"exported.dat\r")?;
    app.wait("Fixed-length export complete")?;
    ensure!(
        std::fs::read(dir.path().join("exported.dat"))? == result,
        "Fixed export changed raw bytes"
    );
    app.finish()
}

#[test]
fn interactive_workbook_sizes_formulas_save_and_yymerge_diff() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("data.csv");
    std::fs::write(&path, b"name,value\r\nalpha,1\r\n")?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.command("Spreadsheet mode")?;
    app.wait("Spreadsheet ready")?;
    app.send(b"\x1bOB\x1bOC\r")?;
    app.wait("Cell / formula")?;
    app.send(b"\x01=2*3\r")?;
    app.wait_for("formula result", |s| {
        let t = s.screen_text();
        t.contains("Spreadsheet ready")
            && !t.contains("[Working]")
            && t.contains("B2: =2*3")
            && !t.contains("Cell / formula")
    })?;
    app.command("Row height")?;
    app.wait("Row height: 1..20")?;
    app.send(b"3\r")?;
    app.wait_for("row height applied", |s| {
        let t = s.screen_text();
        t.contains("Spreadsheet ready")
            && !t.contains("[Working]")
            && !t.contains("Row height: 1..20")
    })?;
    app.command("Column width")?;
    app.wait("Column width: 4..80")?;
    app.send(b"28\r")?;
    app.wait_for("column width applied", |s| {
        let t = s.screen_text();
        t.contains("Spreadsheet ready")
            && !t.contains("[Working]")
            && !t.contains("Column width: 4..80")
    })?;
    app.command("Save as")?;
    app.wait("Save as path")?;
    app.send(b"book.yys\r")?;
    app.wait_for("workbook saved", |s| {
        let t = s.screen_text();
        t.contains("book.yys]")
            && t.contains("Spreadsheet ready")
            && !t.contains("[Working]")
            && !t.contains("Save as path")
    })?;
    ensure!(dir.path().join("book.yys").is_file(), "No workbook file");
    app.command("Compare file (yymerge)")?;
    app.wait("yymerge: compare")?;
    app.send(b"data.csv\r")?;
    app.wait("1 changed")?;
    app.wait("yymerge comparison")?;
    app.send(b"\x1bOQ")?;
    app.wait("B2: =2*3")?;
    app.finish()
}

#[test]
fn interactive_preview_renders_local_pixels_and_mermaid() -> Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/markdown-preview.md");
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.send(b"\x1b[15~")?;
    app.wait("Markdown preview ready")?;
    app.wait("Image: Pixel house")?;
    app.send(b"\x1b[6~")?;
    app.wait("Connections:")?;
    app.send(b"\x1bOQ")?;
    app.wait("Terminal Markdown preview")?;
    app.finish()
}

#[test]
fn standalone_interactive_preview_resizes_and_exits() -> Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/markdown-preview.md");
    let mut app = Harness::with_exe(&path, env!("CARGO_BIN_EXE_markdownViewer"))?;
    app.wait("Ready")?;
    app.wait("Image: Pixel house")?;
    app._master.resize(PtySize {
        cols: 60,
        rows: 18,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    app.screen.resize(60, 18);
    app.wait_for("resized viewer", |s| {
        s.rows() == 18 && s.screen_text().contains("60 cols") && s.screen_text().contains("Ready")
    })?;
    app.send(b"q")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = app.child.try_wait()? {
            ensure!(status.success(), "Viewer exit failed");
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Viewer did not exit: {}",
            app.screen.screen_text()
        );
        if let Ok(bytes) = app
            .rx
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_millis(100))
        {
            app.screen.feed(&bytes);
        }
    }
    Ok(())
}
