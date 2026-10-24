mod editor;
mod git;
mod shell;
mod ui;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M};
use editor::Editor;
use ratatui::layout::Rect;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};
use ui::draw;
use unicode_width::UnicodeWidthStr;

const HELP: &str = include_str!("../docs/user/help.md");
const COMMANDS: &[&str] = &[
    "Open file",
    "Open encoding",
    "Workspace folders",
    "Save",
    "Save as",
    "New file",
    "Next tab",
    "Close tab",
    "Find text",
    "Explorer",
    "Terminal",
    "Git status",
    "Git diff",
    "Git staged diff",
    "Git stage all",
    "Git unstage all",
    "Git commit",
    "Markdown preview",
    "Help",
    "Quit",
];
#[derive(Clone, Copy, PartialEq)]
enum View {
    Editor,
    Terminal,
    Git,
    Preview,
    Help,
}
enum Message {
    Open(Box<Result<Editor>>),
    Saved(Result<(usize, PathBuf, yy_core::Version, [u8; 32])>),
    Git(Result<String>),
    Workspace(Result<Vec<PathBuf>>),
    Entries(Result<(PathBuf, Vec<PathBuf>)>),
}
#[derive(Clone, Copy)]
enum Prompt {
    Open,
    Save,
    Find,
    Commit,
    Encoding,
    Quit,
}
struct App {
    root: PathBuf,
    roots: Vec<PathBuf>,
    encoding: Option<yy_core::Encoding>,
    entries: Vec<PathBuf>,
    selected: usize,
    tabs: Vec<Editor>,
    tab: usize,
    view: View,
    explorer: bool,
    shell: Option<shell::Shell>,
    term_back: usize,
    clipboard: String,
    output: String,
    scroll: u16,
    status: String,
    busy: bool,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    prompt: Option<Prompt>,
    input: String,
    palette: Option<usize>,
    quit: bool,
    rows: Vec<(u64, u64, String)>,
    editor_area: Rect,
}
impl App {
    fn new(root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            roots: vec![root.clone()],
            encoding: None,
            root,
            entries: vec![],
            selected: 0,
            tabs: vec![Editor::empty()],
            tab: 0,
            view: View::Editor,
            explorer: false,
            shell: None,
            term_back: 0,
            clipboard: String::new(),
            output: String::new(),
            scroll: 0,
            status: "F1: commands / help   F6: explorer   F4: terminal".into(),
            busy: false,
            tx,
            rx,
            prompt: None,
            input: String::new(),
            palette: None,
            quit: false,
            rows: vec![],
            editor_area: Rect::default(),
        };
        app.read_dir(app.root.clone());
        app
    }
    fn read_dir(&mut self, path: PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<_> {
                let (entries, skipped) = yy_config::workspace::list_dir(&path)?;
                anyhow::ensure!(
                    skipped == 0,
                    "Directory exceeds the 10,000 entry display limit; open a narrower path"
                );
                let mut paths = entries.into_iter().map(|e| e.path).collect::<Vec<_>>();
                if let Some(parent) = path.parent() {
                    paths.insert(0, parent.to_owned());
                }
                Ok((path, paths))
            })();
            let _ = tx.send(Message::Entries(result));
        });
    }
    fn open(&mut self, path: PathBuf) {
        if self.busy {
            return;
        }
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(extension, "yys" | "yyd" | "yy3d" | "yyplan") {
            self.status = "This YY binary format is not editable in the initial preview".into();
            return;
        }
        self.busy = true;
        self.status = "Opening private snapshot...".into();
        let tx = self.tx.clone();
        let encoding = self.encoding;
        std::thread::spawn(move || {
            if path.extension().is_some_and(|e| e == "yyworkspace") {
                let result = yy_config::workspace::Workspace::load(&path)
                    .map(|ws| ws.folders)
                    .map_err(anyhow::Error::msg);
                let _ = tx.send(Message::Workspace(result));
            } else {
                let _ = tx.send(Message::Open(Box::new(Editor::open(&path, encoding))));
            }
        });
    }
    fn save(&mut self, path: PathBuf) {
        if self.busy {
            return;
        }
        let e = &self.tabs[self.tab];
        let path = if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        };
        let normalized = if path.exists() {
            path.canonicalize()
        } else {
            path.parent()
                .unwrap_or(&self.root)
                .canonicalize()
                .map(|parent| parent.join(path.file_name().unwrap_or_default()))
        };
        let path = match normalized {
            Ok(path) => path,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        let same = e.path.as_ref() == Some(&path);
        if !same && path.exists() {
            self.status = "Save as refuses to overwrite another file".into();
            return;
        }
        let expected = if same { e.digest } else { None };
        let snapshot = e.doc.snapshot().clone();
        let format = e.doc.save_format(e.doc.encoding(), e.doc.has_bom());
        let version = e.doc.version();
        let tab = self.tab;
        let tx = self.tx.clone();
        self.busy = true;
        self.status = "Saving...".into();
        std::thread::spawn(move || {
            let result = (|| -> Result<_> {
                if let Some(expected) = expected {
                    anyhow::ensure!(
                        editor::digest(&path)? == expected,
                        "File changed externally; save to another path"
                    );
                }
                if expected.is_some() {
                    yy_io::save_snapshot(&snapshot, &path, &format, None)?;
                } else {
                    let staged = tempfile::NamedTempFile::new_in(
                        path.parent().unwrap_or(std::path::Path::new(".")),
                    )?
                    .into_temp_path();
                    yy_io::save_snapshot(&snapshot, &staged, &format, None)?;
                    staged
                        .persist_noclobber(&path)
                        .map_err(|error| error.error)?;
                }
                let hash = editor::digest(&path)?;
                Ok((tab, path, version, hash))
            })();
            let _ = tx.send(Message::Saved(result));
        });
    }
    fn git(&mut self, args: Vec<String>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.view = View::Git;
        self.explorer = false;
        self.scroll = 0;
        self.status = "Running Git...".into();
        let tx = self.tx.clone();
        let root = self.root.clone();
        std::thread::spawn(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let _ = tx.send(Message::Git(git::run(&root, &args)));
        });
    }
    fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(message) = self.rx.try_recv() {
            changed = true;
            self.busy = false;
            let result: Result<()> = match message {
                Message::Open(result) => (*result).map(|e| {
                    if self.tabs.len() == 1
                        && self.tabs[0].path.is_none()
                        && !self.tabs[0].modified()
                    {
                        self.tabs[0] = e;
                        self.tab = 0;
                    } else {
                        self.tabs.push(e);
                        self.tab = self.tabs.len() - 1;
                    }
                    self.view = View::Editor;
                    self.explorer = false;
                    self.status = "Opened".into();
                }),
                Message::Saved(result) => result.map(|(tab, path, version, hash)| {
                    let e = &mut self.tabs[tab];
                    e.path = Some(path);
                    e.saved = version;
                    e.digest = Some(hash);
                    self.status = "Saved".into();
                }),
                Message::Workspace(result) => result.map(|roots| {
                    self.roots=roots;
                    self.entries=self.roots.clone();self.selected=0;self.explorer=true;
                    self.status="Workspace roots loaded. SSH and foreign OS paths are preserved but unavailable locally".into();
                }),
                Message::Git(result) => result.map(|text| {
                    self.output = text;
                    self.status = "Git completed".into();
                }),
                Message::Entries(result) => result.map(|(path, entries)| {
                    self.root = path;
                    self.entries = entries;
                    self.selected = 0;
                    self.status = "Explorer loaded".into();
                }),
            };
            if let Err(e) = result {
                self.status = format!("{e:#}");
            }
        }
        if let Some(shell) = &mut self.shell {
            if let Err(error) = shell.poll() {
                self.status = error.to_string();
                changed = true;
            }
            changed |= shell.screen.take_dirty();
        }
        changed
    }
    fn ask(&mut self, prompt: Prompt) {
        self.prompt = Some(prompt);
        self.input.clear();
    }
    fn command(&mut self, name: &str) {
        match name {
            "Open file" => self.ask(Prompt::Open),
            "Open encoding" => self.ask(Prompt::Encoding),
            "Workspace folders" => {
                self.entries = self.roots.clone();
                self.selected = 0;
                self.explorer = true;
            }
            "Save" => {
                if let Some(p) = self.tabs[self.tab].path.clone() {
                    self.save(p)
                } else {
                    self.ask(Prompt::Save)
                }
            }
            "Save as" => self.ask(Prompt::Save),
            "New file" => {
                self.tabs.push(Editor::empty());
                self.tab = self.tabs.len() - 1;
                self.view = View::Editor;
                self.explorer = false;
            }
            "Next tab" => {
                self.tab = (self.tab + 1) % self.tabs.len();
                self.view = View::Editor;
            }
            "Close tab" => {
                if self.busy {
                    self.status = "Wait for the active operation before closing tabs".into();
                    return;
                }
                if self.tabs[self.tab].modified() {
                    self.status = "Save this document before closing its tab".into();
                } else {
                    self.tabs.remove(self.tab);
                    if self.tabs.is_empty() {
                        self.tabs.push(Editor::empty());
                    }
                    self.tab = self.tab.min(self.tabs.len() - 1);
                }
            }
            "Find text" => self.ask(Prompt::Find),
            "Explorer" => {
                self.explorer = true;
            }
            "Terminal" => {
                if self.shell.is_none() {
                    match shell::Shell::new(&self.root) {
                        Ok(s) => self.shell = Some(s),
                        Err(e) => {
                            self.status = e.to_string();
                            return;
                        }
                    }
                }
                self.view = View::Terminal;
                self.explorer = false;
            }
            "Git status" => self.git(vec!["status".into(), "--short".into(), "--branch".into()]),
            "Git diff" => self.git(vec![
                "diff".into(),
                "--no-ext-diff".into(),
                "--no-color".into(),
            ]),
            "Git staged diff" => self.git(vec![
                "diff".into(),
                "--cached".into(),
                "--no-ext-diff".into(),
                "--no-color".into(),
            ]),
            "Git stage all" => self.git(vec!["add".into(), "--all".into()]),
            "Git unstage all" => self.git(vec!["reset".into(), "--mixed".into(), "HEAD".into()]),
            "Git commit" => self.ask(Prompt::Commit),
            "Markdown preview" => {
                self.view = View::Preview;
                self.explorer = false;
                self.scroll = 0;
            }
            "Help" => {
                self.view = View::Help;
                self.explorer = false;
                self.scroll = 0;
            }
            "Quit" => {
                if self.busy {
                    self.status = "Wait for the active operation before quitting".into();
                } else if self.tabs.iter().any(Editor::modified) {
                    self.ask(Prompt::Quit);
                } else {
                    self.quit = true;
                }
            }
            _ => {}
        }
    }
    fn submit(&mut self, prompt: Prompt) {
        let input = std::mem::take(&mut self.input);
        match prompt {
            Prompt::Open => {
                let p = PathBuf::from(input);
                self.open(if p.is_absolute() {
                    p
                } else {
                    self.root.join(p)
                });
            }
            Prompt::Save => {
                if !input.is_empty() {
                    self.save(PathBuf::from(input));
                }
            }
            Prompt::Encoding => {
                if input.eq_ignore_ascii_case("auto") {
                    self.encoding = None;
                    self.status = "Next file: automatic encoding".into();
                } else if let Some(enc) = yy_core::Encoding::from_name(&input) {
                    self.encoding = Some(enc);
                    self.status = format!("Next file: {}", enc.name());
                } else {
                    self.status = "Unknown encoding; use yyte --list-encodings".into();
                }
            }
            Prompt::Commit => {
                if !input.trim().is_empty() {
                    self.git(vec!["commit".into(), "-m".into(), input]);
                }
            }
            Prompt::Quit => {
                self.quit = input == "discard";
            }
            Prompt::Find => {
                if input.is_empty() {
                    return;
                }
                let e = &mut self.tabs[self.tab];
                // Searches bounded chunks on a worker in a later phase; currently cap to 16 MiB.
                let start = e.caret().saturating_add(1).min(e.doc.snapshot().len());
                let end = (start + 16 * 1024 * 1024).min(e.doc.snapshot().len());
                if let Some(at) = e.doc.snapshot().find_bytes(start..end, input.as_bytes()) {
                    e.move_to(at, false);
                    e.move_to(at + input.len() as u64, true);
                    e.top = at;
                    self.status = "Match selected".into();
                } else {
                    self.status = "No match in next 16 MiB".into();
                }
            }
        }
    }
    fn key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if let Some(prompt) = self.prompt {
            match key.code {
                K::Esc => {
                    self.prompt = None;
                }
                K::Enter => {
                    self.prompt = None;
                    self.submit(prompt);
                }
                K::Backspace => {
                    self.input.pop();
                }
                K::Char(c) if !key.modifiers.contains(M::CONTROL) => self.input.push(c),
                _ => {}
            }
            return;
        }
        if let Some(selected) = self.palette {
            match key.code {
                K::Esc => self.palette = None,
                K::Down => self.palette = Some((selected + 1) % COMMANDS.len()),
                K::Up => self.palette = Some((selected + COMMANDS.len() - 1) % COMMANDS.len()),
                K::Enter => {
                    self.palette = None;
                    self.command(COMMANDS[selected]);
                }
                _ => {}
            }
            return;
        }
        match key.code {
            K::F(1) | K::F(10) => {
                self.palette = Some(0);
                return;
            }
            K::F(6) => {
                self.explorer = !self.explorer;
                return;
            }
            K::F(2) => {
                self.view = View::Editor;
                self.explorer = false;
                return;
            }
            K::F(3) => {
                self.command("Git status");
                return;
            }
            K::F(4) => {
                self.command("Terminal");
                return;
            }
            K::F(5) => {
                self.command("Markdown preview");
                return;
            }
            _ => {}
        }
        // Ctrl+Q remains an application escape even when a shell is focused.
        if key.modifiers.contains(M::CONTROL) && key.code == K::Char('q') {
            self.command("Quit");
            return;
        }
        if self.view == View::Terminal && !self.explorer {
            if let Some(shell) = &mut self.shell {
                if key.modifiers.contains(M::SHIFT) && matches!(key.code, K::PageUp | K::PageDown) {
                    if key.code == K::PageUp {
                        self.term_back =
                            (self.term_back + shell.screen.rows()).min(shell.screen.history_len());
                    } else {
                        self.term_back = self.term_back.saturating_sub(shell.screen.rows());
                    }
                    return;
                }
                self.term_back = 0;
                let k = match key.code {
                    K::Char(c) => yy_term::Key::Char(c),
                    K::Enter => yy_term::Key::Enter,
                    K::Tab => yy_term::Key::Tab,
                    K::Backspace => yy_term::Key::Backspace,
                    K::Esc => yy_term::Key::Escape,
                    K::Up => yy_term::Key::Up,
                    K::Down => yy_term::Key::Down,
                    K::Left => yy_term::Key::Left,
                    K::Right => yy_term::Key::Right,
                    K::Home => yy_term::Key::Home,
                    K::End => yy_term::Key::End,
                    K::Delete => yy_term::Key::Delete,
                    K::PageUp => yy_term::Key::PageUp,
                    K::PageDown => yy_term::Key::PageDown,
                    K::F(n) => yy_term::Key::F(n),
                    _ => return,
                };
                let bytes = yy_term::keys::encode(
                    k,
                    yy_term::Mods {
                        ctrl: key.modifiers.contains(M::CONTROL),
                        alt: key.modifiers.contains(M::ALT),
                        shift: key.modifiers.contains(M::SHIFT),
                    },
                    shell.screen.modes(),
                );
                if let Err(e) = shell.send(&bytes) {
                    self.status = e.to_string();
                }
            }
            return;
        }
        if key.modifiers.contains(M::CONTROL) {
            let command = match key.code {
                K::Char('s') => Some("Save"),
                K::Char('o') | K::Char('p') => Some("Open file"),
                K::Char('n') => Some("New file"),
                K::Char('w') => Some("Close tab"),
                K::Char('f') => Some("Find text"),
                K::Tab => Some("Next tab"),
                _ => None,
            };
            if let Some(c) = command {
                self.command(c);
                return;
            }
        }
        if self.explorer {
            match key.code {
                K::Up => self.selected = self.selected.saturating_sub(1),
                K::Down => {
                    self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1))
                }
                K::Enter => {
                    if let Some(path) = self.entries.get(self.selected).cloned() {
                        if path.is_dir() {
                            self.read_dir(path)
                        } else {
                            self.open(path)
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if self.view != View::Editor {
            match key.code {
                K::Up => self.scroll = self.scroll.saturating_sub(1),
                K::Down => self.scroll = self.scroll.saturating_add(1),
                K::PageUp => self.scroll = self.scroll.saturating_sub(10),
                K::PageDown => self.scroll = self.scroll.saturating_add(10),
                _ => {}
            }
            return;
        }
        if key.modifiers.contains(M::CONTROL) && matches!(key.code, K::Char('c' | 'x' | 'v')) {
            let e = &mut self.tabs[self.tab];
            match key.code {
                K::Char('v') => {
                    e.doc.paste(&self.clipboard);
                }
                _ => match e.doc.selected_text(1024 * 1024) {
                    Ok(Some(text)) => {
                        self.clipboard = text;
                        if key.code == K::Char('x') {
                            e.doc.delete_forward();
                        }
                    }
                    Ok(None) => {}
                    Err(_) => self.status = "Internal clipboard limit: 1 MiB".into(),
                },
            }
            return;
        }
        let e = &mut self.tabs[self.tab];
        let shift = key.modifiers.contains(M::SHIFT);
        let ctrl = key.modifiers.contains(M::CONTROL);
        let at = e.caret();
        let snap = e.doc.snapshot();
        let to = match key.code {
            K::Left => Some(yy_core::motion::prev_grapheme(snap, at)),
            K::Right => Some(yy_core::motion::next_grapheme(snap, at)),
            K::Home if ctrl => Some(0),
            K::End if ctrl => Some(snap.len()),
            K::Home => self
                .rows
                .iter()
                .find(|r| at >= r.0 && at < r.1)
                .map(|r| r.0),
            K::End => self
                .rows
                .iter()
                .find(|r| at >= r.0 && at < r.1)
                .map(|r| yy_core::motion::prev_grapheme(snap, r.1)),
            K::Up | K::Down | K::PageUp | K::PageDown => {
                let row = self.rows.iter().rposition(|r| at >= r.0).unwrap_or(0);
                let down = matches!(key.code, K::Down | K::PageDown);
                let page = matches!(key.code, K::PageUp | K::PageDown);
                let amount = if page {
                    self.editor_area.height.max(1) as usize
                } else {
                    1
                };
                let target = if down {
                    row.saturating_add(amount)
                } else {
                    row.saturating_sub(amount)
                };
                if !down && row < amount {
                    // Walk at most one screen, including long unbroken lines.
                    let mut p = self.rows.first().map_or(at, |r| r.0);
                    let mut cells = 0;
                    while p > 0 && cells < self.editor_area.width.max(1) as usize * amount {
                        p = yy_core::motion::prev_grapheme(snap, p);
                        cells += 1;
                        if snap.byte_at(p) == Some(b'\n') && cells > 1 && !page {
                            p += 1;
                            break;
                        }
                    }
                    e.top = p;
                    Some(p)
                } else if let Some(r) = self.rows.get(target) {
                    let mut p = r.0;
                    let prefix = snap.read(self.rows[row].0..at);
                    let prefix = String::from_utf8_lossy(&prefix)
                        .replace('\t', "    ")
                        .replace('\r', "");
                    let count = UnicodeWidthStr::width(prefix.as_str());
                    let mut column = 0;
                    while p < r.1 && column < count {
                        let next = yy_core::motion::next_grapheme(snap, p);
                        if snap.byte_at(p) == Some(b'\n') {
                            break;
                        }
                        let bytes = snap.read(p..next);
                        let text = String::from_utf8_lossy(&bytes);
                        column += if text == "\t" {
                            4
                        } else {
                            UnicodeWidthStr::width(text.as_ref())
                        };
                        p = next;
                    }
                    Some(p)
                } else {
                    self.rows.last().map(|r| r.1)
                }
            }
            _ => None,
        };
        if let Some(to) = to {
            e.move_to(to, shift);
            return;
        }
        match key.code {
            K::Char('a') if ctrl => e.doc.select_all(),
            K::Char('z') if ctrl => {
                e.doc.undo();
            }
            K::Char('y') if ctrl => {
                e.doc.redo();
            }
            K::Char(c) if !ctrl && !key.modifiers.contains(M::ALT) => {
                e.doc.insert_text(&c.to_string(), false);
            }
            K::Enter => {
                e.doc.insert_newline(false);
            }
            K::Tab => {
                e.doc.insert_text("    ", false);
            }
            K::Backspace => {
                e.doc.delete_backward();
            }
            K::Delete => {
                e.doc.delete_forward();
            }
            _ => {}
        }
    }
}

fn run(mut app: App, initial: Option<PathBuf>) -> Result<()> {
    let mut terminal = ratatui::try_init()?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = crossterm::execute!(std::io::stdout(), event::DisableBracketedPaste);
            ratatui::restore();
        }
    }
    let _restore = Restore;
    crossterm::execute!(std::io::stdout(), event::EnableBracketedPaste)?;
    // Initial explorer loading completes before opening the command-line file.
    let mut initial = initial;
    let mut dirty = true;
    while !app.quit {
        dirty |= app.poll();
        if !app.busy
            && let Some(path) = initial.take()
        {
            app.open(path);
            dirty = true;
        }
        if dirty {
            terminal.draw(|f| draw(f, &mut app))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(30))? {
            dirty = true;
            match event::read()? {
                Event::Key(key) => app.key(key),
                Event::Paste(text) => {
                    if app.palette.is_some() {
                        // Pasting into a command menu must not edit the document beneath it.
                    } else if app.prompt.is_some() {
                        app.input.push_str(&text);
                    } else if app.view == View::Terminal {
                        if let Some(shell) = &mut app.shell {
                            let bytes = yy_term::keys::paste(&text, shell.screen.modes());
                            shell.send(&bytes)?;
                        }
                    } else if app.view == View::Editor && !app.explorer {
                        app.tabs[app.tab].doc.paste(&text);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.iter().any(|a| a == "--list-encodings") {
        for encoding in yy_core::Encoding::all() {
            println!("{}", encoding.name());
        }
        return Ok(());
    }
    let mut encoding = None;
    if let Some(index) = args.iter().position(|a| a == "--encoding") {
        anyhow::ensure!(index + 1 < args.len(), "--encoding requires a name");
        encoding = Some(
            yy_core::Encoding::from_name(&args[index + 1].to_string_lossy())
                .context("Unknown encoding")?,
        );
        args.drain(index..=index + 1);
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "yyte [--encoding NAME] [FILE|DIRECTORY|WORKSPACE.yyworkspace]\nyyte --help | --version\n\n{HELP}"
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--version") {
        println!("yyte {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    anyhow::ensure!(args.len() <= 1, "Expected at most one file or directory");
    let cwd = std::env::current_dir()?;
    let path = args.first().map(PathBuf::from);
    let root = path
        .as_ref()
        .filter(|p| p.is_dir())
        .cloned()
        .unwrap_or(cwd)
        .canonicalize()?;
    let initial = path
        .filter(|p| !p.is_dir())
        .map(|p| p.canonicalize())
        .transpose()
        .context("Cannot open command-line file")?;
    let mut app = App::new(root);
    app.encoding = encoding;
    run(app, initial)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_views_render_at_small_sizes() -> Result<()> {
        for (width, height) in [(1, 1), (20, 6), (80, 24), (160, 48)] {
            let mut app = App::new(std::env::current_dir()?);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
            for view in [View::Editor, View::Git, View::Preview, View::Help] {
                app.view = view;
                terminal.draw(|f| draw(f, &mut app))?;
                app.palette = Some(0);
                terminal.draw(|f| draw(f, &mut app))?;
                app.palette = None;
            }
        }
        Ok(())
    }
    fn wait(app: &mut App) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.busy {
            app.poll();
            assert!(std::time::Instant::now() < deadline, "{}", app.status);
            std::thread::yield_now();
        }
    }
    #[test]
    fn save_refuses_external_change_and_preserves_unsaved_edits() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"original")?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        app.open(path.clone());
        wait(&mut app);
        app.tabs[0].doc.insert_text("edit", false);
        std::fs::write(&path, b"outside")?;
        app.save(path.clone());
        wait(&mut app);
        assert!(app.status.contains("externally"), "{}", app.status);
        assert_eq!(std::fs::read(&path)?, b"outside");
        assert!(app.tabs[0].modified());
        let out = dir.path().join("new.txt");
        app.save(out.clone());
        app.tabs[0].doc.insert_text("later", false);
        wait(&mut app);
        assert_eq!(std::fs::read(&out)?, b"editoriginal");
        assert!(app.tabs[0].modified());
        Ok(())
    }
    #[test]
    fn reads_existing_workspace_without_rewriting_it() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sample.yyworkspace");
        let text = "folders = [\".\", \"ssh://host/home/user\"]\n";
        std::fs::write(&path, text)?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        app.open(path.clone());
        wait(&mut app);
        assert_eq!(app.roots.len(), 2);
        assert!(app.explorer);
        assert_eq!(std::fs::read_to_string(path)?, text);
        Ok(())
    }
    #[test]
    fn selection_copy_paste_and_end_navigation() -> Result<()> {
        let mut app = App::new(std::env::current_dir()?);
        wait(&mut app);
        app.tabs[0].doc.insert_text("日本語", false);
        app.key(KeyEvent::new(K::Char('a'), M::CONTROL));
        app.key(KeyEvent::new(K::Char('c'), M::CONTROL));
        let end = app.tabs[0].doc.snapshot().len();
        app.tabs[0].move_to(end, false);
        app.key(KeyEvent::new(K::Char('v'), M::CONTROL));
        assert_eq!(
            app.tabs[0].doc.snapshot().read(0..18),
            "日本語日本語".as_bytes()
        );
        app.tabs[0].doc.insert_text(&"x".repeat(100_000), false);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24))?;
        terminal.draw(|f| draw(f, &mut app))?;
        assert!(app.tabs[0].top > 90_000);
        Ok(())
    }
    #[test]
    fn standard_editing_and_unsaved_quit() -> Result<()> {
        let mut app = App::new(std::env::current_dir()?);
        app.busy = false;
        app.key(KeyEvent::new(K::Char('a'), M::NONE));
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.tabs[0].modified());
        app.command("Quit");
        assert!(app.prompt.is_some());
        assert!(!app.quit);
        Ok(())
    }
}
