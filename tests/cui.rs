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
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.send("\x1b[200~日本語\x1b[201~\x13".as_bytes())?;
    app.wait("Saved")?;
    ensure!(
        std::fs::read_to_string(&path)? == "日本語# Heading\noriginal\n",
        "Saved bytes differ"
    );
    app.send(b"\x1b[15~")?;
    app.wait("Markdown preview ready")?;
    app.send(b"\x1bOS")?;
    app.wait_for("active terminal view", |s| {
        s.screen_text().contains("┌Terminal")
    })?;
    // Assemble a marker so input echo cannot satisfy the shell completion check.
    app.send(b"printf '\\131\\131\\123\\110\\105\\114\\114\\117\\113\\n'\r")?;
    app.wait("YYSHELLOK")?;
    app._master.resize(PtySize {
        cols: 60,
        rows: 15,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    app.screen.resize(60, 15);
    app.wait_for("resized terminal frame", |s| {
        s.screen_row(13).text().contains('┘')
            && s.screen_text().contains("┌Terminal")
            && s.screen_text().contains("YYSHELLOK")
    })?;
    app.send(b"\x1bOQ")?;
    // The file name is always in the tab strip, including while Terminal is active.
    // Wait for the actual editor view, document content and resized frame instead.
    app.wait_for("editor restored after terminal resize", |s| {
        let text = s.screen_text();
        text.contains("┌Editor")
            && text.contains("日本語# Heading")
            && text.contains("original")
            && text.contains("[日本 語.md]")
            && s.screen_row(13).text().contains('┘')
    })?;
    app.finish()
}

#[test]
fn long_paste_saves_all_lines_with_and_without_bracketed_paste() -> Result<()> {
    for bracketed in [false, true] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("paste.txt");
        std::fs::write(&path, "tail")?;
        let mut app = Harness::new(&path)?;
        app.wait("Opened")?;
        let text = "日本語 long paste line\n".repeat(2000);
        let payload = if bracketed {
            format!("\x1b[200~{text}\x1b[201~\x13")
        } else {
            format!("{}\x13", text.replace('\n', "\r"))
        };
        app.send_bounded(payload.as_bytes())?;
        app.wait("Saved")?;
        let expected = format!("{text}tail");
        ensure!(
            std::fs::read_to_string(&path)? == expected,
            "Paste content differs (bracketed={bracketed}); screen: {}",
            app.screen.screen_text()
        );
        app.finish()?;
    }
    Ok(())
}

#[test]
fn outline_json_hierarchy_references_undo_export_and_save_in_real_terminal() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("notes.yyoutline");
    std::fs::write(&path, include_str!("../demo/outline-notes.yyoutline"))?;
    let mut app = Harness::new(&path)?;
    app.wait("Outline ready")?;
    app.wait("Outline notebook")?;
    app.send(b"\x1b[B")?;
    app.wait("ID: idea")?;
    app.send(b"\x1b[1;3B\x02")?; // move branch down, bookmark
    app.send(b"\x1bOQ")?; // F2 edits the selected item, not the JSON source
    app.wait("Outline item text")?;
    app.send("\x15\x1b[200~考えたこと\x1b[201~\r".as_bytes())?;
    app.wait("考えたこと")?;
    app.command("Outline reference")?;
    app.wait("Reference target ID")?;
    app.send(b"tasks\r\x13")?;
    app.wait("Saved")?;
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let nodes = value["nodes"].as_array().unwrap();
    ensure!(
        nodes
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect::<Vec<_>>()
            == ["root", "tasks", "idea", "detail"],
        "Branch order differs: {}; screen: {}",
        value,
        app.screen.screen_text()
    );
    ensure!(
        nodes[2]["text"] == "考えたこと"
            && nodes[2]["bookmark"] == true
            && nodes[2]["refs"][0] == "tasks"
            && nodes[2]["colorHint"] == "blue"
            && value["extensions"]["example"] == true,
        "JSON metadata differs: {value}; screen: {}",
        app.screen.screen_text()
    );
    app.send(b"\x1a\x19")?; // undo and redo reference edit
    app.command("Outline export")?;
    app.wait("New Markdown export path")?;
    app.send(b"notes.md\r")?;
    app.wait("Outline Markdown exported")?;
    let markdown = std::fs::read_to_string(dir.path().join("notes.md"))?;
    ensure!(
        markdown.contains("## 考えたこと") && markdown.contains("### Detail"),
        "Markdown differs: {markdown}; screen: {}",
        app.screen.screen_text()
    );
    app.command("Outline source")?;
    app.wait_for("outline source editor", |s| {
        s.screen_text().contains("┌Editor")
    })?;
    app.command("Outline editor")?;
    app.wait("Outline ready")?;
    app.send(b"\x13")?;
    app.wait("Saved")?;
    app.finish()?;
    let mut reopened = Harness::new(&path)?;
    reopened.wait("Outline ready")?;
    reopened.wait("考えたこと")?;
    reopened.finish()
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
        Self::with_terminal(path, exe, "xterm-256color", Some("truecolor"))
    }
    /// Starts exe under TERM and COLORTERM (None removes an inherited COLORTERM).
    fn with_terminal(
        path: &std::path::Path,
        exe: &str,
        term: &str,
        colorterm: Option<&str>,
    ) -> Result<Self> {
        let pair = portable_pty::native_pty_system().openpty(PtySize {
            cols: 100,
            rows: 30,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(exe);
        command.arg(path);
        command.cwd(path.parent().unwrap());
        command.env("TERM", term);
        match colorterm {
            Some(value) => command.env("COLORTERM", value),
            None => command.env_remove("COLORTERM"),
        }
        command.env("SHELL", "/bin/sh");
        command.env(
            "YYTE_CONFIG_DIR",
            path.parent().unwrap().join(".yyte-test-config"),
        );
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
    fn send_bounded(&mut self, mut bytes: &[u8]) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !bytes.is_empty() {
            let mut fd = libc::pollfd {
                fd: self._master.as_raw_fd().context("PTY has no descriptor")?,
                events: libc::POLLOUT,
                revents: 0,
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(
                !remaining.is_zero(),
                "Paste send timed out: {}",
                self.screen.screen_text()
            );
            // This descriptor belongs to the master held by this Harness.
            let ready = unsafe { libc::poll(&mut fd, 1, remaining.as_millis().min(100) as i32) };
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error.into());
            }
            if ready > 0 {
                ensure!(
                    fd.revents & libc::POLLOUT != 0,
                    "PTY closed during paste: {}",
                    self.screen.screen_text()
                );
                // Stay below the PTY's writable low-water mark so a single
                // blocking write cannot fill its input queue after poll.
                let n = self.writer.write(&bytes[..bytes.len().min(64)])?;
                ensure!(n > 0, "PTY accepted no paste bytes");
                bytes = &bytes[n..];
            }
        }
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
    fn click_text(&mut self, label: &str) -> Result<()> {
        self.click_button(label, 0)
    }
    fn click_button(&mut self, label: &str, button: u8) -> Result<()> {
        for row in 0..self.screen.rows() {
            let text = self.screen.screen_row(row).text();
            if let Some(at) = text.find(label) {
                let col = unicode_width::UnicodeWidthStr::width(&text[..at]);
                self.send(
                    format!(
                        "\x1b[<{button};{};{}M\x1b[<{button};{};{}m",
                        col + 1,
                        row + 1,
                        col + 1,
                        row + 1
                    )
                    .as_bytes(),
                )?;
                return Ok(());
            }
        }
        anyhow::bail!(
            "Mouse target {label:?} missing: {}",
            self.screen.screen_text()
        )
    }
    fn command(&mut self, name: &str) -> Result<()> {
        self.send(b"\x1bOP")?;
        struct MenuView {
            left: usize,
            right: usize,
            bottom: usize,
            items: Vec<(usize, String)>,
        }
        let menu = |screen: &yy_term::Terminal| -> Option<MenuView> {
            for y in 0..screen.rows() {
                let text = screen.screen_row(y).text();
                if let Some(at) = text.find("Commands: arrows") {
                    let x = unicode_width::UnicodeWidthStr::width(&text[..at]).checked_sub(1)?;
                    let row = screen.screen_row(y);
                    let right = row
                        .cells
                        .iter()
                        .enumerate()
                        .skip(x + 1)
                        .find(|(_, c)| c.text() == "┐")?
                        .0;
                    let bottom = (y + 1..screen.rows()).find(|r| {
                        let row = screen.screen_row(*r);
                        row.cells.get(x).is_some_and(|c| c.text() == "└")
                            && row.cells.get(right).is_some_and(|c| c.text() == "┘")
                    })?;
                    let items = (y + 1..bottom)
                        .map(|r| {
                            let row = screen.screen_row(r);
                            (
                                r,
                                row.cells
                                    .iter()
                                    .skip(x + 1)
                                    .take(right - x - 1)
                                    .map(|c| c.text())
                                    .collect::<String>()
                                    .trim()
                                    .to_owned(),
                            )
                        })
                        .collect();
                    return Some(MenuView {
                        left: x,
                        right,
                        bottom,
                        items,
                    });
                }
            }
            None
        };
        self.wait_for("complete command menu", |s| menu(s).is_some())?;
        let commands = yyterminal_editor::commands::COMMANDS;
        ensure!(commands.contains(&name), "Unknown command: {name}");
        let name_label = yyterminal_editor::commands::label(
            name,
            yyterminal_editor::commands::Language::English,
        );
        let mut selected = 0usize;
        for _ in 0..commands.len() {
            let MenuView {
                left: x,
                right,
                bottom,
                items,
            } = menu(&self.screen).context("Command menu disappeared")?;
            if let Some((row, _)) = items.iter().find(|(_, label)| label == name_label) {
                self.send(
                    format!(
                        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                        x + 2,
                        row + 1,
                        x + 2,
                        row + 1
                    )
                    .as_bytes(),
                )?;
                return Ok(());
            }
            selected = (selected + 3).min(commands.len() - 1);
            let expected = yyterminal_editor::commands::label(
                commands[selected],
                yyterminal_editor::commands::Language::English,
            );
            self.send(
                format!("\x1b[<65;{};{}M", (x + right) / 2 + 1, bottom.max(2) / 2).as_bytes(),
            )?;
            self.wait_for("command wheel selection applied", |s| {
                menu(s).is_some_and(|view| {
                    view.items.iter().any(|(row, label)| {
                        label == expected
                            && s.screen_row(*row)
                                .cells
                                .iter()
                                .skip(view.left + 1)
                                .take(view.right - view.left - 1)
                                .any(|c| c.attr.bg == yy_term::Color::Indexed(4))
                    })
                })
            })?;
        }
        anyhow::bail!(
            "Menu command missing: {name}; {}",
            self.screen.screen_text()
        )
    }
    fn finish(&mut self) -> Result<()> {
        self.send(b"\x11")?;
        self.wait_exit()
    }
    fn wait_exit(&mut self) -> Result<()> {
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
    let frame = (0..app.screen.rows())
        .map(|row| app.screen.screen_row(row).text())
        .find(|row| row.contains("┌Markdown preview"))
        .context("Preview frame missing")?;
    let at = frame.find("┌Markdown preview").unwrap();
    let left = unicode_width::UnicodeWidthStr::width(&frame[..at]);
    let width = app
        .screen
        .screen_row(0)
        .cells
        .len()
        .saturating_sub(left + 2);
    let rendered = yyterminal_editor::preview::render(
        &std::fs::read_to_string(&path)?,
        &yyterminal_editor::preview::Options::new(path.parent().unwrap().to_owned(), width as u16),
    );
    let target = rendered
        .lines
        .iter()
        .position(|line| line.to_string().contains("Connections:"))
        .context("Demo diagram has no connections")?;
    // Decoration and image height determine the row; a fixed page count does not.
    app.send("\x1b[B".repeat(target).as_bytes())?;
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

#[test]
fn interactive_mouse_menu_folder_and_file_navigation() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let child = dir.path().join("mouse-folder");
    std::fs::create_dir(&child)?;
    let path = dir.path().join("start.txt");
    std::fs::write(&path, b"initial file\n")?;
    std::fs::write(child.join("mouse-note.txt"), b"opened by mouse\n")?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.click_text("File")?;
    app.wait("New file")?;
    app.click_text("New file")?;
    app.wait_for("new file selected", |s| {
        let t = s.screen_text();
        t.contains("[Untitled]") && !t.contains("Save as")
    })?;
    app.click_text("mouse-folder")?;
    app.wait("mouse-note.txt")?;
    app.click_text("mouse-note.txt")?;
    app.wait("opened by mouse")?;
    app.click_text("[..]")?;
    app.wait("mouse-folder")?;
    app.finish()
}

#[test]
fn mouse_terminal_tabs_and_live_split_preview() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("note.md");
    std::fs::write(&path, "# Sample\n\nsource paragraph\n")?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.command("New terminal")?;
    app.wait("Terminal 2")?;
    app.send(b"printf '\\102\\101\\103\\113\\107\\122\\117\\125\\116\\104\\n'\r")?;
    app.click_text("[note.md]")?;
    app.wait("# Sample")?;
    app.click_text("Terminal 2")?;
    app.wait("BACKGROUND")?;
    let (tab_row, row) = (0..app.screen.rows())
        .find_map(|y| {
            let row = app.screen.screen_row(y).text();
            row.contains("[Terminal 2]").then_some((y, row))
        })
        .with_context(|| format!("Terminal tab missing: {}", app.screen.screen_text()))?;
    let start = row.find("[Terminal 2]").unwrap();
    let close = start
        + row[start..]
            .find(" x ")
            .context("Close tab button missing")?
        + 1;
    let col = unicode_width::UnicodeWidthStr::width(&row[..close]) + 1;
    let y = tab_row + 1;
    app.send(format!("\x1b[<0;{col};{y}M\x1b[<0;{col};{y}m").as_bytes())?;
    app.wait_for("terminal tab closed", |s| {
        !s.screen_text().contains("Terminal 2") && s.screen_text().contains("# Sample")
    })?;
    app.command("Split Markdown preview")?;
    app.wait_for("split preview ready", |s| {
        let t = s.screen_text();
        t.matches("Sample").count() >= 2 && !t.contains("Rendering Markdown")
    })?;
    app.click_text("# Sample")?;
    app.send(b"LIVE ")?;
    app.wait_for("live unsaved preview", |s| {
        let t = s.screen_text();
        t.matches("LIVE").count() >= 2 && !t.contains("Rendering Markdown")
    })?;
    ensure!(
        !std::fs::read_to_string(&path)?.starts_with("LIVE"),
        "Preview wrote unsaved edits"
    );
    app.command("Save")?;
    app.wait("Saved")?;
    ensure!(
        std::fs::read_to_string(&path)?.starts_with("LIVE"),
        "Mouse edit was not saved"
    );
    app.finish()
}
#[test]
fn mouse_confirms_save_as_and_viewer_buttons_work() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("input.md");
    std::fs::write(&path, b"# Original\n")?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.command("Save as")?;
    app.wait("Save as path")?;
    app.send(b"copy.md")?;
    app.wait("copy.md")?;
    app.click_text("[OK]")?;
    app.wait_for("mouse save confirmed", |s| {
        let t = s.screen_text();
        t.contains("Saved") && t.contains("[copy.md]") && !t.contains("[OK]")
    })?;
    ensure!(
        std::fs::read(dir.path().join("copy.md"))? == b"# Original\n",
        "Saved bytes changed"
    );
    app.finish()?;
    let viewer_path = dir.path().join("viewer.md");
    std::fs::write(
        &viewer_path,
        "# Original\n\n".to_owned() + &"scrollable content\n".repeat(80),
    )?;
    let mut viewer = Harness::with_exe(&viewer_path, env!("CARGO_BIN_EXE_markdownViewer"))?;
    viewer.wait("Ready")?;
    viewer.click_text("[Down]")?;
    viewer.wait_for("mouse scrolled viewer", |s| {
        !s.screen_text().contains("Original")
    })?;
    viewer.click_text("[Top]")?;
    viewer.wait("Original")?;
    viewer.click_text("[Close]")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = viewer.child.try_wait()? {
            ensure!(status.success(), "Viewer mouse close failed");
            break;
        }
        ensure!(Instant::now() < deadline, "Viewer did not close");
        if let Ok(bytes) = viewer
            .rx
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_millis(100))
        {
            viewer.screen.feed(&bytes);
        }
    }
    Ok(())
}

#[test]
fn categorized_menus_tree_wrapping_open_modes_and_language_persist() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("start.txt");
    std::fs::write(&path, b"original document\n")?;
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder)?;
    // The final line is deliberately a distinct clickable continuation.
    let filename = format!("{}target.csv", "x".repeat(44));
    std::fs::write(folder.join(&filename), b"id,name\n1,Alice\n")?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.click_text("View")?;
    app.wait("Folder tree")?;
    app.click_text("Folder tree")?;
    app.wait_for("tree entries refreshed after preferences created", |s| {
        let text = s.screen_text();
        text.contains("Explorer / Tree")
            && text.contains("Explorer loaded")
            && text.contains(".yyte-test-config")
            && !text.contains("[Working]")
    })?;
    app.click_text("folder")?;
    app.wait("target.csv")?;
    ensure!(
        app.screen.screen_text().contains("start.txt"),
        "Root sibling disappeared: {}",
        app.screen.screen_text()
    );
    app.click_button("target.csv", 2)?;
    app.wait("Open as CSV")?;
    app.click_text("Open as CSV")?;
    app.wait("Alice")?;
    app.click_text("Settings")?;
    app.wait("Language: Japanese")?;
    app.click_text("Language: Japanese")?;
    app.wait("ファイル")?;
    app.click_text("設定")?;
    app.wait("English（英語）")?;
    app.send(b"\x1b")?;
    app.wait_for("Japanese settings menu closed", |s| {
        !s.screen_text().contains("English（英語）")
    })?;
    app.finish()?;
    let settings = dir.path().join(".yyte-test-config/ui.conf");
    let text = std::fs::read_to_string(&settings)?;
    ensure!(
        text.contains("language=ja") && text.contains("explorer=tree"),
        "Preferences not saved: {text}"
    );
    let mut reopened = Harness::new(&path)?;
    reopened.wait("ファイル")?;
    reopened.click_text("設定")?;
    reopened.wait("English（英語）")?;
    reopened.click_text("English（英語）")?;
    reopened.wait("Settings")?;
    reopened.finish()
}

#[test]
fn large_preview_end_navigation_and_spreadsheet_table_render_in_both_apps() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("large-preview.md");
    let source = "```text\n".to_owned()
        + &"document content past old byte and line limits\n".repeat(70_000)
        + "```\n\n| Item | Quantity |\n|---|---:|\n| FINAL_TABLE | 123456 |\n";
    std::fs::write(&path, source)?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.send(b"\x1b[15~")?;
    app.wait("Markdown preview ready")?;
    app.send(b"\x1b[F")?;
    app.wait("FINAL_TABLE")?;
    ensure!(
        app.screen.screen_text().contains("123456") && app.screen.screen_text().contains('╞'),
        "Grid missing: {}",
        app.screen.screen_text()
    );
    app.finish()?;
    let mut viewer = Harness::with_exe(&path, env!("CARGO_BIN_EXE_markdownViewer"))?;
    viewer.wait("Ready")?;
    viewer.click_text("[End]")?;
    viewer.wait("FINAL_TABLE")?;
    ensure!(
        viewer.screen.screen_text().contains("123456") && viewer.screen.screen_text().contains('╞'),
        "Standalone grid missing: {}",
        viewer.screen.screen_text()
    );
    viewer.click_text("[Top]")?;
    viewer.wait("document content past")?;
    viewer.click_text("[Close]")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = viewer.child.try_wait()? {
            ensure!(status.success(), "Viewer exit failed");
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Viewer did not exit: {}",
            viewer.screen.screen_text()
        );
        if let Ok(bytes) = viewer
            .rx
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_millis(100))
        {
            viewer.screen.feed(&bytes);
        }
    }
    Ok(())
}

#[test]
fn editor_and_markdown_preview_highlight_languages_and_allow_manual_override() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.rs");
    let original = "fn main() {\n    let value = \"日本語\"; // note\n}\n";
    std::fs::write(&path, original)?;
    let markdown = dir.path().join("syntax.md");
    std::fs::write(
        &markdown,
        format!("# Syntax sample\n\n```rust\n{original}```\n"),
    )?;
    let colored = |screen: &yy_term::Terminal, needle: &str, rgb: (u8, u8, u8)| {
        (0..screen.rows()).any(|y| {
            let row = screen.screen_row(y);
            let text = row.text();
            text.find(needle).is_some_and(|at| {
                let x = unicode_width::UnicodeWidthStr::width(&text[..at]);
                row.cells
                    .get(x)
                    .is_some_and(|c| c.attr.fg == yy_term::Color::Rgb(rgb.0, rgb.1, rgb.2))
            })
        })
    };
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.wait_for("Rust editor keyword highlighting", |s| {
        colored(s, "let value", (198, 120, 221))
    })?;
    app.wait_for("Rust editor comment highlighting", |s| {
        colored(s, "// note", (127, 135, 145))
    })?;
    app.command("Syntax language")?;
    app.wait("Syntax: auto / none")?;
    app.send(b"none\r")?;
    app.wait("Syntax: none")?;
    app.wait_for("highlight disabled", |s| {
        !colored(s, "let value", (198, 120, 221))
    })?;
    app.command("Syntax language")?;
    app.wait("Syntax: auto / none")?;
    app.send(b"auto\r")?;
    app.wait_for("automatic syntax restored", |s| {
        colored(s, "let value", (198, 120, 221))
    })?;
    app.send(b"\x0fsyntax.md\r")?;
    app.wait_for("Markdown source embedded Rust highlighting", |s| {
        s.screen_text().contains("[syntax.md]") && colored(s, "let value", (198, 120, 221))
    })?;
    app.send(b"\x1b[15~")?;
    app.wait("Markdown preview ready")?;
    app.wait_for("preview Rust keyword highlighting", |s| {
        colored(s, "let value", (198, 120, 221))
    })?;
    app.finish()?;
    ensure!(
        std::fs::read_to_string(&path)? == original,
        "Highlighting changed file bytes"
    );
    let mut viewer = Harness::with_exe(&markdown, env!("CARGO_BIN_EXE_markdownViewer"))?;
    viewer.wait("Ready")?;
    viewer.wait_for("standalone Rust keyword highlighting", |s| {
        colored(s, "let value", (198, 120, 221))
    })?;
    viewer.click_text("[Close]")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = viewer.child.try_wait()? {
            ensure!(status.success(), "Viewer exit failed");
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Viewer did not exit: {}",
            viewer.screen.screen_text()
        );
        if let Ok(bytes) = viewer
            .rx
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_millis(100))
        {
            viewer.screen.feed(&bytes);
        }
    }
    Ok(())
}

#[test]
fn markdown_code_stays_readable_without_truecolor() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("code.md");
    std::fs::write(
        &path,
        "Inline `inline_code` here.\n\n```\nplain_block_code\n```\n\n```rust\nlet value = 1;\n```\n",
    )?;
    let cell_at = |screen: &yy_term::Terminal, needle: &str| {
        (0..screen.rows()).find_map(|y| {
            let row = screen.screen_row(y);
            let text = row.text();
            text.find(needle).and_then(|at| {
                let x = unicode_width::UnicodeWidthStr::width(&text[..at]);
                row.cells.get(x).map(|c| (c.attr.fg, c.attr.bg))
            })
        })
    };
    let no_rgb = |screen: &yy_term::Terminal| {
        (0..screen.rows()).all(|y| {
            screen.screen_row(y).cells.iter().all(|c| {
                !matches!(c.attr.fg, yy_term::Color::Rgb(..))
                    && !matches!(c.attr.bg, yy_term::Color::Rgb(..))
            })
        })
    };
    // 256 colors: the code background is palette gray 235 instead of 48;2;30;36;46.
    // 16 colors (Linux console): it is black.
    for (term, background) in [("xterm-256color", 235), ("linux", 0)] {
        let mut viewer =
            Harness::with_terminal(&path, env!("CARGO_BIN_EXE_markdownViewer"), term, None)?;
        viewer.wait("Ready")?;
        for needle in ["inline_code", "plain_block_code", "let value"] {
            viewer.wait_for(&format!("{term} {needle} on palette background"), |s| {
                cell_at(s, needle)
                    .is_some_and(|(fg, bg)| bg == yy_term::Color::Indexed(background) && fg != bg)
                    && no_rgb(s)
            })?;
        }
        viewer.send(b"q")?;
        viewer.wait_exit()?;
    }
    Ok(())
}

#[test]
fn preview_block_spacing_is_visible_in_editor_and_standalone_viewer() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("spacing.md");
    let source = "# READABLE_TITLE\n\nFIRST_PARAGRAPH\n\n- LIST_ONE\n- LIST_TWO\n\nAFTER_LIST\n\nLAST_PARAGRAPH\n";
    std::fs::write(&path, source)?;
    let spaced = |screen: &yy_term::Terminal| {
        let rows = (0..screen.rows())
            .map(|y| screen.screen_row(y).text())
            .collect::<Vec<_>>();
        let find = |text: &str| rows.iter().position(|row| row.contains(text));
        let Some(title) = find("READABLE_TITLE") else {
            return false;
        };
        let at = rows[title].find("READABLE_TITLE").unwrap();
        let left = unicode_width::UnicodeWidthStr::width(&rows[title][..at]);
        // Inspect the preview's columns, not the independent Explorer beside it.
        let blank = |row: usize| {
            if row >= screen.rows() {
                return false;
            }
            screen
                .screen_row(row)
                .cells
                .iter()
                .skip(left)
                .map(|cell| cell.text())
                .collect::<String>()
                .trim_matches(['│', ' '])
                .is_empty()
        };
        let Some(first) = find("FIRST_PARAGRAPH") else {
            return false;
        };
        let Some(one) = find("• LIST_ONE") else {
            return false;
        };
        let Some(two) = find("• LIST_TWO") else {
            return false;
        };
        let Some(after) = find("AFTER_LIST") else {
            return false;
        };
        let Some(last) = find("LAST_PARAGRAPH") else {
            return false;
        };
        title > 0
            && first == title + 3
            && one == first + 2
            && two == one + 1
            && after == two + 2
            && last == after + 2
            && blank(title + 2)
            && rows[title - 1].contains('╔')
            && rows[title + 1].contains('╚')
            && [first, two, after].iter().all(|row| blank(row + 1))
    };
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.send(b"\x1b[15~")?;
    app.wait("Markdown preview ready")?;
    app.wait_for("readable preview block spacing", spaced)?;
    app.finish()?;
    let mut viewer = Harness::with_exe(&path, env!("CARGO_BIN_EXE_markdownViewer"))?;
    viewer.wait("Ready")?;
    viewer.wait_for("standalone block spacing", spaced)?;
    viewer.click_text("[Close]")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = viewer.child.try_wait()? {
            ensure!(status.success(), "Viewer exit failed");
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Viewer did not exit: {}",
            viewer.screen.screen_text()
        );
        if let Ok(bytes) = viewer
            .rx
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_millis(100))
        {
            viewer.screen.feed(&bytes);
        }
    }
    ensure!(
        std::fs::read_to_string(&path)? == source,
        "Preview changed Markdown source"
    );
    Ok(())
}

#[test]
fn database_query_grid_plan_mouse_and_resize_use_real_terminal() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("query.sql");
    std::fs::write(
        &path,
        "SELECT 9223372036854775807 AS exact_integer,'東京' AS city UNION ALL SELECT 42,'大阪';\n",
    )?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.command("DB Connect")?;
    app.wait("Connected: SQLite / memory")?;
    app.command("DB Execute")?;
    app.wait("SQL complete:")?;
    app.wait("9223372036854775807")?;
    app.wait("東京")?;
    app.wait_for("spreadsheet result header and grid", |s| {
        s.screen_text().contains("exact_integer")
            && s.screen_text().contains("2 rows")
            && s.screen_text().contains("Results")
    })?;
    app.send(b"\x1b[C\x03")?;
    app.wait("Result selection copied")?;
    app.command("DB Column width")?;
    app.send(b"20\r")?;
    app.command("DB Estimated plan")?;
    app.wait("Execution plan ready")?;
    app.wait("Operator properties")?;
    app.command("DB Open plan")?;
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("demo/database/sqlserver-hash-sort.sqlplan");
    app.send(format!("\x1b[200~{}\x1b[201~\r", fixture.display()).as_bytes())?;
    app.wait_for("SQL Server plan", |s| {
        s.screen_text().contains("SQL Server")
            && s.screen_text().contains("Actual")
            && s.screen_text().contains("Operator properties")
    })?;
    app.command("DB Next operator")?;
    app.wait_for(
        "next plan operator selected and command menu closed",
        |screen| {
            let text = screen.screen_text();
            text.contains("表示対象: Sort") && !text.contains("Commands: arrows")
        },
    )?;
    app._master.resize(PtySize {
        cols: 60,
        rows: 18,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    app.screen.resize(60, 18);
    app.wait_for("resized database frame", |s| {
        s.screen_row(16).text().contains('┘') && s.screen_text().contains("Database")
    })?;
    app.command("DB Query")?;
    app.wait("SQL editor")?;
    app.finish()
}

#[test]
fn database_management_menus_are_local_in_real_terminal() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("empty.txt");
    std::fs::write(&path, "")?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.click_text("View")?;
    app.wait("Database Management")?;
    app.click_text("Database Management")?;
    app.wait("SQL editor")?;
    app.wait_for("database categories inside management frame", |screen| {
        !screen.screen_row(0).text().contains("Database")
            && (1..screen.rows()).any(|y| {
                let text = screen.screen_row(y).text();
                text.contains("Database") && text.contains("SQL") && text.contains("Results")
            })
    })?;
    app.send(b"\x1b[21~")?;
    app.wait("DB New connection")?;
    app.send(b"\x1b[C")?;
    app.wait("DB Commit")?;
    app.send(b"\x1b[D")?;
    app.wait("DB New connection")?;
    app.send(b"\x1b")?;
    app.wait_for("database menu closed", |screen| {
        !screen.screen_text().contains("DB New connection")
    })?;
    app.click_text("SQL  Results")?;
    app.wait("DB Commit")?;
    app.click_text("DB Query")?;
    app.wait("SQL editor")?;
    app.finish()
}

#[test]
fn sqlite_join_plan_connects_in_real_terminal() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("join.sql");
    std::fs::write(
        &path,
        "CREATE TABLE a(id INTEGER PRIMARY KEY); CREATE TABLE b(id INTEGER PRIMARY KEY,a_id INTEGER); INSERT INTO a VALUES(1); INSERT INTO b VALUES(1,1); SELECT a.id,b.id FROM a JOIN b ON b.a_id=a.id;",
    )?;
    let mut app = Harness::new(&path)?;
    app.wait("Opened")?;
    app.command("DB Connect")?;
    app.wait("Connected: SQLite / memory")?;
    app.command("DB Execute")?;
    app.wait("SQL complete:")?;
    app.command("DB Query")?;
    app.send(b"\x01\x1b[200~SELECT a.id,b.id FROM a JOIN b ON b.a_id=a.id;\x1b[201~")?;
    app.command("DB Estimated plan")?;
    app.wait("Execution plan ready")?;
    app.wait_for(
        "connected SQLite join and explicit display group",
        |screen| {
            let text = screen.screen_text();
            text.contains("SQLite query (display group)") && text.contains('◀')
        },
    )?;
    app._master.resize(PtySize {
        cols: 60,
        rows: 18,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    app.screen.resize(60, 18);
    app.wait_for("SQLite plan after narrow resize", |screen| {
        screen.screen_row(16).text().contains('┘') && screen.screen_text().contains("Plan")
    })?;
    app.command("Save")?;
    app.wait("Saved")?;
    app.finish()
}
