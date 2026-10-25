mod data;
mod diff;
mod editor;
mod fixed_export;
mod git;
mod layouts;
mod shell;
mod ui;
mod workbook;

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
use yyterminal_editor::commands::COMMANDS;
#[derive(Clone, Copy, PartialEq)]
enum View {
    Editor,
    Table,
    Sheet,
    Terminal,
    Git,
    Diff,
    Preview,
    Help,
}
enum Message {
    Preview(yyterminal_editor::preview::Rendered),
    Open(Box<Result<Editor>>),
    Fixed(Box<Result<Editor>>),
    Workbook(Box<Result<Editor>>),
    BookOp {
        tab: usize,
        book: Box<workbook::Book>,
        result: Result<()>,
    },
    Page {
        tab: usize,
        version: yy_core::Version,
        result: Result<data::Page>,
    },
    Saved(Result<(usize, PathBuf, yy_core::Version, [u8; 32])>),
    Git(Result<String>),
    Compared {
        book: Option<(usize, Box<workbook::Book>)>,
        result: Result<diff::Comparison>,
    },
    Notice(Result<String>),
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
    Delimiter,
    Fixed,
    Cell,
    BookCell,
    BookFilter,
    BookFill,
    BookWidth,
    BookHeight,
    FixedExport,
    DiffPath,
    BookFormat,
    BookSheet,
    BookMove,
    BookLayout,
    BookExport,
    LayoutOpen,
    LayoutSave,
    LayoutInfer,
    Quit,
}
#[derive(Clone, Copy)]
enum MouseAction {
    Key(K),
    Run(&'static str),
    Tab(usize),
    CloseTab(usize),
    TabPage(bool),
    Accept,
    Cancel,
    ColumnSize(u32, u16),
    RowSize(u64, u16),
    Entry(usize),
    Command(usize),
}
#[derive(Clone, Copy)]
enum ResizeAxis {
    Column(u32),
    Row(u64),
}
#[derive(Clone, Copy)]
struct ResizeDrag {
    axis: ResizeAxis,
    origin: u16,
    initial: u16,
    value: u16,
}
impl ResizeDrag {
    fn column(self, c: u32, default: u16) -> u16 {
        if matches!(self.axis,ResizeAxis::Column(col) if c==col) {
            self.value
        } else {
            default
        }
    }
    fn row(self, r: u64, default: u16) -> u16 {
        if matches!(self.axis,ResizeAxis::Row(row) if r==row) {
            self.value
        } else {
            default
        }
    }
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

    clipboard: String,
    output: String,
    preview: yyterminal_editor::preview::Rendered,
    preview_pending: bool,
    preview_key: Option<(usize, yy_core::Version, u16, bool)>,
    preview_remote: bool,
    comparison: Option<diff::Comparison>,
    scroll: u16,
    status: String,
    busy: bool,
    pending_saves: std::collections::VecDeque<(usize, PathBuf)>,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    prompt: Option<Prompt>,
    input: String,
    palette: Option<usize>,
    palette_offset: usize,
    tab_offset: usize,
    tab_manual_scroll: bool,
    preview_side: bool,
    preview_focus: bool,
    preview_area: Rect,
    follow_caret: bool,
    resize_drag: Option<ResizeDrag>,
    last_document: usize,
    palette_area: Rect,
    explorer_area: Rect,
    content_area: Rect,
    mouse_actions: Vec<(Rect, MouseAction)>,
    quit: bool,
    rows: Vec<(u64, u64, String)>,
    editor_area: Rect,
    hit_cells: Vec<(Rect, u64, u32)>,
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

            clipboard: String::new(),
            output: String::new(),
            preview: Default::default(),
            preview_pending: false,
            preview_key: None,
            preview_remote: false,
            comparison: None,
            scroll: 0,
            status: "Menu: commands / help | Files: explorer | +Terminal: new shell".into(),
            busy: false,
            pending_saves: std::collections::VecDeque::new(),
            tx,
            rx,
            prompt: None,
            input: String::new(),
            palette: None,
            palette_offset: 0,
            tab_offset: 0,
            tab_manual_scroll: false,
            preview_side: false,
            preview_focus: false,
            preview_area: Rect::default(),
            follow_caret: true,
            resize_drag: None,
            last_document: 0,
            palette_area: Rect::default(),
            explorer_area: Rect::default(),
            content_area: Rect::default(),
            mouse_actions: vec![],
            quit: false,
            rows: vec![],
            editor_area: Rect::default(),
            hit_cells: vec![],
        };
        app.read_dir(app.root.clone());
        app
    }
    fn select_tab(&mut self, tab: usize) {
        if tab >= self.tabs.len() {
            return;
        }
        if self.tab < self.tabs.len() && self.tabs[self.tab].terminal.is_none() {
            self.last_document = self.tab;
        }
        self.tab = tab;
        self.tab_manual_scroll = false;
        self.explorer = false;
        self.preview_focus = false;
        self.follow_caret = true;
        self.preview_key = None;
        self.view = if self.tabs[tab].terminal.is_some() {
            View::Terminal
        } else {
            self.last_document = tab;
            if self.tabs[tab].spreadsheet {
                View::Sheet
            } else if self.tabs[tab].raw || self.tabs[tab].table.is_some() {
                View::Table
            } else {
                View::Editor
            }
        };
    }
    fn new_terminal(&mut self) {
        match shell::Shell::new(&self.root) {
            Ok(shell) => {
                let mut tab = Editor::empty();
                tab.terminal = Some(shell);
                self.tabs.push(tab);
                self.select_tab(self.tabs.len() - 1);
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    fn request_preview(&mut self, width: u16) {
        let width = width.clamp(1, 240);
        let key = (
            self.tab,
            self.tabs[self.tab].doc.version(),
            width,
            self.preview_remote,
        );
        if self.preview_pending || self.preview_key == Some(key) {
            return;
        }
        self.preview_key = Some(key);
        self.preview_pending = true;
        let snapshot = self.tabs[self.tab].doc.snapshot().clone();
        let base = self.tabs[self.tab]
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(&self.root)
            .to_owned();
        let mut opts = yyterminal_editor::preview::Options::new(base, width);
        opts.remote_images = self.preview_remote;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let bytes = snapshot.read(
                0..snapshot
                    .len()
                    .min(yyterminal_editor::preview::MAX_MARKDOWN as u64),
            );
            let text = String::from_utf8_lossy(&bytes);
            let mut rendered = yyterminal_editor::preview::render(&text, &opts);
            if snapshot.len() > bytes.len() as u64 {
                rendered
                    .lines
                    .push(ratatui::text::Line::raw("[Markdown truncated at 1 MiB]"));
            }
            let _ = tx.send(Message::Preview(rendered));
        });
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
        if extension.eq_ignore_ascii_case("yys") {
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<Editor> {
                    let book = workbook::Book::open(&path)?;
                    let mut editor = Editor::empty();
                    editor.path = book.path.clone();
                    editor.digest = book.digest;
                    editor.spreadsheet = true;
                    editor.book = Some(book);
                    Ok(editor)
                })();
                let _ = tx.send(Message::Workbook(Box::new(result)));
            });
            return;
        }
        if matches!(extension, "yyd" | "yy3d" | "yyplan") {
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
            self.pending_saves.retain(|(tab, _)| *tab != self.tab);
            self.pending_saves.push_back((self.tab, path));
            self.status = "Save queued after the active operation".into();
            return;
        }
        if self.tabs[self.tab].spreadsheet {
            let path = if path.is_absolute() {
                path
            } else {
                self.root.join(path)
            };
            self.book_op(workbook::Op::Save(path));
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
    fn export_raw_fixed(&mut self, path: PathBuf) {
        if self.busy {
            return;
        }
        if !self.tabs[self.tab].raw
            || !matches!(
                self.tabs[self.tab].table.as_ref().map(|g| &g.mode),
                Some(data::Mode::Fixed(_))
            )
        {
            self.status = "Apply a fixed-length layout first".into();
            return;
        }
        let snapshot = self.tabs[self.tab].doc.snapshot().clone();
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<String> {
                anyhow::ensure!(!path.exists(), "Export refuses existing paths");
                let file = tempfile::NamedTempFile::new_in(path.parent().context("No parent")?)?
                    .into_temp_path();
                yy_io::save_snapshot(&snapshot, &file, &yy_io::SaveFormat::utf8(), None)?;
                file.persist_noclobber(&path).map_err(|e| e.error)?;
                Ok("Fixed-length export complete (original bytes and layout preserved)".into())
            })();
            let _ = tx.send(Message::Notice(result));
        });
    }
    fn diff_file(&mut self, path: PathBuf) {
        if self.busy {
            return;
        }
        let left = self.tabs[self.tab].doc.snapshot().clone();
        let raw = self.tabs[self.tab].raw;
        let encoding = self.encoding;
        let book = self.tabs[self.tab]
            .book
            .take()
            .map(|b| (self.tab, Box::new(b)));
        self.busy = true;
        self.status = "Comparing with yymerge...".into();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<diff::Comparison> {
                let storage = tempfile::tempdir()?;
                let left = if let Some((_, book)) = &book {
                    let p = storage.path().join("left.csv");
                    book.export(&p)?;
                    Editor::open(&p, Some(yy_core::Encoding::Utf8))?
                        .doc
                        .snapshot()
                        .clone()
                } else {
                    left
                };
                let right = if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("yys"))
                {
                    let b = workbook::Book::open(&path)?;
                    let p = storage.path().join("right.csv");
                    b.export(&p)?;
                    Editor::open(&p, Some(yy_core::Encoding::Utf8))?
                } else if raw {
                    Editor::open_raw(&path)?
                } else {
                    Editor::open(&path, encoding)?
                };
                diff::Comparison::new(
                    left,
                    right.doc.snapshot().clone(),
                    path.display().to_string(),
                )
            })();
            let _ = tx.send(Message::Compared { book, result });
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
            let message = match message {
                Message::Preview(rendered) => {
                    self.preview = rendered;
                    self.preview_pending = false;
                    if self.view == View::Preview {
                        self.status = "Markdown preview ready".into();
                    }
                    continue;
                }
                other => other,
            };
            self.busy = false;
            let result: Result<()> = match message {
                Message::Preview(_) => unreachable!(),
                Message::Workbook(result)=>(*result).map(|editor|{self.tabs.push(editor);self.tab=self.tabs.len()-1;self.view=View::Sheet;self.explorer=false;self.status="Spreadsheet ready (yysheet engine)".into();}),
                Message::BookOp{tab,book,result}=>{
                    self.tabs[tab].path=book.path.clone();self.tabs[tab].digest=book.digest;self.tabs[tab].book=Some(*book);
                    match result {Ok(())=>{self.status="Spreadsheet ready".into();Ok(())},Err(e)=>Err(e)}
                },
                Message::Compared { book, result } => {
                    if let Some((tab, book)) = book { self.tabs[tab].book = Some(*book); }
                    result.map(|comparison| { self.status = format!("yymerge: {} changed rows | N/P: next/previous difference | F2: editor", comparison.index.changes); self.comparison = Some(comparison); self.view = View::Diff; self.explorer = false; })
                },
                Message::Notice(result)=>result.map(|notice|self.status=notice),
                Message::Fixed(result) => (*result).map(|e| {
                    self.tabs.push(e);self.tab=self.tabs.len()-1;self.view=if self.tabs[self.tab].spreadsheet{View::Sheet}else{View::Table};self.explorer=false;
                    self.status="Fixed-length document opened".into();
                }),
                Message::Page {tab,version,result} => {
                    let editor=&mut self.tabs[tab];
                    if editor.doc.version()==version
                        && let Some(grid)=&mut editor.table {match result {
                            Ok(page)=>{self.status=if page.notice.is_empty(){"Table ready: Enter edit / F8 delete / F9 append".into()}else{page.notice.clone()};
                                grid.row=grid.row.min(page.rows.len().saturating_sub(1));
                                grid.col=grid.col.min(page.rows.get(grid.row).map_or(0,|row|row.fields.len().saturating_sub(1)));
                                grid.page=Some(page);},
                            Err(error)=>{self.status=format!("{error:#}");grid.error=Some(self.status.clone());}
                        }}
                    Ok(())
                },
                Message::Open(result) => (*result).map(|e| {
                    if self.tabs.len() == 1
                        && self.tabs[0].terminal.is_none()
                        && self.tabs[0].path.is_none()
                        && !self.tabs[0].modified()
                        && !self.pending_saves.iter().any(|(tab,_)|*tab==0)
                    {
                        self.tabs[0] = e;
                        self.tab = 0;
                    } else {
                        self.tabs.push(e);
                        self.tab = self.tabs.len() - 1;
                    }
                    self.select_tab(self.tab);
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
        for tab in &mut self.tabs {
            if let Some(shell) = &mut tab.terminal {
                if let Err(error) = shell.poll() {
                    self.status = error.to_string();
                    changed = true;
                }
                changed |= shell.screen.take_dirty();
            }
        }
        if !self.busy
            && let Some((tab, path)) = self.pending_saves.pop_front()
        {
            let active = self.tab;
            self.tab = tab;
            self.save(path);
            self.tab = active;
            changed = true;
        }
        if self.view == View::Table && !self.busy {
            let editor = &mut self.tabs[self.tab];
            if let Some(grid) = &mut editor.table {
                if grid
                    .page
                    .as_ref()
                    .is_some_and(|page| page.version != editor.doc.version())
                {
                    grid.reset();
                }
                if grid.page.is_none() && grid.error.is_none() {
                    let snapshot = editor.doc.snapshot().clone();
                    let version = editor.doc.version();
                    let mode = grid.mode.clone();
                    let start = grid.start;
                    let tab = self.tab;
                    let tx = self.tx.clone();
                    self.busy = true;
                    changed = true;
                    std::thread::spawn(move || {
                        let result = data::read_page(&snapshot, version, &mode, start);
                        let _ = tx.send(Message::Page {
                            tab,
                            version,
                            result,
                        });
                    });
                }
            }
        }
        changed
    }
    fn make_workbook(&mut self, empty: bool) {
        if self.busy {
            return;
        }
        if self.tabs[self.tab].spreadsheet {
            self.view = View::Sheet;
            return;
        }
        let snapshot = self.tabs[self.tab].doc.snapshot().clone();
        let mode = self.tabs[self.tab].table.as_ref().map(|g| g.mode.clone());
        let tx = self.tx.clone();
        self.busy = true;
        std::thread::spawn(move || {
            let result = (|| -> Result<Editor> {
                let book = if empty {
                    workbook::Book::empty()?
                } else {
                    workbook::Book::import(&snapshot, mode.as_ref())?
                };
                let mut editor = Editor::empty();
                editor.spreadsheet = true;
                editor.book = Some(book);
                Ok(editor)
            })();
            let _ = tx.send(Message::Workbook(Box::new(result)));
        });
    }
    fn book_op(&mut self, op: workbook::Op) {
        if self.busy {
            self.status = "Wait for the active operation".into();
            return;
        }
        let Some(mut book) = self.tabs[self.tab].book.take() else {
            self.status = "Use Spreadsheet mode for this command".into();
            return;
        };
        self.busy = true;
        let tab = self.tab;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = book.apply(op);
            let _ = tx.send(Message::BookOp {
                tab,
                book: Box::new(book),
                result,
            });
        });
    }
    fn sheet_key(&mut self, key: KeyEvent) {
        if self.busy {
            return;
        }
        if key.code == K::Enter {
            self.command("Edit cell");
            return;
        }
        if key.modifiers.contains(M::CONTROL) {
            match key.code {
                K::Char('c') | K::Char('x') => {
                    if let Some(book) = &self.tabs[self.tab].book {
                        match book.copy() {
                            Ok(s) => self.clipboard = s,
                            Err(e) => {
                                self.status = e.to_string();
                                return;
                            }
                        }
                    }
                    if key.code == K::Char('x') {
                        self.book_op(workbook::Op::Clear);
                    }
                }
                K::Char('v') => self.book_op(workbook::Op::Paste(self.clipboard.clone())),
                K::Char('z') | K::Char('y') => {
                    if let Some(book) = &mut self.tabs[self.tab].book {
                        if key.code == K::Char('z') {
                            book.doc.undo();
                        } else {
                            book.doc.redo();
                        }
                    }
                }
                K::Char('a') => {
                    if let Some(book) = &mut self.tabs[self.tab].book {
                        let (rows, _) = book.current().extent();
                        let cols = book.data_cols();
                        book.anchor = Some((0, 0));
                        book.row = rows.saturating_sub(1);
                        book.col = cols.saturating_sub(1);
                    }
                }
                _ => {}
            }
            return;
        }
        if key.code == K::Delete {
            self.book_op(workbook::Op::Clear);
            return;
        }
        if let Some(book) = &mut self.tabs[self.tab].book {
            let old = (book.row, book.col);
            match key.code {
                K::Up => book.row = book.row.saturating_sub(1),
                K::Down => book.row = book.row.saturating_add(1).min(1_000_000_000),
                K::Left | K::BackTab => book.col = book.col.saturating_sub(1),
                K::Right | K::Tab => {
                    book.col = book
                        .col
                        .saturating_add(1)
                        .min(if book.current().fixed.is_some() {
                            book.data_cols().saturating_sub(1)
                        } else {
                            16383
                        })
                }
                K::PageUp => book.row = book.row.saturating_sub(20),
                K::PageDown => book.row = book.row.saturating_add(20).min(1_000_000_000),
                K::Home => book.col = 0,
                K::End => book.col = book.data_cols().saturating_sub(1),
                _ => return,
            }
            if key.modifiers.contains(M::SHIFT) {
                book.anchor.get_or_insert(old);
            } else {
                book.anchor = None;
            }
        }
    }
    fn copy_system(&mut self) {
        use base64::Engine;
        let result = if let Some(book) = &self.tabs[self.tab].book {
            book.copy()
        } else if let Some(grid) = &self.tabs[self.tab].table {
            grid.copy()
        } else {
            Ok(self.clipboard.clone())
        };
        match result {
            Ok(text) => {
                if text.len() > 1024 * 1024 {
                    self.status = "Clipboard exceeds 1 MiB".into();
                    return;
                }
                let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
                use std::io::Write;
                if let Err(e) = write!(std::io::stdout(), "\x1b]52;c;{encoded}\x07")
                    .and_then(|_| std::io::stdout().flush())
                {
                    self.status = e.to_string();
                } else {
                    self.clipboard = text;
                    self.status="OSC 52 clipboard copy requested; paste using the terminal's paste shortcut".into();
                }
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    fn open_selected_entry(&mut self) {
        if self.busy {
            return;
        }
        if let Some(path) = self.entries.get(self.selected).cloned() {
            if path.is_dir() {
                self.read_dir(path);
            } else {
                self.open(path);
            }
        }
    }
    fn mouse(&mut self, event: event::MouseEvent) {
        use event::{MouseButton, MouseEventKind as Kind};
        let point = ratatui::layout::Position::new(event.column, event.row);
        let action = self
            .mouse_actions
            .iter()
            .rev()
            .find(|(r, _)| r.contains(point))
            .map(|(_, a)| *a);
        if let Some(mut drag) = self.resize_drag {
            if self.prompt.is_some() || self.palette.is_some() {
                self.resize_drag = None;
                return;
            }
            if matches!(
                event.kind,
                Kind::Drag(MouseButton::Left) | Kind::Up(MouseButton::Left)
            ) {
                let (at, min, max) = match drag.axis {
                    ResizeAxis::Column(_) => (event.column, 4, 80),
                    ResizeAxis::Row(_) => (event.row, 1, 20),
                };
                drag.value = (i32::from(drag.initial) + i32::from(at) - i32::from(drag.origin))
                    .clamp(min, max) as u16;
                if event.kind == Kind::Up(MouseButton::Left) {
                    self.resize_drag = None;
                    if let Some(book) = &mut self.tabs[self.tab].book {
                        book.anchor = None;
                        match drag.axis {
                            ResizeAxis::Column(c) => book.col = c,
                            ResizeAxis::Row(r) => book.row = r,
                        };
                        self.book_op(match drag.axis {
                            ResizeAxis::Column(_) => workbook::Op::Width(drag.value),
                            ResizeAxis::Row(_) => workbook::Op::Height(drag.value),
                        });
                    } else if let Some(grid) = &mut self.tabs[self.tab].table {
                        match drag.axis {
                            ResizeAxis::Column(c) => {
                                grid.widths.insert(c as usize, drag.value);
                            }
                            ResizeAxis::Row(r) => {
                                grid.heights.insert(grid.base + r, drag.value);
                            }
                        }
                    }
                } else {
                    self.resize_drag = Some(drag);
                }
                return;
            }
        }
        if self.prompt.is_some() {
            if event.kind == Kind::Down(MouseButton::Left) {
                match action {
                    Some(MouseAction::Accept) => {
                        if matches!(self.prompt, Some(Prompt::Quit)) {
                            self.input = "discard".into();
                        }
                        self.key(KeyEvent::new(K::Enter, M::NONE));
                    }
                    Some(MouseAction::Cancel) => {
                        self.key(KeyEvent::new(K::Esc, M::NONE));
                    }
                    _ => {}
                }
            }
            return;
        }
        if let Some(selected) = self.palette {
            match event.kind {
                Kind::ScrollUp => self.palette = Some(selected.saturating_sub(3)),
                Kind::ScrollDown => self.palette = Some((selected + 3).min(COMMANDS.len() - 1)),
                Kind::Moved => {
                    if let Some(MouseAction::Command(i)) = action {
                        self.palette = Some(i);
                    }
                }
                Kind::Down(MouseButton::Left) => {
                    if let Some(MouseAction::Command(i)) = action {
                        self.palette = None;
                        self.palette_offset = 0;
                        self.command(COMMANDS[i]);
                    } else if !self.palette_area.contains(point) {
                        self.palette = None;
                        self.palette_offset = 0;
                    }
                }
                _ => {}
            }
            return;
        }
        if event.kind == Kind::Down(MouseButton::Left) {
            match action {
                Some(MouseAction::Key(key)) => {
                    self.key(KeyEvent::new(key, M::NONE));
                    return;
                }
                Some(MouseAction::ColumnSize(c, width)) => {
                    if !self.busy {
                        self.resize_drag = Some(ResizeDrag {
                            axis: ResizeAxis::Column(c),
                            origin: event.column,
                            initial: width,
                            value: width,
                        });
                    }
                    return;
                }
                Some(MouseAction::RowSize(r, height)) => {
                    if !self.busy {
                        self.resize_drag = Some(ResizeDrag {
                            axis: ResizeAxis::Row(r),
                            origin: event.row,
                            initial: height,
                            value: height,
                        });
                    }
                    return;
                }
                Some(MouseAction::Run(name)) => {
                    self.command(name);
                    return;
                }
                Some(MouseAction::Tab(i)) => {
                    self.select_tab(i);
                    return;
                }
                Some(MouseAction::CloseTab(i)) => {
                    if !self.busy {
                        self.select_tab(i);
                        self.command("Close tab");
                    }
                    return;
                }
                Some(MouseAction::TabPage(next)) => {
                    self.tab_manual_scroll = true;
                    self.tab_offset = if next {
                        (self.tab_offset + 1).min(self.tabs.len().saturating_sub(1))
                    } else {
                        self.tab_offset.saturating_sub(1)
                    };
                    return;
                }
                Some(MouseAction::Entry(i)) => {
                    if !self.busy {
                        self.explorer = true;
                        self.selected = i;
                        self.open_selected_entry();
                    }
                    return;
                }
                _ => {}
            }
        }
        if self.busy {
            return;
        }
        if matches!(event.kind, Kind::ScrollUp | Kind::ScrollDown)
            && self.explorer_area.contains(point)
        {
            self.explorer = true;
            self.selected = if event.kind == Kind::ScrollUp {
                self.selected.saturating_sub(3)
            } else {
                (self.selected + 3).min(self.entries.len().saturating_sub(1))
            };
            return;
        }
        if self.preview_area.contains(point) {
            if event.kind == Kind::Down(MouseButton::Left) {
                self.preview_focus = true;
                self.explorer = false;
                return;
            }
            if matches!(event.kind, Kind::ScrollUp | Kind::ScrollDown) {
                self.scroll = if event.kind == Kind::ScrollUp {
                    self.scroll.saturating_sub(3)
                } else {
                    self.scroll.saturating_add(3)
                };
                return;
            }
        }
        if self.view == View::Editor
            && self.editor_area.contains(point)
            && matches!(
                event.kind,
                Kind::Down(MouseButton::Left) | Kind::Drag(MouseButton::Left)
            )
        {
            self.explorer = false;
            self.preview_focus = false;
            if let Some((start, end, _)) = self.rows.get((event.row - self.editor_area.y) as usize)
            {
                let e = &mut self.tabs[self.tab];
                let at = e.offset_at(*start, *end, (event.column - self.editor_area.x) as usize);
                e.move_to(
                    at,
                    event.modifiers.contains(M::SHIFT) || matches!(event.kind, Kind::Drag(_)),
                );
                self.follow_caret = true;
            }
            return;
        }
        if self.content_area.contains(point)
            && matches!(
                event.kind,
                Kind::Down(MouseButton::Left) | Kind::Down(MouseButton::Right)
            )
        {
            self.explorer = false;
        }
        if matches!(event.kind, Kind::ScrollUp | Kind::ScrollDown) {
            let up = event.kind == Kind::ScrollUp;
            if self.content_area.contains(point) {
                match self.view {
                    View::Editor => {
                        self.tabs[self.tab].scroll_rows(!up, 3, self.editor_area.width as usize);
                        self.follow_caret = false;
                    }
                    View::Preview | View::Git | View::Help => {
                        self.scroll = if up {
                            self.scroll.saturating_sub(3)
                        } else {
                            self.scroll.saturating_add(3)
                        };
                    }
                    View::Diff => {
                        if let Some(diff) = &mut self.comparison {
                            diff.top = if up {
                                diff.top.saturating_sub(3)
                            } else {
                                (diff.top + 3).min(diff.index.rows.saturating_sub(1))
                            };
                        }
                    }
                    View::Terminal => {
                        let tab = &mut self.tabs[self.tab];
                        if let Some(shell) = &tab.terminal {
                            tab.term_back = if up {
                                (tab.term_back + 3).min(shell.screen.history_len())
                            } else {
                                tab.term_back.saturating_sub(3)
                            };
                        }
                    }
                    _ => {}
                }
            }

            if self.view == View::Sheet {
                if let Some(book) = &mut self.tabs[self.tab].book {
                    if up {
                        book.row = book.row.saturating_sub(3);
                    } else {
                        book.row = book.row.saturating_add(3);
                    }
                }
            } else if self.view == View::Table {
                for _ in 0..3 {
                    self.table_key(KeyEvent::new(if up { K::Up } else { K::Down }, M::NONE));
                }
            }
            return;
        }
        if !matches!(
            event.kind,
            Kind::Down(MouseButton::Left)
                | Kind::Drag(MouseButton::Left)
                | Kind::Down(MouseButton::Right)
        ) {
            return;
        }
        let Some((_, r, c)) = self
            .hit_cells
            .iter()
            .find(|(rect, _, _)| rect.contains(point))
            .copied()
        else {
            if event.kind == Kind::Down(MouseButton::Right) && self.content_area.contains(point) {
                self.palette = Some(0);
                self.palette_offset = 0;
            }
            return;
        };
        let extend = event.modifiers.contains(M::SHIFT) || matches!(event.kind, Kind::Drag(_));
        if self.view == View::Sheet {
            if let Some(book) = &mut self.tabs[self.tab].book {
                let old = (book.row, book.col);
                if extend {
                    book.anchor.get_or_insert(old);
                } else {
                    book.anchor = None;
                }
                book.row = r;
                book.col = c;
            }
        } else if self.view == View::Table
            && let Some(grid) = &mut self.tabs[self.tab].table
        {
            let old = (grid.row, grid.col);
            if extend {
                grid.anchor.get_or_insert(old);
            } else {
                grid.anchor = None;
            }
            grid.row = r as usize;
            grid.col = c as usize;
        }
        if event.kind == Kind::Down(MouseButton::Right) {
            self.palette = Some(0);
        }
    }
    fn table_mode(&mut self, dialect: yy_delimited::Dialect) {
        if self.tabs[self.tab].spreadsheet {
            self.view = View::Sheet;
            return;
        }
        if self.busy {
            return;
        }
        if self.tabs[self.tab].raw {
            self.status = "Raw fixed-length documents cannot become CSV text".into();
            return;
        }
        self.tabs[self.tab].table = Some(data::Grid::new(data::Mode::Delimited(dialect)));
        self.view = View::Table;
        self.explorer = false;
    }
    fn table_toggle(&mut self) {
        if self.tabs[self.tab].terminal.is_some() {
            self.status = "Select a document tab for table editing".into();
            return;
        }
        if self.tabs[self.tab].spreadsheet {
            self.view = View::Sheet;
            self.explorer = false;
            return;
        }
        if self.view == View::Table && !self.tabs[self.tab].raw {
            self.view = View::Editor;
            self.explorer = false;
            return;
        }
        if self.tabs[self.tab].table.is_some() {
            self.view = View::Table;
            self.explorer = false;
            return;
        }
        let dialect = self.tabs[self.tab]
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|s| s.to_str())
            .and_then(yy_delimited::Dialect::for_extension)
            .unwrap_or_else(yy_delimited::Dialect::csv);
        self.table_mode(dialect);
    }
    fn table_key(&mut self, key: KeyEvent) {
        if self.busy {
            return;
        }
        if key.code == K::Enter {
            self.command("Edit cell");
            return;
        }
        let e = &mut self.tabs[self.tab];
        let Some(grid) = &mut e.table else {
            return;
        };
        if key.modifiers.contains(M::CONTROL) {
            match key.code {
                K::Char('z') => {
                    e.doc.undo();
                    grid.reset();
                }
                K::Char('y') => {
                    e.doc.redo();
                    grid.reset();
                }
                K::Char('c') => match grid.copy() {
                    Ok(text) => self.clipboard = text,
                    Err(error) => self.status = error.to_string(),
                },
                K::Char('v') => {
                    if let Err(error) = grid.paste(&mut e.doc, &self.clipboard) {
                        self.status = error.to_string();
                    }
                }
                _ => {}
            }
            return;
        }
        let old = (grid.row, grid.col);
        if key.code == K::Delete {
            if let Err(e) = grid.clear(&mut e.doc) {
                self.status = e.to_string();
            }
            return;
        }
        let count = grid.page.as_ref().map_or(0, |p| p.rows.len());
        match key.code {
            K::Up => {
                if grid.row == 0 {
                    if grid.previous_page() {
                        grid.row = data::PAGE_ROWS - 1;
                    }
                } else {
                    grid.row -= 1;
                }
            }
            K::Down => {
                if grid.row + 1 >= count {
                    grid.next_page();
                } else {
                    grid.row += 1;
                }
            }
            K::PageDown => {
                grid.next_page();
            }
            K::PageUp => {
                grid.previous_page();
            }
            K::Left => grid.col = grid.col.saturating_sub(1),
            K::Right | K::Tab => {
                grid.col = (grid.col + 1).min(
                    grid.selected()
                        .map_or(0, |r| r.fields.len().saturating_sub(1)),
                )
            }
            K::BackTab => grid.col = grid.col.saturating_sub(1),
            K::Home => grid.col = 0,
            K::End => {
                grid.col = grid
                    .selected()
                    .map_or(0, |r| r.fields.len().saturating_sub(1))
            }
            _ => {}
        }
        if key.modifiers.contains(M::SHIFT) {
            grid.anchor.get_or_insert(old);
        } else {
            grid.anchor = None;
        }
        grid.col = grid.col.min(
            grid.selected()
                .map_or(0, |r| r.fields.len().saturating_sub(1)),
        );
    }
    fn ask(&mut self, prompt: Prompt) {
        self.prompt = Some(prompt);
        self.input.clear();
    }
    fn command(&mut self, name: &str) {
        if self.tabs[self.tab].terminal.is_some()
            && matches!(
                name,
                "Save"
                    | "Save as"
                    | "Spreadsheet mode"
                    | "CSV table"
                    | "TSV table"
                    | "Delimited table"
                    | "Fixed-length table"
                    | "Apply layout (.yyl)"
                    | "Save layout (.yyl)"
                    | "Infer fixed layout"
                    | "Export fixed-length"
                    | "Compare file (yymerge)"
                    | "Markdown preview"
            )
        {
            self.status = "Select a document tab for this operation".into();
            return;
        }

        match name {
            "Spreadsheet mode" => self.make_workbook(false),
            "New workbook" => self.make_workbook(true),
            "Insert rows" => self.book_op(workbook::Op::InsertRows),
            "Delete rows" => self.book_op(workbook::Op::DeleteRows),
            "Insert columns" => self.book_op(workbook::Op::InsertCols),
            "Delete columns" => self.book_op(workbook::Op::DeleteCols),
            "Sort ascending" => self.book_op(workbook::Op::Sort(false)),
            "Sort descending" => self.book_op(workbook::Op::Sort(true)),
            "Clear sort/filter" => self.book_op(workbook::Op::ClearView),
            "Bold" => self.book_op(workbook::Op::Bold),
            "Next sheet" => self.book_op(workbook::Op::NextSheet),
            "Filter column" => self.ask(Prompt::BookFilter),
            "Autofill down" => self.ask(Prompt::BookFill),
            "Row height" => self.ask(Prompt::BookHeight),
            "Export fixed-length" => self.ask(Prompt::FixedExport),
            "Compare file (yymerge)" => self.ask(Prompt::DiffPath),
            "Column width" => self.ask(Prompt::BookWidth),
            "Apply layout (.yyl)" => self.ask(Prompt::LayoutOpen),
            "Save layout (.yyl)" => self.ask(Prompt::LayoutSave),
            "Infer fixed layout" => self.ask(Prompt::LayoutInfer),
            "Number format" => self.ask(Prompt::BookFormat),
            "Move column" => self.ask(Prompt::BookMove),
            "Add sheet" => self.ask(Prompt::BookSheet),
            "Row layout" => self.ask(Prompt::BookLayout),
            "Export CSV" => self.ask(Prompt::BookExport),
            "Copy to system clipboard" => self.copy_system(),
            "CSV table" => self.table_mode(yy_delimited::Dialect::csv()),
            "TSV table" => self.table_mode(yy_delimited::Dialect::tsv()),
            "Delimited table" => self.ask(Prompt::Delimiter),
            "Fixed-length table" => self.ask(Prompt::Fixed),
            "Edit cell" if self.tabs[self.tab].spreadsheet => {
                if let Some(book) = &self.tabs[self.tab].book {
                    match book.entry() {
                        Ok(text) => {
                            self.prompt = Some(Prompt::BookCell);
                            self.input = text;
                        }
                        Err(e) => self.status = e.to_string(),
                    }
                }
            }
            "Edit cell" => {
                if let Some(grid) = &self.tabs[self.tab].table {
                    match grid.value() {
                        Ok(value) => {
                            self.prompt = Some(Prompt::Cell);
                            self.input = value;
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            "Append record" | "Delete record" => {
                if self.busy {
                    return;
                }
                let e = &mut self.tabs[self.tab];
                if let Some(grid) = &mut e.table {
                    let result = if name == "Append record" {
                        grid.append(&mut e.doc)
                    } else {
                        grid.delete(&mut e.doc)
                    };
                    if let Err(error) = result {
                        self.status = error.to_string();
                    }
                }
            }
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
                self.select_tab(self.tab);
                self.view = View::Editor;
                self.explorer = false;
            }
            "Next tab" => self.select_tab((self.tab + 1) % self.tabs.len()),
            "Close tab" => {
                if self.busy || !self.pending_saves.is_empty() {
                    self.status = "Wait for the active operation before closing tabs".into();
                    return;
                }
                if self.tabs[self.tab].modified() {
                    self.status = "Save this document before closing its tab".into();
                    return;
                }
                self.tabs.remove(self.tab);
                if self.tabs.is_empty() {
                    self.tabs.push(Editor::empty());
                }
                self.last_document = self.last_document.min(self.tabs.len() - 1);
                self.select_tab(self.tab.min(self.tabs.len() - 1));
            }
            "Find text" => self.ask(Prompt::Find),
            "Explorer" => {
                self.explorer = true;
            }
            "Terminal" => {
                if let Some(tab) = self.tabs.iter().position(|e| e.terminal.is_some()) {
                    self.select_tab(tab);
                } else {
                    self.new_terminal();
                }
            }
            "New terminal" => self.new_terminal(),
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
            "Preview remote images" => {
                self.preview_remote = !self.preview_remote;
                self.preview_key = None;
                self.status = format!("Remote preview images: {}", self.preview_remote);
            }
            "Split Markdown preview" => {
                if self.tabs[self.tab].terminal.is_some() {
                    self.status = "Select a Markdown document first".into();
                    return;
                }
                self.preview_side = !self.preview_side;
                self.preview_focus = false;
                self.preview_key = None;
                self.view = View::Editor;
                self.explorer = false;
                self.scroll = 0;
            }
            "Copy" | "Cut" | "Paste" | "Undo" | "Redo" | "Select all" => {
                if self.tabs[self.tab].terminal.is_some() {
                    if name == "Paste" {
                        if let Some(shell) = &mut self.tabs[self.tab].terminal {
                            let bytes = yy_term::keys::paste(&self.clipboard, shell.screen.modes());
                            if let Err(e) = shell.send(&bytes) {
                                self.status = e.to_string();
                            }
                        }
                    } else if name == "Copy" {
                        self.clipboard = self.tabs[self.tab]
                            .terminal
                            .as_ref()
                            .unwrap()
                            .screen
                            .screen_text();
                    } else {
                        self.status = "This operation applies to document tabs".into();
                    }
                } else {
                    let key = match name {
                        "Copy" => 'c',
                        "Cut" => 'x',
                        "Paste" => 'v',
                        "Undo" => 'z',
                        "Redo" => 'y',
                        _ => 'a',
                    };
                    self.preview_focus = false;
                    self.view = if self.tabs[self.tab].spreadsheet {
                        View::Sheet
                    } else if self.tabs[self.tab].raw {
                        View::Table
                    } else {
                        View::Editor
                    };
                    self.key(KeyEvent::new(K::Char(key), M::CONTROL));
                }
            }
            "Markdown preview" => {
                self.preview_key = None;
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
                if self.busy || !self.pending_saves.is_empty() {
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
            Prompt::LayoutOpen => {
                if self.busy {
                    return;
                }
                let Some(path) = self.tabs[self.tab].path.clone() else {
                    self.status = "Open a data file first".into();
                    return;
                };
                if self.tabs[self.tab].modified() {
                    self.status = "Save changes before applying a layout".into();
                    return;
                }
                let layout = self.root.join(input);
                let tx = self.tx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let result =
                        layouts::load(&layout).and_then(|spec| layouts::open_data(&path, spec));
                    let _ = tx.send(Message::Fixed(Box::new(result)));
                });
            }
            Prompt::LayoutSave => {
                if self.busy {
                    return;
                }
                let e = &self.tabs[self.tab];
                let spec = e
                    .book
                    .as_ref()
                    .and_then(|b| b.current().fixed.clone())
                    .or_else(|| {
                        e.table.as_ref().and_then(|g| match &g.mode {
                            data::Mode::Fixed(spec) => Some(spec.clone()),
                            _ => None,
                        })
                    });
                let Some(spec) = spec else {
                    self.status = "No fixed layout in this document".into();
                    return;
                };
                let path = self.root.join(input);
                let tx = self.tx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let result = layouts::save(&path, &spec).map(|_| "Layout saved".to_owned());
                    let _ = tx.send(Message::Notice(result));
                });
            }
            Prompt::LayoutInfer => {
                if self.busy {
                    return;
                }
                let e = &self.tabs[self.tab];
                let snapshot = e.raw.then(|| e.doc.snapshot().clone());
                let path = e.path.clone();
                if !e.raw && e.modified() {
                    self.status = "Save text changes before inferring raw bytes".into();
                    return;
                }
                let tx = self.tx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let result = (|| -> Result<Editor> {
                        let raw = if snapshot.is_none() {
                            Some(Editor::open_raw(
                                path.as_deref().context("Open a file first")?,
                            )?)
                        } else {
                            None
                        };
                        let snap = snapshot
                            .as_ref()
                            .or_else(|| raw.as_ref().map(|e| e.doc.snapshot()))
                            .context("No bytes")?;
                        let text = layouts::infer(snap, &input)?;
                        let mut e = Editor::empty();
                        e.doc.insert_text(&text, false);
                        Ok(e)
                    })();
                    let _ = tx.send(Message::Open(Box::new(result)));
                });
            }
            Prompt::BookCell => self.book_op(workbook::Op::Set(input)),
            Prompt::BookFilter => self.book_op(workbook::Op::Filter(input)),
            Prompt::BookFormat => self.book_op(workbook::Op::Format(input)),
            Prompt::BookSheet => self.book_op(workbook::Op::AddSheet(input)),
            Prompt::BookLayout => self.book_op(workbook::Op::RowLayout(input)),
            Prompt::BookExport => self.book_op(workbook::Op::Export(self.root.join(input))),
            Prompt::BookFill => match input.parse() {
                Ok(n) => self.book_op(workbook::Op::Fill(n)),
                Err(_) => self.status = "Enter a row count".into(),
            },
            Prompt::BookHeight if !self.tabs[self.tab].spreadsheet => match input.parse::<u16>() {
                Ok(h) if (1..=20).contains(&h) => {
                    if let Some(g) = &mut self.tabs[self.tab].table {
                        g.height = h;
                    }
                }
                _ => self.status = "Height: 1..20".into(),
            },
            Prompt::BookHeight => match input.parse() {
                Ok(h) => self.book_op(workbook::Op::Height(h)),
                Err(_) => self.status = "Height: 1..20 terminal lines".into(),
            },
            Prompt::FixedExport => {
                let path = self.root.join(input);
                if self.tabs[self.tab].spreadsheet {
                    self.book_op(workbook::Op::ExportFixed(path));
                } else {
                    self.export_raw_fixed(path);
                }
            }
            Prompt::DiffPath => self.diff_file(self.root.join(input)),
            Prompt::BookWidth if !self.tabs[self.tab].spreadsheet => match input.parse::<u16>() {
                Ok(w) if (4..=80).contains(&w) => {
                    if let Some(grid) = &mut self.tabs[self.tab].table {
                        grid.widths.insert(grid.col, w);
                    }
                }
                _ => self.status = "Width: 4..80".into(),
            },
            Prompt::BookWidth => match input.parse() {
                Ok(n) => self.book_op(workbook::Op::Width(n)),
                Err(_) => self.status = "Enter a column width (4..80)".into(),
            },
            Prompt::BookMove => match yy_sheet::parse_col(&input) {
                Some(c) => self.book_op(workbook::Op::MoveColumn(c)),
                None => self.status = "Enter a destination column (A,B,...)".into(),
            },
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
            Prompt::Cell => {
                let e = &mut self.tabs[self.tab];
                if let Some(grid) = &mut e.table {
                    match grid.replace(&mut e.doc, &input) {
                        Ok(()) => self.status = "Cell updated; Ctrl+Z undoes".into(),
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            Prompt::Delimiter => {
                if let Ok(bytes) = yy_delimited::parse_bytes(&input) {
                    if let Some(d) = yy_delimited::Dialect::new(&bytes, Some(b'"')) {
                        self.table_mode(d);
                    } else {
                        self.status = "Delimiter: 1..4 bytes, excluding quotes and newlines".into();
                    }
                } else {
                    self.status = "Invalid delimiter escape".into();
                }
            }
            Prompt::Fixed => {
                if self.busy {
                    return;
                }
                let e = &self.tabs[self.tab];
                if e.modified() {
                    self.status =
                        "Save the document before opening it as fixed-length bytes".into();
                    return;
                }
                let Some(path) = e.path.clone() else {
                    self.status = "Open a data file first".into();
                    return;
                };
                let base = self.root.clone();
                let tx = self.tx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let result = (|| -> Result<Editor> {
                        let spec = data::fixed_spec(&input, &base)?;
                        let e = layouts::open_data(&path, spec)?;
                        Ok(e)
                    })();
                    let _ = tx.send(Message::Fixed(Box::new(result)));
                });
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
                K::Enter
                    if matches!(prompt, Prompt::Cell | Prompt::BookCell)
                        && key.modifiers.contains(M::ALT) =>
                {
                    self.input.push('\n')
                }
                K::Char('a' | 'u') if key.modifiers.contains(M::CONTROL) => self.input.clear(),
                K::Enter => {
                    self.prompt = None;
                    self.submit(prompt);
                }
                K::Char('v') if key.modifiers.contains(M::CONTROL) => {
                    self.input.push_str(&self.clipboard)
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
            K::F(7) => {
                self.table_toggle();
                return;
            }
            K::F(8) if self.view == View::Table => {
                self.command("Delete record");
                return;
            }
            K::F(9) if self.view == View::Table => {
                self.command("Append record");
                return;
            }
            K::F(6) => {
                self.explorer = !self.explorer;
                return;
            }
            K::F(2) => {
                if self.tabs[self.tab].terminal.is_some() {
                    if let Some(i) = self
                        .tabs
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| e.terminal.is_none())
                        .min_by_key(|(i, _)| i.abs_diff(self.last_document))
                        .map(|(i, _)| i)
                    {
                        self.select_tab(i);
                    } else {
                        self.command("New file");
                    }
                }
                self.view = if self.tabs[self.tab].spreadsheet {
                    View::Sheet
                } else if self.tabs[self.tab].raw {
                    View::Table
                } else {
                    View::Editor
                };
                self.preview_focus = false;
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
        if key.modifiers.contains(M::CONTROL) && matches!(key.code, K::Tab | K::Char('w')) {
            self.command(if key.code == K::Tab {
                "Next tab"
            } else {
                "Close tab"
            });
            return;
        }
        if self.view == View::Terminal && !self.explorer {
            let tab = &mut self.tabs[self.tab];
            if let Some(shell) = &mut tab.terminal {
                if key.modifiers.contains(M::SHIFT) && matches!(key.code, K::PageUp | K::PageDown) {
                    if key.code == K::PageUp {
                        tab.term_back =
                            (tab.term_back + shell.screen.rows()).min(shell.screen.history_len());
                    } else {
                        tab.term_back = tab.term_back.saturating_sub(shell.screen.rows());
                    }
                    return;
                }
                tab.term_back = 0;
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
        if self.view == View::Editor && self.preview_side && self.preview_focus && !self.explorer {
            match key.code {
                K::Up => self.scroll = self.scroll.saturating_sub(1),
                K::Down => self.scroll = self.scroll.saturating_add(1),
                K::PageUp => self.scroll = self.scroll.saturating_sub(10),
                K::PageDown => self.scroll = self.scroll.saturating_add(10),
                _ => {}
            }
            return;
        }
        if self.explorer {
            match key.code {
                K::Up => self.selected = self.selected.saturating_sub(1),
                K::Down => {
                    self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1))
                }
                K::Enter => self.open_selected_entry(),
                _ => {}
            }
            return;
        }
        if self.view == View::Sheet {
            self.sheet_key(key);
            return;
        }
        if self.view == View::Table {
            self.table_key(key);
            return;
        }
        if self.view == View::Diff {
            if let Some(diff) = &mut self.comparison {
                let result = match key.code {
                    K::Char('n') => diff.jump(false),
                    K::Char('p') => diff.jump(true),
                    K::Up => {
                        diff.top = diff.top.saturating_sub(1);
                        Ok(())
                    }
                    K::Down => {
                        diff.top = (diff.top + 1).min(diff.index.rows.saturating_sub(1));
                        Ok(())
                    }
                    K::PageUp => {
                        diff.top = diff.top.saturating_sub(20);
                        Ok(())
                    }
                    K::PageDown => {
                        diff.top = (diff.top + 20).min(diff.index.rows.saturating_sub(1));
                        Ok(())
                    }
                    K::Home => {
                        diff.top = 0;
                        Ok(())
                    }
                    K::End => {
                        diff.top = diff.index.rows.saturating_sub(1);
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(e) = result {
                    self.status = e.to_string();
                }
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
                    self.follow_caret = true;
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
        self.follow_caret = true;
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
            let _ = crossterm::execute!(
                std::io::stdout(),
                event::DisableBracketedPaste,
                event::DisableMouseCapture
            );
            ratatui::restore();
        }
    }
    let _restore = Restore;
    crossterm::execute!(
        std::io::stdout(),
        event::EnableBracketedPaste,
        event::EnableMouseCapture
    )?;
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
                Event::Mouse(mouse) => app.mouse(mouse),
                Event::Key(key) => app.key(key),
                Event::Paste(text) => {
                    if app.palette.is_some() {
                        // Pasting into a command menu must not edit the document beneath it.
                    } else if app.prompt.is_some() {
                        app.input.push_str(&text);
                    } else if app.view == View::Terminal {
                        if let Some(shell) = &mut app.tabs[app.tab].terminal {
                            let bytes = yy_term::keys::paste(&text, shell.screen.modes());
                            shell.send(&bytes)?;
                        }
                    } else if app.view == View::Sheet && !app.explorer {
                        app.book_op(workbook::Op::Paste(text));
                    } else if app.view == View::Table && !app.explorer {
                        let e = &mut app.tabs[app.tab];
                        if let Some(grid) = &mut e.table
                            && let Err(error) = grid.paste(&mut e.doc, &text)
                        {
                            app.status = error.to_string();
                        }
                    } else if app.view == View::Editor && !app.explorer && !app.preview_focus {
                        app.preview_focus = false;
                        app.follow_caret = true;
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
            app.tabs[0].doc.insert_text("id,note\n1,example\n", false);
            let mut grid = data::Grid::new(data::Mode::Delimited(yy_delimited::Dialect::csv()));
            grid.page = Some(data::read_page(
                app.tabs[0].doc.snapshot(),
                app.tabs[0].doc.version(),
                &grid.mode,
                0,
            )?);
            grid.row = 1;
            grid.col = 1;
            app.tabs[0].table = Some(grid);
            let mut b = workbook::Book::empty()?;
            b.apply(workbook::Op::Set("multiple\nlines".into()))?;
            b.apply(workbook::Op::Height(3))?;
            app.tabs[0].book = Some(b);
            app.comparison = Some(diff::Comparison::new(
                yy_buffer::Snapshot::from_bytes(b"left\n"),
                yy_buffer::Snapshot::from_bytes(b"right\n"),
                "test".into(),
            )?);
            for view in [
                View::Sheet,
                View::Diff,
                View::Editor,
                View::Table,
                View::Git,
                View::Preview,
                View::Help,
            ] {
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
    fn table_key_workflow_and_save_queued_during_page_load() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("test.csv");
        std::fs::write(&path, b"id,note\r\n1,original\r\n")?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        app.open(path.clone());
        wait(&mut app);
        app.key(KeyEvent::new(K::F(7), M::NONE));
        app.poll();
        wait(&mut app);
        assert!(app.view == View::Table);
        app.key(KeyEvent::new(K::Down, M::NONE));
        app.key(KeyEvent::new(K::Right, M::NONE));
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert_eq!(app.input, "original");
        app.key(KeyEvent::new(K::Char('a'), M::CONTROL));
        assert!(app.input.is_empty());
        app.input = "changed,\nline".into();
        app.key(KeyEvent::new(K::Enter, M::NONE));
        app.poll();
        assert!(app.busy);
        app.key(KeyEvent::new(K::Char('s'), M::CONTROL));
        assert_eq!(app.pending_saves.len(), 1);
        wait(&mut app);
        assert_eq!(
            std::fs::read(&path)?,
            b"id,note\r\n1,\"changed,\nline\"\r\n"
        );
        assert!(!app.tabs[0].modified());
        app.key(KeyEvent::new(K::Char('z'), M::CONTROL));
        app.poll();
        wait(&mut app);
        assert!(app.tabs[0].modified());
        assert_eq!(
            app.tabs[0]
                .doc
                .snapshot()
                .read(0..app.tabs[0].doc.snapshot().len()),
            b"id,note\r\n1,original\r\n"
        );
        Ok(())
    }
    #[test]
    fn fixed_mode_opens_raw_tab_and_rejects_normal_text_input() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("fixed.dat");
        let bytes = [b'A', b'B', b'C', b'D', 0x12, 0x34, 0x5c, 0xde, 0xad];
        std::fs::write(&path, bytes)?;
        std::fs::write(
            dir.path().join("record.cpy"),
            "01 REC.\n 05 ID PIC X(4).\n 05 AMOUNT PIC S9(5) COMP-3.\n",
        )?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        app.open(path.clone());
        wait(&mut app);
        app.input = "record.cpy;9;MS932;none".into();
        app.submit(Prompt::Fixed);
        wait(&mut app);
        assert!(app.tabs[app.tab].raw);
        assert!(app.view == View::Table);
        let before = app.tabs[app.tab].doc.version();
        app.key(KeyEvent::new(K::F(2), M::NONE));
        app.key(KeyEvent::new(K::Char('x'), M::NONE));
        assert_eq!(app.tabs[app.tab].doc.version(), before);
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert_eq!(app.input, "ABCD");
        app.input = "Q".into();
        app.key(KeyEvent::new(K::Enter, M::NONE));
        app.save(path.clone());
        wait(&mut app);
        let saved = std::fs::read(&path)?;
        assert_eq!(&saved[..4], b"Q   ");
        assert_eq!(&saved[4..], &bytes[4..]);
        Ok(())
    }
    #[test]
    fn queued_untitled_save_is_not_replaced_by_a_file_open_result() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let input = dir.path().join("input");
        let output = dir.path().join("output");
        std::fs::write(&input, b"opened file")?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        app.open(input.clone());
        app.save(output.clone());
        wait(&mut app);
        assert_eq!(std::fs::read(output)?, b"");
        assert_eq!(std::fs::read(input)?, b"opened file");
        assert_eq!(app.tabs.len(), 2);
        Ok(())
    }
    fn mouse_at(app: &mut App, kind: event::MouseEventKind, x: u16, y: u16) {
        app.mouse(event::MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: M::NONE,
        });
    }
    fn action_rect(app: &App, predicate: impl Fn(MouseAction) -> bool) -> Rect {
        app.mouse_actions
            .iter()
            .find(|(_, a)| predicate(*a))
            .expect("mouse action missing")
            .0
    }
    #[test]
    fn mouse_text_selection_dialog_buttons_and_tabs() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        app.tabs[0].doc = yy_core::Document::from_text("a日\tb\nz");
        render_mouse(&mut app, 120, 24)?;
        let area = app.editor_area;
        mouse_at(
            &mut app,
            event::MouseEventKind::Down(event::MouseButton::Left),
            area.x + 2,
            area.y,
        );
        mouse_at(
            &mut app,
            event::MouseEventKind::Drag(event::MouseButton::Left),
            area.x + 7,
            area.y,
        );
        assert_eq!(
            app.tabs[0].doc.selected_text(100).unwrap().as_deref(),
            Some("日\t")
        );
        app.command("Copy");
        assert_eq!(app.clipboard, "日\t");
        app.command("Save as");
        app.input = "cancelled.txt".into();
        render_mouse(&mut app, 120, 24)?;
        let cancel = action_rect(&app, |a| matches!(a, MouseAction::Cancel));
        click(&mut app, cancel);
        assert!(app.prompt.is_none());
        assert!(!dir.path().join("cancelled.txt").exists());
        app.command("Save as");
        app.input = "saved.txt".into();
        render_mouse(&mut app, 120, 24)?;
        let accept = action_rect(&app, |a| matches!(a, MouseAction::Accept));
        click(&mut app, accept);
        wait(&mut app);
        assert_eq!(
            std::fs::read(dir.path().join("saved.txt"))?,
            "a日\tb\nz".as_bytes()
        );
        app.command("New file");
        render_mouse(&mut app, 120, 24)?;
        let first = action_rect(&app, |a| matches!(a, MouseAction::Tab(0)));
        click(&mut app, first);
        assert_eq!(app.tab, 0);
        render_mouse(&mut app, 120, 24)?;
        let close = action_rect(&app, |a| matches!(a, MouseAction::CloseTab(1)));
        click(&mut app, close);
        assert_eq!(app.tabs.len(), 1);
        Ok(())
    }
    #[test]
    fn split_preview_follows_unsaved_text_and_scrolls_independently() -> Result<()> {
        let mut app = App::new(std::env::current_dir()?);
        wait(&mut app);
        app.tabs[0].doc = yy_core::Document::from_text(
            &(0..40).map(|i| format!("line {i}\n")).collect::<String>(),
        );
        app.command("Split Markdown preview");
        render_mouse(&mut app, 120, 24)?;
        assert!(app.preview_area.x > app.editor_area.x);
        assert!(app.preview_area.width > 0);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.preview_pending {
            app.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(app.preview.plain().contains("line 0"));
        app.tabs[0].doc.insert_text("UNSAVED\n", false);
        render_mouse(&mut app, 120, 24)?;
        while app.preview_pending {
            app.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(app.preview.plain().contains("UNSAVED"));
        let area = app.preview_area;
        let caret = app.tabs[0].caret();
        mouse_at(&mut app, event::MouseEventKind::ScrollDown, area.x, area.y);
        assert_eq!(app.scroll, 3);
        assert_eq!(app.tabs[0].caret(), caret);
        let area = app.editor_area;
        mouse_at(&mut app, event::MouseEventKind::ScrollDown, area.x, area.y);
        render_mouse(&mut app, 120, 24)?;
        assert!(app.tabs[0].top > 0);
        assert_eq!(app.scroll, 3);
        render_mouse(&mut app, 40, 16)?;
        assert!(app.preview_area.y > app.editor_area.y);
        for (w, h) in [(1, 1), (20, 6), (40, 12)] {
            render_mouse(&mut app, w, h)?;
        }
        Ok(())
    }
    fn drag_resize(app: &mut App, rect: Rect, dx: u16, dy: u16) {
        mouse_at(
            app,
            event::MouseEventKind::Down(event::MouseButton::Left),
            rect.x,
            rect.y,
        );
        mouse_at(
            app,
            event::MouseEventKind::Drag(event::MouseButton::Left),
            rect.x + dx,
            rect.y + dy,
        );
        mouse_at(
            app,
            event::MouseEventKind::Up(event::MouseButton::Left),
            rect.x + dx,
            rect.y + dy,
        );
    }
    #[test]
    fn mouse_drag_resizes_raw_grid_and_workbook_with_one_undo() -> Result<()> {
        let mut app = App::new(std::env::current_dir()?);
        wait(&mut app);
        app.tabs[0].doc = yy_core::Document::from_text("a,b\n1,2\n");
        app.table_mode(yy_delimited::Dialect::csv());
        app.poll();
        wait(&mut app);
        render_mouse(&mut app, 120, 24)?;
        let original = app.tabs[0]
            .doc
            .snapshot()
            .read(0..app.tabs[0].doc.snapshot().len());
        let col = action_rect(&app, |a| matches!(a, MouseAction::ColumnSize(0, _)));
        drag_resize(&mut app, col, 5, 0);
        render_mouse(&mut app, 120, 24)?;
        let row = action_rect(&app, |a| matches!(a, MouseAction::RowSize(0, _)));
        drag_resize(&mut app, row, 0, 3);
        let grid = app.tabs[0].table.as_ref().unwrap();
        assert_eq!(grid.column_width(0), 24);
        assert_eq!(grid.row_height(0), 4);
        assert_eq!(
            app.tabs[0].doc.snapshot().read(0..original.len() as u64),
            original
        );
        let mut book = workbook::Book::empty()?;
        book.apply(workbook::Op::Set("text".into()))?;
        app.tabs[0].book = Some(book);
        app.tabs[0].spreadsheet = true;
        app.view = View::Sheet;
        render_mouse(&mut app, 120, 24)?;
        let col = action_rect(&app, |a| matches!(a, MouseAction::ColumnSize(0, _)));
        drag_resize(&mut app, col, 5, 0);
        wait(&mut app);
        assert_eq!(
            app.tabs[0]
                .book
                .as_ref()
                .unwrap()
                .current()
                .col_widths
                .get(&0),
            Some(&21.0)
        );
        render_mouse(&mut app, 120, 24)?;
        let row = action_rect(&app, |a| matches!(a, MouseAction::RowSize(0, _)));
        drag_resize(&mut app, row, 0, 3);
        wait(&mut app);
        let book = app.tabs[0].book.as_mut().unwrap();
        assert_eq!(book.row_height(0), 4);
        book.doc.undo();
        assert_eq!(book.row_height(0), 1);
        book.doc.undo();
        assert!(!book.current().col_widths.contains_key(&0));
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn terminal_tabs_keep_background_output_and_close_individually() -> Result<()> {
        let mut app = App::new(std::env::current_dir()?);
        wait(&mut app);
        app.tabs[0].doc.insert_text("document", false);
        app.command("New terminal");
        assert_eq!(app.tab, 1);
        assert!(app.view == View::Terminal);
        app.key(KeyEvent::new(K::F(7), M::NONE));
        assert!(app.view == View::Terminal);
        assert!(app.tabs[1].table.is_none());
        app.tabs[1]
            .terminal
            .as_mut()
            .unwrap()
            .send(b"printf '\\102\\101\\103\\113\\107\\122\\117\\125\\116\\104\\n'\r")?;
        app.select_tab(0);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            app.poll();
            if app.tabs[1]
                .terminal
                .as_ref()
                .unwrap()
                .screen
                .screen_text()
                .contains("BACKGROUND")
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        app.command("Terminal");
        assert_eq!(app.tab, 1);
        app.command("New terminal");
        assert_eq!(app.tabs.len(), 3);
        app.command("Close tab");
        assert_eq!(app.tabs.len(), 2);
        assert!(app.tabs[1].terminal.is_some());
        app.select_tab(1);
        app.command("Close tab");
        assert_eq!(app.tabs.len(), 1);
        assert!(app.tabs[0].modified());
        assert_eq!(app.tabs[0].doc.snapshot().read(0..8), b"document");
        Ok(())
    }
    fn render_mouse(app: &mut App, width: u16, height: u16) -> Result<()> {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
        terminal.draw(|f| draw(f, app))?;
        Ok(())
    }
    fn click(app: &mut App, rect: Rect) {
        app.mouse(event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: M::NONE,
        });
    }
    #[test]
    fn mouse_opens_menu_commands_and_blocks_background_clicks() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("child"))?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        render_mouse(&mut app, 80, 16)?;
        let menu = app
            .mouse_actions
            .iter()
            .find(|(_, a)| matches!(a, MouseAction::Key(K::F(1))))
            .unwrap()
            .0;
        click(&mut app, menu);
        assert_eq!(app.palette, Some(0));
        render_mouse(&mut app, 80, 16)?;
        let new = app
            .mouse_actions
            .iter()
            .find(|(_, a)| matches!(a,MouseAction::Command(i) if COMMANDS[*i]=="New file"))
            .unwrap()
            .0;
        let count = app.tabs.len();
        click(&mut app, new);
        assert_eq!(app.tabs.len(), count + 1);
        assert!(app.palette.is_none());
        render_mouse(&mut app, 80, 16)?;
        click(&mut app, menu);
        render_mouse(&mut app, 80, 16)?;
        let entry=app.mouse_actions.iter().find(|(_,a)|matches!(a,MouseAction::Entry(i) if app.entries[*i].file_name().is_some_and(|s|s=="child"))).unwrap().0;
        let root = app.root.clone();
        click(&mut app, entry);
        assert!(app.palette.is_none());
        assert_eq!(app.root, root);
        assert!(!app.busy);
        Ok(())
    }
    #[test]
    fn mouse_folder_file_parent_and_scrolled_menu_at_small_sizes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;
        std::fs::write(child.join("note.txt"), b"from mouse\n")?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        app.explorer = true;
        render_mouse(&mut app, 40, 12)?;
        let entry = app
            .mouse_actions
            .iter()
            .find(|(_, a)| matches!(a,MouseAction::Entry(i) if app.entries[*i]==child))
            .unwrap()
            .0;
        click(&mut app, entry);
        wait(&mut app);
        assert_eq!(app.root, child);
        render_mouse(&mut app, 40, 12)?;
        let file=app.mouse_actions.iter().find(|(_,a)|matches!(a,MouseAction::Entry(i) if app.entries[*i].file_name().is_some_and(|s|s=="note.txt"))).unwrap().0;
        click(&mut app, file);
        wait(&mut app);
        assert_eq!(
            app.tabs[app.tab].path,
            Some(child.join("note.txt").canonicalize()?)
        );
        app.explorer = true;
        render_mouse(&mut app, 40, 12)?;
        let parent = app
            .mouse_actions
            .iter()
            .find(|(_, a)| matches!(a, MouseAction::Entry(0)))
            .unwrap()
            .0;
        click(&mut app, parent);
        wait(&mut app);
        assert_eq!(app.root, dir.path());
        app.palette = Some(COMMANDS.iter().position(|c| *c == "Help").unwrap());
        render_mouse(&mut app, 40, 8)?;
        assert!(app.palette_offset > 0);
        let help = app
            .mouse_actions
            .iter()
            .find(|(_, a)| matches!(a,MouseAction::Command(i) if COMMANDS[*i]=="Help"))
            .unwrap()
            .0;
        click(&mut app, help);
        assert!(app.view == View::Help);
        assert!(app.palette.is_none());
        Ok(())
    }
    #[test]
    fn mouse_rectangle_tracks_resized_row_heights() -> Result<()> {
        let mut app = App::new(std::env::current_dir()?);
        wait(&mut app);
        let mut book = workbook::Book::empty()?;
        book.apply(workbook::Op::Paste("a\tb\nc\td".into()))?;
        book.apply(workbook::Op::Height(3))?;
        book.apply(workbook::Op::Width(12))?;
        app.tabs[0].book = Some(book);
        app.tabs[0].spreadsheet = true;
        app.view = View::Sheet;
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 15))?;
        terminal.draw(|frame| draw(frame, &mut app))?;
        let first = app
            .hit_cells
            .iter()
            .find(|(_, r, c)| *r == 0 && *c == 0)
            .unwrap()
            .0;
        assert_eq!(first.height, 3);
        assert_eq!(first.width, 12);
        let last = app
            .hit_cells
            .iter()
            .find(|(_, r, c)| *r == 1 && *c == 1)
            .unwrap()
            .0;
        for (kind, rect) in [
            (event::MouseEventKind::Down(event::MouseButton::Left), first),
            (event::MouseEventKind::Drag(event::MouseButton::Left), last),
        ] {
            app.mouse(event::MouseEvent {
                kind,
                column: rect.x,
                row: rect.y + rect.height - 1,
                modifiers: M::NONE,
            });
        }
        assert_eq!(app.tabs[0].book.as_ref().unwrap().copy()?, "a\tb\nc\td");
        app.mouse(event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Right),
            column: first.x,
            row: first.y,
            modifiers: M::NONE,
        });
        assert_eq!(app.palette, Some(0));
        let book = app.tabs[0].book.as_ref().unwrap();
        assert_eq!((book.row, book.col), (0, 0));
        Ok(())
    }
    #[test]
    fn original_fixed_export_and_shared_diff_workers_keep_snapshots_alive() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("raw.dat");
        let raw = b"AB\x00\xff";
        std::fs::write(&path, raw)?;
        let mut app = App::new(dir.path().to_owned());
        wait(&mut app);
        let mut editor = Editor::open_raw(&path)?;
        let spec = yy_sheet::fixed::FixedSpec::new(
            "01 REC.\n 05 ID PIC X(4).\n",
            yy_cobol::Codec {
                charset: yy_cobol::Charset::Ms932,
                little_endian: false,
            },
            yy_sheet::fixed::RecordSep::None,
        )
        .map_err(anyhow::Error::msg)?;
        editor.table = Some(data::Grid::new(data::Mode::Fixed(std::sync::Arc::new(
            spec,
        ))));
        app.tabs = vec![editor];
        app.tab = 0;
        let out = dir.path().join("out.dat");
        app.export_raw_fixed(out.clone());
        wait(&mut app);
        assert_eq!(std::fs::read(&out)?, raw);
        app.diff_file(out);
        wait(&mut app);
        assert!(app.view == View::Diff);
        let d = app.comparison.as_mut().context("Missing diff")?;
        assert_eq!(d.index.changes, 0);
        assert_eq!(d.rows(1)?.len(), 1);
        let text = dir.path().join("right.txt");
        std::fs::write(&text, b"right\n")?;
        app.tabs = vec![Editor::empty()];
        app.tabs[0].doc = yy_core::Document::from_text("left\n");
        app.diff_file(text);
        wait(&mut app);
        let d = app.comparison.as_mut().context("Missing diff")?;
        let row = d.rows(1)?[0];
        assert_eq!(d.text(row, false), "right↵");
        assert_eq!(d.text(row, true), "left↵");
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
