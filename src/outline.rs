//! Terminal outline editing of yyeditor's yyoutline version 1 JSON documents.
use crate::{App, MouseAction, Prompt, View};
use anyhow::{Context, Result, bail, ensure};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicU64, Ordering},
};

const LIMIT: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub depth: usize,
    pub text: String,
    pub bookmark: bool,
    pub collapsed: bool,
    pub refs: Vec<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    format: String,
    version: u64,
    pub nodes: Vec<Node>,
    #[serde(rename = "listStyle", skip_serializing_if = "Option::is_none")]
    list_style: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}
impl Model {
    pub fn parse(text: &str) -> Result<Self> {
        ensure!(text.len() <= LIMIT, "Outline exceeds 1 MiB");
        let mut value: Value = if text.trim().is_empty() {
            serde_json::from_str(r#"{"format":"yyoutline","version":1,"nodes":[]}"#)?
        } else {
            serde_json::from_str(text).context("Invalid outline JSON")?
        };
        if let Some(style) = value.get("listStyle") {
            ensure!(
                matches!(style.as_str(), Some("bullets" | "numbered")),
                "Invalid listStyle"
            );
        }
        // JavaScript accepts integer-valued JSON numbers such as 1.0.
        fn integer(value: &mut Value) -> Result<()> {
            let number = value.as_f64().context("Expected an integer")?;
            ensure!(
                number >= 0.0 && number.fract() == 0.0 && number <= 9_007_199_254_740_991.0,
                "Expected a safe non-negative integer"
            );
            *value = Value::from(number as u64);
            Ok(())
        }
        if let Some(v) = value.get_mut("version") {
            integer(v)?;
        }
        if let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) {
            for node in nodes {
                if let Some(d) = node.get_mut("depth") {
                    integer(d)?;
                }
            }
        }
        let model: Self = serde_json::from_value(value).context("Invalid outline fields")?;
        ensure!(
            model.format == "yyoutline" && model.version == 1,
            "Expected yyoutline version 1"
        );
        ensure!(
            model
                .list_style
                .as_deref()
                .is_none_or(|s| matches!(s, "bullets" | "numbered")),
            "Invalid listStyle"
        );
        let mut ids = HashSet::new();
        let mut allowed = 0;
        for node in &model.nodes {
            ensure!(
                !node.id.is_empty() && ids.insert(&node.id),
                "Empty or duplicate outline ID"
            );
            ensure!(node.depth <= allowed, "Invalid outline depth");
            allowed = node.depth + 1;
        }
        Ok(model)
    }
    pub fn json(&self) -> Result<String> {
        let text = serde_json::to_string_pretty(self)? + "\n";
        ensure!(
            text.len() <= LIMIT,
            "Outline exceeds 1 MiB; change was not applied"
        );
        Ok(text)
    }
    pub fn end(&self, at: usize) -> usize {
        (at + 1..self.nodes.len())
            .find(|&i| self.nodes[i].depth <= self.nodes[at].depth)
            .unwrap_or(self.nodes.len())
    }
    fn add(&mut self, at: Option<usize>, zoom: Option<&str>) -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = loop {
            let candidate = format!(
                "n-{:x}-{:x}-{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            if !self.nodes.iter().any(|n| n.id == candidate) {
                break candidate;
            }
        };
        let depth = at.map_or(0, |i| {
            self.nodes[i].depth + usize::from(zoom == Some(self.nodes[i].id.as_str()))
        });
        let insert = at.map_or(self.nodes.len(), |i| self.end(i));
        self.nodes.insert(
            insert,
            Node {
                id: id.clone(),
                depth,
                text: String::new(),
                bookmark: false,
                collapsed: false,
                refs: vec![],
                extra: Map::new(),
            },
        );
        id
    }
    fn move_branch(&mut self, at: usize, op: &str) -> bool {
        let stop = self.end(at);
        let depth = self.nodes[at].depth;
        match op {
            "indent" | "up" if at > 0 && self.nodes[at - 1].depth >= depth => {
                let previous = (0..at).rfind(|&i| self.nodes[i].depth == depth).unwrap();
                if op == "indent" {
                    for node in &mut self.nodes[at..stop] {
                        node.depth += 1;
                    }
                    self.nodes[previous].collapsed = false;
                } else {
                    self.nodes[previous..stop].rotate_right(stop - at);
                }
            }
            "outdent" if depth > 0 => {
                let parent = (0..at).rfind(|&i| self.nodes[i].depth < depth).unwrap();
                let end = self.end(parent);
                for node in &mut self.nodes[at..stop] {
                    node.depth -= 1;
                }
                self.nodes[at..end].rotate_left(stop - at);
            }
            "down" if stop < self.nodes.len() && self.nodes[stop].depth == depth => {
                let end = self.end(stop);
                self.nodes[at..end].rotate_left(stop - at);
            }
            _ => return false,
        }
        true
    }
    pub fn markdown(&self) -> Result<String> {
        let numbers = self.numbers();
        let mut indents = vec![0usize];
        let mut output = String::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let mut text = String::new();
            for c in n
                .text
                .replace("\r\n", " ")
                .replace(['\r', '\n'], " ")
                .chars()
            {
                if "\\`*_[]{}<>#!|~&".contains(c) {
                    text.push('\\');
                }
                text.push(c);
            }
            let trimmed = text.trim_start();
            let offset = text.len() - trimmed.len();
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            let punctuation = if digits > 0
                && trimmed
                    .as_bytes()
                    .get(digits)
                    .is_some_and(|c| matches!(c, b'.' | b')'))
            {
                Some(digits)
            } else if trimmed.starts_with(['-', '+']) {
                Some(0)
            } else {
                None
            };
            if let Some(p) = punctuation
                && trimmed[p + 1..].starts_with(char::is_whitespace)
            {
                text.insert(offset + p, '\\');
            }
            if n.depth < 6 {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&"#".repeat(n.depth + 1));
                output.push(' ');
            } else {
                let level = n.depth - 6;
                let marker = if self.list_style.as_deref() == Some("numbered") {
                    format!("{}. ", numbers[i])
                } else {
                    "- ".into()
                };
                indents.resize(level + 2, 0);
                output.push_str(&" ".repeat(indents[level]));
                output.push_str(&marker);
                indents[level + 1] = indents[level] + marker.len().max(4);
            }
            output.push_str(&text);
            output.push('\n');
            ensure!(output.len() <= LIMIT, "Markdown exceeds 1 MiB");
        }
        Ok(output)
    }
    fn numbers(&self) -> Vec<usize> {
        let mut counts = Vec::new();
        self.nodes
            .iter()
            .map(|n| {
                counts.resize(n.depth + 1, 0);
                counts[n.depth] += 1;
                counts[n.depth]
            })
            .collect()
    }
}

#[derive(Default)]
pub struct State {
    pub active: bool,
    version: Option<yy_core::Version>,
    model: Option<Model>,
    error: String,
    selected: Option<String>,
    zoom: Option<String>,
    top: usize,
}
impl State {
    fn sync(&mut self, doc: &yy_core::Document) {
        if self.version == Some(doc.version()) {
            return;
        }
        self.version = Some(doc.version());
        let parsed = (|| {
            ensure!(
                doc.snapshot().len() <= LIMIT as u64,
                "Outline exceeds 1 MiB"
            );
            let bytes = doc.snapshot().read(0..doc.snapshot().len());
            Model::parse(std::str::from_utf8(&bytes).context("Outline is not UTF-8 text")?)
        })();
        match parsed {
            Ok(model) => {
                self.error.clear();
                self.model = Some(model);
                self.reconcile();
            }
            Err(e) => {
                self.error = format!("{e:#}");
                self.model = None;
            }
        }
    }
    fn reconcile(&mut self) {
        let Some(m) = &self.model else { return };
        if !m.nodes.iter().any(|n| Some(&n.id) == self.zoom.as_ref()) {
            self.zoom = None;
        }
        let visible = self.visible();
        if !visible
            .iter()
            .any(|&i| Some(&m.nodes[i].id) == self.selected.as_ref())
        {
            self.selected = visible.first().map(|&i| m.nodes[i].id.clone());
        }
    }
    fn index(&self) -> Option<usize> {
        self.model
            .as_ref()?
            .nodes
            .iter()
            .position(|n| Some(&n.id) == self.selected.as_ref())
    }
    fn visible(&self) -> Vec<usize> {
        let Some(m) = &self.model else { return vec![] };
        let start = self
            .zoom
            .as_ref()
            .and_then(|id| m.nodes.iter().position(|n| &n.id == id))
            .unwrap_or(0);
        let stop = if self.zoom.is_some() && start < m.nodes.len() {
            m.end(start)
        } else {
            m.nodes.len()
        };
        let mut hidden = None;
        (start..stop)
            .filter(|&i| {
                let n = &m.nodes[i];
                if hidden.is_some_and(|d| n.depth > d) {
                    return false;
                }
                hidden = if n.collapsed && Some(&n.id) != self.zoom.as_ref() {
                    Some(n.depth)
                } else {
                    None
                };
                true
            })
            .collect()
    }
}

impl App {
    fn outline_ask(&mut self, prompt: Prompt) {
        self.ask(prompt);
        self.outline_prompt = Some((
            self.tab,
            self.tabs[self.tab].doc.version(),
            self.tabs[self.tab].outline.selected.clone(),
        ));
    }
    pub(crate) fn outline_prompt_submit(&mut self, prompt: Prompt, input: String) -> Result<()> {
        let (tab, version, selected) = self
            .outline_prompt
            .take()
            .context("Outline input expired")?;
        ensure!(
            self.view == View::Outline
                && self.tab == tab
                && self.tabs[self.tab].doc.version() == version
                && self.tabs[self.tab].outline.selected == selected,
            "Outline document or item changed while input was open; edit was not applied"
        );
        self.outline_submit(prompt, input)
    }
    pub(crate) fn outline_command(&mut self, name: &str) -> bool {
        if !name.starts_with("Outline ") {
            return false;
        }
        let result = self.outline_action(name.trim_start_matches("Outline "));
        if let Err(e) = result {
            self.status = format!("{e:#}");
        }
        true
    }
    fn outline_action(&mut self, op: &str) -> Result<()> {
        ensure!(
            !self.tabs[self.tab].raw
                && !self.tabs[self.tab].spreadsheet
                && self.tabs[self.tab].terminal.is_none()
                && !self.tabs[self.tab].database,
            "Select a text document for outline editing"
        );
        if op == "source" {
            self.tabs[self.tab].outline.active = false;
            self.view = View::Editor;
            return Ok(());
        }
        let e = &mut self.tabs[self.tab];
        e.outline.sync(&e.doc);
        let editable = can_edit(e);
        let s = &mut e.outline;
        if op == "editor" {
            ensure!(s.model.is_some(), "{}; JSON source was kept", s.error);
            s.active = true;
            self.view = View::Outline;
            self.explorer = false;
            self.status =
                "Outline ready: Enter add / F2 edit / Tab indent / Space fold / Ctrl+S save".into();
            return Ok(());
        }
        ensure!(self.view == View::Outline, "Open Outline editor first");
        let mut model = s
            .model
            .clone()
            .context("Invalid outline; switch to source to repair")?;
        let at = s.index();
        match op {
            "open URL" => {
                let url = urls(&model.nodes[at.context("Select an outline item")?].text)
                    .into_iter()
                    .next()
                    .context("No HTTP(S) URL in selected item")?;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = (|| -> Result<String> {
                        #[cfg(windows)]
                        let mut command = {
                            use std::os::windows::process::CommandExt;
                            let mut c = std::process::Command::new("rundll32.exe");
                            c.args(["url.dll,FileProtocolHandler", &url])
                                .creation_flags(0x08000000);
                            c
                        };
                        #[cfg(not(windows))]
                        let mut command = {
                            let mut c = std::process::Command::new("xdg-open");
                            c.arg(&url);
                            c
                        };
                        let mut child = command.spawn().context("Cannot start URL handler")?;
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(5);
                        loop {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    ensure!(status.success(), "URL handler failed: {status}");
                                    return Ok(format!("Opened {url}"));
                                }
                                Ok(None) if std::time::Instant::now() < deadline => {
                                    std::thread::park_timeout(std::time::Duration::from_millis(10))
                                }
                                other => {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    if let Err(e) = other {
                                        return Err(e.into());
                                    }
                                    bail!("URL handler timed out");
                                }
                            }
                        }
                    })();
                    let _ = tx.send(crate::Message::Notice(result));
                });
                return Ok(());
            }
            "all" => {
                s.zoom = None;
                s.reconcile();
                return Ok(());
            }
            "zoom" => {
                s.zoom = s.selected.clone();
                s.top = 0;
                return Ok(());
            }
            "parent" => {
                if let Some(i) = s
                    .zoom
                    .as_ref()
                    .and_then(|id| model.nodes.iter().position(|n| &n.id == id))
                {
                    s.zoom = (0..i)
                        .rfind(|&j| model.nodes[j].depth < model.nodes[i].depth)
                        .map(|j| model.nodes[j].id.clone());
                } else {
                    s.zoom = None;
                }
                s.top = 0;
                s.reconcile();
                return Ok(());
            }
            "edit" | "reference" | "remove reference" | "export" | "delete" => {
                ensure!(op == "export" || editable, "Outline is read-only or busy");
                let prompt = match op {
                    "edit" => Prompt::OutlineEdit,
                    "reference" => Prompt::OutlineRef,
                    "remove reference" => Prompt::OutlineUnref,
                    "export" => Prompt::OutlineExport,
                    _ => Prompt::OutlineDelete,
                };
                if op != "export" {
                    ensure!(at.is_some(), "Select an outline item");
                }
                self.outline_ask(prompt);
                if op == "edit" {
                    self.input = model.nodes[at.unwrap()].text.clone();
                }
                if op == "reference" || op == "remove reference" {
                    self.status = "Enter the target item ID shown beside each item".into();
                }
                if op == "delete" {
                    let count = model.end(at.unwrap()) - at.unwrap();
                    self.status = format!("Delete branch ({count} items)? Enter yes to confirm");
                }
                return Ok(());
            }
            "bookmarks" | "backlinks" | "follow reference" => {
                let candidates: Vec<_> = model
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| match op {
                        "bookmarks" => n.bookmark,
                        "backlinks" => s.selected.as_ref().is_some_and(|id| n.refs.contains(id)),
                        _ => at.is_some_and(|i| model.nodes[i].refs.contains(&n.id)),
                    })
                    .map(|(i, _)| i)
                    .collect();
                ensure!(
                    !candidates.is_empty(),
                    "No matching items (references may be missing)"
                );
                if op != "bookmarks" {
                    self.outline_ask(Prompt::OutlineJump(op == "backlinks"));
                    self.input = model.nodes[candidates[0]].id.clone();
                    self.status = format!(
                        "Targets: {}",
                        safe(
                            &candidates
                                .iter()
                                .take(16)
                                .map(|&i| model.nodes[i].id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    );
                    return Ok(());
                }
                let next = candidates
                    .iter()
                    .copied()
                    .find(|&i| at.is_none_or(|a| i > a))
                    .unwrap_or(candidates[0]);
                s.selected = Some(model.nodes[next].id.clone());
                s.zoom = s.selected.clone();
                s.top = 0;
                return Ok(());
            }
            _ => {}
        }
        ensure!(editable, "Outline is read-only or busy");
        let mut selected = s.selected.clone();
        match op {
            "add" => selected = Some(model.add(at, s.zoom.as_deref())),
            "style" => {
                model.list_style = Some(
                    if model.list_style.as_deref() == Some("numbered") {
                        "bullets"
                    } else {
                        "numbered"
                    }
                    .into(),
                )
            }
            "bookmark" => {
                let i = at.context("Select an outline item")?;
                model.nodes[i].bookmark = !model.nodes[i].bookmark;
            }
            "fold" => {
                let i = at.context("Select an outline item")?;
                if model.end(i) == i + 1 {
                    return Ok(());
                }
                model.nodes[i].collapsed = !model.nodes[i].collapsed;
            }
            "indent" | "outdent" | "up" | "down" => {
                if !model.move_branch(at.context("Select an outline item")?, op) {
                    return Ok(());
                }
            }
            _ => bail!("Unknown outline operation"),
        }
        commit(e, model)?;
        e.outline.selected = selected;
        e.outline.reconcile();
        Ok(())
    }
    pub(crate) fn outline_submit(&mut self, prompt: Prompt, input: String) -> Result<()> {
        ensure!(input.len() <= LIMIT, "Outline input exceeds 1 MiB");
        let e = &mut self.tabs[self.tab];
        e.outline.sync(&e.doc);
        let mut model = e.outline.model.clone().context("Invalid outline JSON")?;
        if matches!(prompt, Prompt::OutlineExport) {
            let path = self.root.join(input);
            ensure!(
                e.path.as_ref().is_none_or(|p| p != &path),
                "Choose a separate Markdown file"
            );
            let markdown = model.markdown()?;
            let mut output = tempfile::NamedTempFile::new_in(
                path.parent().context("Export path has no parent")?,
            )?;
            use std::io::Write;
            output.write_all(markdown.as_bytes())?;
            output.as_file().sync_all()?;
            output
                .persist_noclobber(path)
                .context("Choose a new Markdown path; existing files are not overwritten")?;
            self.status = "Outline Markdown exported".into();
            return Ok(());
        }
        let i = e.outline.index().context("Select an outline item")?;
        if let Prompt::OutlineJump(backlink) = prompt {
            let target = model
                .nodes
                .iter()
                .position(|n| n.id == input)
                .context("Unknown target ID")?;
            ensure!(
                if backlink {
                    model.nodes[target].refs.contains(&model.nodes[i].id)
                } else {
                    model.nodes[i].refs.contains(&input)
                },
                "Item is not a reference target"
            );
            e.outline.selected = Some(input);
            e.outline.zoom = e.outline.selected.clone();
            e.outline.top = 0;
            return Ok(());
        }
        ensure!(can_edit(e), "Outline is read-only or busy");
        let mut selected = e.outline.selected.clone();
        match prompt {
            Prompt::OutlineEdit => model.nodes[i].text = input,
            Prompt::OutlineRef => {
                ensure!(
                    model.nodes.iter().any(|n| n.id == input) && model.nodes[i].id != input,
                    "Unknown or self reference"
                );
                if !model.nodes[i].refs.contains(&input) {
                    model.nodes[i].refs.push(input);
                }
            }
            Prompt::OutlineUnref => model.nodes[i].refs.retain(|r| r != &input),
            Prompt::OutlineDelete => {
                ensure!(input == "yes", "Branch deletion cancelled");
                let end = model.end(i);
                model.nodes.drain(i..end);
                selected = model
                    .nodes
                    .get(i.min(model.nodes.len().saturating_sub(1)))
                    .map(|n| n.id.clone());
            }
            _ => unreachable!(),
        }
        commit(e, model)?;
        e.outline.selected = selected;
        e.outline.reconcile();
        Ok(())
    }
    pub(crate) fn outline_key(&mut self, key: KeyEvent) {
        let command = match (key.code, key.modifiers) {
            (K::Up, m) if m.contains(M::ALT) => Some("Outline up"),
            (K::Down, m) if m.contains(M::ALT) => Some("Outline down"),
            (K::Enter, _) => Some("Outline add"),
            (K::Tab, _) => Some("Outline indent"),
            (K::BackTab, _) => Some("Outline outdent"),
            (K::F(2), _) => Some("Outline edit"),
            (K::Char(' '), M::NONE) => Some("Outline fold"),
            (K::Right, _) => Some("Outline zoom"),
            (K::Left, _) => Some("Outline parent"),
            (K::Esc, _) => Some("Outline all"),
            (K::Delete, _) => Some("Outline delete"),
            (K::Char('b'), M::CONTROL) => Some("Outline bookmark"),
            (K::Char('r'), M::CONTROL) => Some("Outline reference"),
            _ => None,
        };
        if let Some(command) = command {
            self.outline_command(command);
            return;
        }
        let e = &mut self.tabs[self.tab];
        e.outline.sync(&e.doc);
        if key.modifiers.contains(M::CONTROL) {
            let text = e
                .outline
                .index()
                .map(|i| e.outline.model.as_ref().unwrap().nodes[i].text.clone());
            if matches!(key.code, K::Char('c' | 'x' | 'v')) {
                if matches!(key.code, K::Char('c' | 'x')) {
                    self.clipboard = text.unwrap_or_default();
                }
                if key.code != K::Char('c') {
                    let value = if key.code == K::Char('v') {
                        self.clipboard.clone()
                    } else {
                        String::new()
                    };
                    if let Err(error) = self.outline_submit(Prompt::OutlineEdit, value) {
                        self.status = format!("{error:#}");
                    }
                }
                return;
            }
            if !can_edit(e) {
                self.status = "Outline is read-only or busy".into();
                return;
            }
            match key.code {
                K::Char('z') => {
                    e.doc.undo();
                }
                K::Char('y') => {
                    e.doc.redo();
                }
                _ => {}
            }
            e.outline.sync(&e.doc);
            return;
        }
        let s = &mut e.outline;
        let visible = s.visible();
        let pos = visible
            .iter()
            .position(|&i| Some(i) == s.index())
            .unwrap_or(0);
        let next = match key.code {
            K::Up => pos.saturating_sub(1),
            K::Down => pos + 1,
            K::PageUp => pos.saturating_sub(10),
            K::PageDown => pos + 10,
            K::Home => 0,
            K::End => visible.len().saturating_sub(1),
            _ => return,
        };
        if let Some(&i) = visible.get(next.min(visible.len().saturating_sub(1))) {
            s.selected = Some(s.model.as_ref().unwrap().nodes[i].id.clone());
        }
    }
}
fn commit(e: &mut crate::editor::Editor, model: Model) -> Result<()> {
    let text = model.json()?;
    let normalized_len = text.len()
        + text.bytes().filter(|&c| c == b'\n').count() * (e.doc.eol().as_bytes().len() - 1);
    ensure!(
        normalized_len <= LIMIT,
        "Outline exceeds 1 MiB after newline conversion; change was not applied"
    );
    let previous = e.doc.snapshot().read(0..e.doc.snapshot().len());
    let normalized = text.replace(
        '\n',
        std::str::from_utf8(e.doc.eol().as_bytes()).expect("ASCII newline"),
    );
    if previous != normalized.as_bytes() {
        e.doc.select_all();
        ensure!(e.doc.insert_text(&text, false), "Outline edit was refused");
    }
    e.outline.version = None;
    e.outline.sync(&e.doc);
    Ok(())
}
fn can_edit(e: &crate::editor::Editor) -> bool {
    !e.doc.is_read_only()
        && !e.doc.is_busy()
        && !e
            .path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .is_some_and(|m| m.permissions().readonly())
}
fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
fn clipped(text: &str, width: u16) -> String {
    text.chars()
        .take(usize::from(width))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
pub(crate) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let e = &mut app.tabs[app.tab];
    e.outline.sync(&e.doc);
    let s = &mut e.outline;
    let Some(m) = &s.model else {
        frame.render_widget(
            Paragraph::new(format!(
                "{}\nOutline source: repair JSON without losing the original",
                s.error
            )),
            area,
        );
        return;
    };
    let visible = s.visible();
    let selected = s.index();
    let area = if area.width >= 96 {
        let width = 28;
        let side = Rect::new(area.right() - width, area.y, width, area.height);
        let border = ratatui::widgets::Block::bordered().title("Bookmarks / backlinks");
        let inner = border.inner(side);
        frame.render_widget(border, side);
        for (row, (i, n)) in m
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.bookmark || s.selected.as_ref().is_some_and(|id| n.refs.contains(id))
            })
            .take(inner.height as usize)
            .enumerate()
        {
            let rect = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
            frame.render_widget(
                Paragraph::new(format!(
                    "{}{} {}",
                    if n.bookmark { "★" } else { "" },
                    if s.selected.as_ref().is_some_and(|id| n.refs.contains(id)) {
                        "↶"
                    } else {
                        ""
                    },
                    clipped(&n.text, inner.width)
                )),
                rect,
            );
            app.mouse_actions
                .push((rect, MouseAction::Outline(i, "zoom")));
        }
        Rect::new(area.x, area.y, area.width - width, area.height)
    } else {
        area
    };
    let at = visible
        .iter()
        .position(|i| Some(*i) == selected)
        .unwrap_or(0);
    let height = area.height.saturating_sub(2) as usize;
    if at < s.top {
        s.top = at;
    } else if at >= s.top + height {
        s.top = at.saturating_sub(height.saturating_sub(1));
    }
    let mut path = vec!["All".to_owned()];
    if let Some(id) = &s.zoom
        && let Some(i) = m.nodes.iter().position(|n| &n.id == id)
    {
        let mut ancestors = vec![i];
        let mut depth = m.nodes[i].depth;
        for j in (0..i).rev() {
            if m.nodes[j].depth < depth {
                ancestors.push(j);
                depth = m.nodes[j].depth;
            }
        }
        path.extend(ancestors.into_iter().rev().map(|j| safe(&m.nodes[j].text)));
    }
    frame.render_widget(
        Paragraph::new(path.join(" > ")).style(Style::default().fg(Color::Cyan)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let numbers = m.numbers();
    let base = s
        .zoom
        .as_ref()
        .and_then(|id| m.nodes.iter().find(|n| &n.id == id))
        .map_or(0, |n| n.depth);
    for (row, &i) in visible.iter().skip(s.top).take(height).enumerate() {
        let n = &m.nodes[i];
        let indent = (n.depth - base)
            .saturating_mul(2)
            .min(usize::from(area.width) / 3);
        let fold = if i + 1 < m.nodes.len() && m.nodes[i + 1].depth > n.depth {
            if n.collapsed { "▸" } else { "▾" }
        } else {
            "·"
        };
        let bullet = if m.list_style.as_deref() == Some("numbered") {
            format!("{}.", numbers[i])
        } else {
            "●".into()
        };
        let mut refs = String::new();
        let mut remaining = usize::from(area.width);
        for id in &n.refs {
            if remaining < 3 {
                break;
            }
            let text = m
                .nodes
                .iter()
                .find(|n| &n.id == id)
                .map_or("[missing]", |n| n.text.as_str());
            let label = clipped(text, (remaining - 2) as u16);
            remaining -= label.chars().count() + 2;
            refs.push_str(" ↗");
            refs.push_str(&label);
        }
        let line = format!(
            "{}{} {} {}{} [{}]{}",
            " ".repeat(indent),
            fold,
            bullet,
            if n.bookmark { "★ " } else { "" },
            clipped(&n.text, area.width),
            clipped(&n.id, area.width),
            safe(&refs)
        );
        let rect = Rect::new(area.x, area.y + 1 + row as u16, area.width, 1);
        frame.render_widget(
            Paragraph::new(line).style(if Some(i) == selected {
                Style::default().bg(Color::Blue)
            } else {
                Style::default()
            }),
            rect,
        );
        app.mouse_actions.push((rect, MouseAction::Outline(i, "")));
        let x = rect.x + indent as u16;
        if x < rect.right() {
            app.mouse_actions
                .push((Rect::new(x, rect.y, 1, 1), MouseAction::Outline(i, "fold")));
        }
        if x + 2 < rect.right() {
            app.mouse_actions.push((
                Rect::new(
                    x + 2,
                    rect.y,
                    ((unicode_width::UnicodeWidthStr::width(bullet.as_str()) + 1) as u16)
                        .min(rect.right() - x - 2),
                    1,
                ),
                MouseAction::Outline(i, "zoom"),
            ));
        }
    }
    if area.height > 1 {
        let info = selected
            .map(|i| {
                let n = &m.nodes[i];
                let backlinks = m
                    .nodes
                    .iter()
                    .filter(|n| n.refs.contains(&m.nodes[i].id))
                    .map(|n| n.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "ID: {} | refs: {} | backlinks: {}",
                    n.id,
                    n.refs.join(", "),
                    backlinks
                )
            })
            .unwrap_or_else(|| "Enter: add first item".into());
        frame.render_widget(
            Paragraph::new(safe(&info)),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
}
pub(crate) fn select(app: &mut App, index: usize, op: &'static str) {
    let e = &mut app.tabs[app.tab];
    e.outline.sync(&e.doc);
    if let Some(n) = e.outline.model.as_ref().and_then(|m| m.nodes.get(index)) {
        e.outline.selected = Some(n.id.clone());
        app.explorer = false;
    }
    if !op.is_empty() {
        app.outline_command(&format!("Outline {op}"));
    }
}

fn urls(mut text: &str) -> Vec<String> {
    let mut result = vec![];
    while let Some((start, _)) = text.char_indices().find(|(i, _)| {
        text[*i..]
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
            || text[*i..]
                .get(..8)
                .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
    }) {
        text = &text[start..];
        let end = text
            .find(|c: char| c.is_whitespace() || "<>\"'「」『』【】".contains(c))
            .unwrap_or(text.len());
        let mut candidate = text[..end]
            .trim_end_matches(['.', ',', '!', '?', ';', ':', '、', '。', '！', '？', '）']);
        let mut excess = candidate
            .matches(')')
            .count()
            .saturating_sub(candidate.matches('(').count());
        while candidate.ends_with(')') && excess > 0 {
            candidate = &candidate[..candidate.len() - 1];
            excess -= 1;
        }
        if reqwest::Url::parse(candidate)
            .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
            && !result.iter().any(|s| s == candidate)
        {
            result.push(candidate.to_owned());
        }
        text = &text[end..];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXAMPLE: &str = include_str!("../demo/outline-notes.yyoutline");
    fn ids(m: &Model) -> Vec<&str> {
        m.nodes.iter().map(|n| n.id.as_str()).collect()
    }
    fn app() -> App {
        let mut app = App::new(std::env::current_dir().unwrap());
        app.tabs[0].doc.insert_text(EXAMPLE, false);
        app.outline_action("editor").unwrap();
        app
    }
    #[test]
    fn json_schema_roundtrip_extensions_and_invalid_inputs() -> Result<()> {
        let mut model = Model::parse(EXAMPLE)?;
        model.move_branch(1, "down");
        assert_eq!(ids(&model), ["root", "tasks", "idea", "detail"]);
        let encoded = model.json()?;
        let value: Value = serde_json::from_str(&encoded)?;
        assert_eq!(value["extensions"]["example"], true);
        assert_eq!(value["nodes"][2]["colorHint"], "blue");
        assert_eq!(Model::parse(&encoded)?.nodes[1].refs, ["idea"]);
        let mut valid: Value = serde_json::from_str(EXAMPLE)?;
        valid["version"] = serde_json::json!(1.0);
        valid["nodes"][0]["depth"] = serde_json::json!(0.0);
        Model::parse(&valid.to_string())?;
        for change in [
            "duplicate",
            "depth",
            "fraction",
            "style",
            "missing",
            "version",
        ] {
            let mut v: Value = serde_json::from_str(EXAMPLE)?;
            match change {
                "duplicate" => v["nodes"][1]["id"] = v["nodes"][0]["id"].clone(),
                "depth" => v["nodes"][1]["depth"] = serde_json::json!(9),
                "fraction" => v["nodes"][1]["depth"] = serde_json::json!(0.5),
                "style" => v["listStyle"] = Value::Null,
                "missing" => {
                    v["nodes"][0].as_object_mut().unwrap().remove("refs");
                }
                _ => v["version"] = serde_json::json!(2),
            }
            assert!(Model::parse(&v.to_string()).is_err(), "{change}");
        }
        if let Some(path) = std::env::var_os("YYTE_OUTLINE_INTEROP_OUTPUT") {
            std::fs::write(path, encoded)?;
        }
        if let Some(path) = std::env::var_os("YYTE_OUTLINE_INTEROP_INPUT") {
            let path = std::path::PathBuf::from(path);
            let model = Model::parse(&std::fs::read_to_string(&path)?)?;
            assert_eq!(
                model.markdown()?,
                std::fs::read_to_string(path.with_extension("md"))?
            );
            assert_eq!(
                serde_json::to_value(&model)?,
                serde_json::from_str::<Value>(&std::fs::read_to_string(path)?)?
            );
        }
        Ok(())
    }
    #[test]
    fn branches_keep_siblings_references_ids_and_unbounded_depth() -> Result<()> {
        let mut m = Model::parse(EXAMPLE)?;
        m.move_branch(1, "outdent");
        assert_eq!(ids(&m), ["root", "tasks", "idea", "detail"]);
        assert_eq!(
            m.nodes.iter().map(|n| n.depth).collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        m.move_branch(2, "indent");
        assert_eq!(m.nodes[2].depth, 1);
        assert_eq!(m.nodes[3].depth, 2);
        m.move_branch(2, "up");
        assert_eq!(ids(&m), ["root", "idea", "detail", "tasks"]);
        m.nodes.drain(1..3);
        assert_eq!(m.nodes[1].refs, ["idea"]);
        let mut m = Model::parse("")?;
        for i in 0..3000 {
            m.nodes.push(Node {
                id: format!("deep{i}"),
                depth: i,
                text: "x".into(),
                bookmark: false,
                collapsed: false,
                refs: vec![],
                extra: Map::new(),
            });
        }
        let parsed = Model::parse(&m.json()?)?;
        assert_eq!(parsed.end(0), 3000);
        assert!(parsed.markdown().is_err()); // bounded output despite deep nesting
        Ok(())
    }
    #[test]
    fn outline_operations_share_document_undo_and_keep_failed_edits_atomic() -> Result<()> {
        let mut app = app();
        let original_version = app.tabs[0].doc.version();
        app.outline_action("indent")?;
        assert_eq!(original_version, app.tabs[0].doc.version());
        app.outline_key(KeyEvent::new(K::Down, M::NONE));
        app.outline_action("bookmark")?;
        assert!(app.tabs[0].outline.model.as_ref().unwrap().nodes[1].bookmark);
        app.command("Undo");
        assert!(app.view == View::Outline);
        assert!(!app.tabs[0].outline.model.as_ref().unwrap().nodes[1].bookmark);
        app.command("Redo");
        assert!(app.tabs[0].outline.model.as_ref().unwrap().nodes[1].bookmark);
        let before = app.tabs[0].doc.snapshot().clone();
        assert!(
            app.outline_submit(Prompt::OutlineEdit, "x".repeat(LIMIT))
                .is_err()
        );
        assert_eq!(
            before.read(0..before.len()),
            app.tabs[0].doc.snapshot().read(0..before.len())
        );
        app.outline_action("fold")?;
        assert_eq!(app.tabs[0].outline.visible(), [0, 1, 3]);
        app.outline_action("zoom")?;
        assert_eq!(app.tabs[0].outline.visible(), [1, 2]);
        app.outline_action("add")?;
        let s = &app.tabs[0].outline;
        assert_eq!(s.model.as_ref().unwrap().nodes[s.index().unwrap()].depth, 2);
        app.outline_action("all")?;
        app.tabs[0].outline.selected = Some("idea".into());
        app.outline_submit(Prompt::OutlineDelete, "yes".into())?;
        let m = app.tabs[0].outline.model.as_ref().unwrap();
        assert!(m.nodes.iter().any(|n| n.refs.contains(&"idea".into())));
        app.outline_key(KeyEvent::new(K::Char('z'), M::CONTROL));
        assert!(
            app.tabs[0]
                .outline
                .model
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .any(|n| n.id == "idea")
        );
        Ok(())
    }
    #[test]
    fn reference_navigation_keeps_history_and_large_labels_are_bounded() -> Result<()> {
        let mut app = app();
        app.tabs[0].outline.selected = Some("tasks".into());
        let version = app.tabs[0].doc.version();
        app.outline_action("follow reference")?;
        assert_eq!(app.input, "idea");
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert_eq!(app.tabs[0].outline.selected.as_deref(), Some("idea"));
        app.outline_action("backlinks")?;
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert_eq!(app.tabs[0].outline.selected.as_deref(), Some("tasks"));
        assert_eq!(app.tabs[0].doc.version(), version);
        let mut model = app.tabs[0].outline.model.clone().unwrap();
        model.nodes[0].refs = vec!["idea".into(); 3000];
        model.nodes[1].text = "長い文章".repeat(3000);
        commit(&mut app.tabs[0], model)?;
        app.outline_action("all")?;
        app.tabs[0].outline.selected = Some("root".into());
        for (w, h) in [(1, 1), (12, 5), (140, 24)] {
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h))?;
            terminal.draw(|f| draw(f, &mut app, f.area()))?;
        }
        assert_eq!(
            urls(&format!("https://example.com/x{}", ")".repeat(5000))),
            ["https://example.com/x"]
        );
        app.tabs.push(crate::editor::Editor::empty());
        app.select_tab(1);
        app.select_tab(0);
        assert!(app.view == View::Outline);
        Ok(())
    }
    #[test]
    fn empty_outline_add_edit_cancel_and_undo_use_normal_key_dispatch() -> Result<()> {
        let mut app = App::new(std::env::current_dir()?);
        app.outline_action("editor")?;
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert_eq!(app.tabs[0].outline.model.as_ref().unwrap().nodes.len(), 1);
        app.key(KeyEvent::new(K::F(2), M::NONE));
        assert!(matches!(app.prompt, Some(Prompt::OutlineEdit)));
        app.input = "first item".into();
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert_eq!(
            app.tabs[0].outline.model.as_ref().unwrap().nodes[0].text,
            "first item"
        );
        app.key(KeyEvent::new(K::F(2), M::NONE));
        app.input = "cancel".into();
        app.key(KeyEvent::new(K::Esc, M::NONE));
        assert_eq!(
            app.tabs[0].outline.model.as_ref().unwrap().nodes[0].text,
            "first item"
        );
        app.key(KeyEvent::new(K::Char('z'), M::CONTROL));
        assert_eq!(
            app.tabs[0].outline.model.as_ref().unwrap().nodes[0].text,
            ""
        );
        app.key(KeyEvent::new(K::Char('z'), M::CONTROL));
        assert!(app.tabs[0].outline.model.as_ref().unwrap().nodes.is_empty());
        Ok(())
    }
    #[test]
    fn crlf_size_limit_is_checked_before_changing_document() -> Result<()> {
        let mut app = app();
        app.tabs[0].doc.convert_eol(yy_core::Eol::CrLf)?;
        let mut model = Model::parse(EXAMPLE)?;
        commit(&mut app.tabs[0], model.clone())?;
        let unchanged = app.tabs[0].doc.version();
        commit(&mut app.tabs[0], model.clone())?;
        assert_eq!(app.tabs[0].doc.version(), unchanged);
        model.nodes[1].text.clear();
        let overhead = model.json()?.len();
        model.nodes[1].text = "x".repeat(LIMIT - overhead);
        assert_eq!(model.json()?.len(), LIMIT);
        let before = app.tabs[0].doc.snapshot().clone();
        let version = app.tabs[0].doc.version();
        assert!(commit(&mut app.tabs[0], model).is_err());
        assert_eq!(app.tabs[0].doc.version(), version);
        assert_eq!(
            app.tabs[0].doc.snapshot().read(0..before.len()),
            before.read(0..before.len())
        );
        Ok(())
    }
    #[test]
    fn pending_outline_input_never_edits_a_different_tab_or_version() -> Result<()> {
        let mut app = app();
        let original = app.tabs[0].doc.snapshot().clone();
        app.outline_action("edit")?;
        app.tabs[0].outline.selected = Some("idea".into());
        app.input = "wrong item".into();
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.status.contains("changed while input"));
        assert_eq!(
            app.tabs[0].doc.snapshot().read(0..original.len()),
            original.read(0..original.len())
        );
        app.outline_action("edit")?;
        app.tabs[0].doc.insert_text(" ", false);
        let version = app.tabs[0].doc.version();
        app.input = "stale edit".into();
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.status.contains("changed while input"));
        assert_eq!(app.tabs[0].doc.version(), version);
        app.outline_action("edit")?;
        app.tabs.push(crate::editor::Editor::empty());
        app.select_tab(1);
        app.input = "wrong tab".into();
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.status.contains("changed while input"));
        assert_eq!(app.tabs[1].doc.snapshot().len(), 0);
        Ok(())
    }
    #[test]
    fn malformed_source_readonly_export_and_narrow_rendering() -> Result<()> {
        let mut readonly = app();
        let mut app = app();
        let dir = tempfile::tempdir()?;
        let read_path = dir.path().join("readonly.yyoutline");
        std::fs::write(&read_path, EXAMPLE)?;
        let permissions = std::fs::metadata(&read_path)?.permissions();
        let mut read_permissions = permissions.clone();
        read_permissions.set_readonly(true);
        std::fs::set_permissions(&read_path, read_permissions)?;
        readonly.tabs[0].path = Some(read_path.clone());
        let refused = readonly.outline_action("bookmark");
        std::fs::set_permissions(read_path, permissions)?;
        assert!(refused.is_err());
        app.root = dir.path().to_owned();
        app.outline_submit(Prompt::OutlineExport, "notes.md".into())?;
        assert_eq!(
            std::fs::read_to_string(dir.path().join("notes.md"))?,
            app.tabs[0].outline.model.as_ref().unwrap().markdown()?
        );
        assert!(
            app.outline_submit(Prompt::OutlineExport, "notes.md".into())
                .is_err()
        );
        app.outline_action("source")?;
        app.tabs[0].doc.select_all();
        app.tabs[0].doc.insert_text("invalid JSON", false);
        let version = app.tabs[0].doc.version();
        assert!(app.outline_action("editor").is_err());
        assert_eq!(version, app.tabs[0].doc.version());
        for (w, h) in [(1, 1), (12, 5), (140, 24)] {
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h))?;
            terminal.draw(|f| draw(f, &mut app, f.area()))?;
        }
        assert_eq!(
            urls("「HTTPS://example.com/a_(b)」。 https://example.com/c). javascript:evil"),
            ["HTTPS://example.com/a_(b)", "https://example.com/c"]
        );
        Ok(())
    }
}
