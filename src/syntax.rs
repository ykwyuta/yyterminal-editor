//! Shared YY syntax definitions and terminal token colors; no external highlighting process.
use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, mpsc},
};
use yy_syntax::{LineState, Registry, Syntax};

pub const CONTEXT_BYTES: u64 = 256 * 1024;
pub const VIEW_BYTES: u64 = 256 * 1024;
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::builtin)
}
pub fn languages() -> Vec<(String, String)> {
    registry().list()
}
pub fn language(name: &str) -> Option<Arc<Syntax>> {
    let name = name
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(['{', '}']);
    let name = name
        .strip_prefix('.')
        .or_else(|| name.strip_prefix("language-"))
        .unwrap_or(name);
    registry().get(name).ok().or_else(|| {
        registry()
            .detect(Some(Path::new(&format!("file.{name}"))), b"")
            .and_then(|id| registry().get(&id).ok())
    })
}
pub fn valid_language(name: &str) -> bool {
    matches!(name, "auto" | "none" | "plain" | "text") || language(name).is_some()
}
pub fn token_style(token: &str) -> Style {
    let base = token.split('.').next().unwrap_or(token);
    match base {
        "keyword" => Style::default().fg(Color::Rgb(198, 120, 221)),
        "string" => Style::default().fg(Color::Rgb(152, 195, 121)),
        "comment" => Style::default()
            .fg(Color::Rgb(127, 135, 145))
            .add_modifier(Modifier::ITALIC),
        "number" | "constant" => Style::default().fg(Color::Rgb(209, 154, 102)),
        "type" | "tag" => Style::default().fg(Color::Rgb(86, 182, 194)),
        "function" | "attribute" => Style::default().fg(Color::Rgb(97, 175, 239)),
        "variable" | "label" => Style::default().fg(Color::Rgb(224, 108, 117)),
        "heading" => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        "strong" => Style::default().add_modifier(Modifier::BOLD),
        "emphasis" => Style::default().add_modifier(Modifier::ITALIC),
        "code" => Style::default().fg(Color::Yellow),
        "link" => Style::default()
            .fg(Color::Rgb(97, 175, 239))
            .add_modifier(Modifier::UNDERLINED),
        _ => Style::default(),
    }
}
#[derive(Clone)]
struct Token {
    range: Range<u64>,
    style: Style,
}
fn floor(bytes: &[u8], mut at: usize) -> usize {
    while at > 0 && at < bytes.len() && bytes[at] & 0xc0 == 0x80 {
        at -= 1;
    }
    at
}
fn tokens(syntax: &Syntax, state: &LineState, bytes: &[u8], base: u64) -> (LineState, Vec<Token>) {
    let mut spans = vec![];
    let next = syntax.highlight_line(state, bytes, &mut spans);
    let spans = spans
        .into_iter()
        .filter_map(|span| {
            let start = floor(bytes, (span.range.start as usize).min(bytes.len()));
            let end = floor(bytes, (span.range.end as usize).min(bytes.len()));
            (start < end).then(|| Token {
                range: base + start as u64..base + end as u64,
                style: token_style(syntax.token_name(span.token)),
            })
        })
        .collect();
    (next, spans)
}
fn display(bytes: &[u8]) -> String {
    let mut result = String::new();
    for ch in String::from_utf8_lossy(bytes).chars() {
        match ch {
            '\r' | '\n' => {}
            '\t' => result.push_str("    "),
            c if c.is_control() => result.push('�'),
            c => result.push(c),
        }
    }
    result
}
fn styled(bytes: &[u8], base: u64, tokens: &[Token], selection: Range<u64>) -> Line<'static> {
    let end = base + bytes.len() as u64;
    let mut boundaries = vec![base, end];
    for token in tokens {
        if token.range.start < end && token.range.end > base {
            boundaries.push(token.range.start.max(base));
            boundaries.push(token.range.end.min(end));
        }
    }
    if selection.start < end && selection.end > base {
        boundaries.push(selection.start.max(base));
        boundaries.push(selection.end.min(end));
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut spans = vec![];
    let mut token_index = 0;
    for pair in boundaries.windows(2) {
        let (start, stop) = (pair[0], pair[1]);
        while tokens
            .get(token_index)
            .is_some_and(|token| token.range.end <= start)
        {
            token_index += 1;
        }
        let mut style = tokens
            .get(token_index)
            .filter(|token| token.range.start <= start && token.range.end >= stop)
            .map_or(Style::default(), |t| t.style);
        if start >= selection.start && start < selection.end {
            style = style.bg(Color::Blue);
        }
        let text = display(&bytes[(start - base) as usize..(stop - base) as usize]);
        if !text.is_empty() {
            crate::preview_layout::append(&mut spans, &text, style);
        }
    }
    Line::from(spans)
}
#[derive(Clone)]
struct Fence {
    marker: u8,
    length: usize,
    syntax: Option<Arc<Syntax>>,
    state: LineState,
}
/// A code block owns its lexical state, so comments and strings cross physical lines.
#[derive(Clone)]
pub struct CodeHighlighter {
    syntax: Option<Arc<Syntax>>,
    state: LineState,
    fence: Option<Fence>,
}
impl CodeHighlighter {
    pub fn new(name: &str) -> Self {
        Self::with_syntax(language(name))
    }
    fn with_syntax(syntax: Option<Arc<Syntax>>) -> Self {
        Self {
            syntax,
            state: LineState::default(),
            fence: None,
        }
    }
    fn highlight(&mut self, line: &[u8], base: u64) -> Vec<Token> {
        if self
            .syntax
            .as_ref()
            .is_some_and(|syntax| syntax.id == "markdown")
        {
            let indent = line.iter().take_while(|b| **b == b' ').count();
            let trimmed = &line[indent.min(line.len())..];
            if let Some(fence) = &mut self.fence {
                let count = trimmed.iter().take_while(|b| **b == fence.marker).count();
                if indent <= 3
                    && count >= fence.length
                    && trimmed[count..].iter().all(u8::is_ascii_whitespace)
                {
                    self.fence = None;
                    self.state = LineState::default();
                    return vec![Token {
                        range: base..base + line.len() as u64,
                        style: Style::default().fg(Color::DarkGray),
                    }];
                }
                if let Some(syntax) = &fence.syntax {
                    let (state, ranges) = tokens(syntax, &fence.state, line, base);
                    fence.state = state;
                    return ranges;
                }
                return vec![];
            }
            if self.state.is_initial()
                && indent <= 3
                && matches!(trimmed.first(), Some(b'`' | b'~'))
            {
                let marker = trimmed[0];
                let count = trimmed.iter().take_while(|b| **b == marker).count();
                if count >= 3 {
                    let info = String::from_utf8_lossy(&trimmed[count..]);
                    // Backtick info strings may not themselves contain backticks.
                    if marker != b'`' || !info.contains('`') {
                        self.fence = Some(Fence {
                            marker,
                            length: count,
                            syntax: language(info.trim()),
                            state: LineState::default(),
                        });
                        return vec![Token {
                            range: base..base + line.len() as u64,
                            style: Style::default().fg(Color::DarkGray),
                        }];
                    }
                }
            }
        }
        if let Some(syntax) = &self.syntax {
            let (state, ranges) = tokens(syntax, &self.state, line, base);
            self.state = state;
            ranges
        } else {
            vec![]
        }
    }
    pub fn line(&mut self, line: &str) -> Line<'static> {
        let ranges = self.highlight(line.as_bytes(), 0);
        styled(line.as_bytes(), 0, &ranges, 0..0)
    }
}
#[derive(Clone, PartialEq)]
struct Key {
    version: yy_core::Version,
    path: Option<PathBuf>,
    language: Option<String>,
    start: u64,
    end: u64,
}
#[derive(Clone)]
struct Checkpoint {
    offset: u64,
    highlighter: CodeHighlighter,
}
struct Highlighted {
    key: Key,
    syntax: Option<String>,
    tokens: Vec<Token>,
    checkpoints: Vec<Checkpoint>,
}
/// One worker per active request, with a bounded input window and no document-wide scan.
pub struct EditorHighlighter {
    tx: mpsc::Sender<Highlighted>,
    rx: mpsc::Receiver<Highlighted>,
    pending: bool,
    ready: Option<Highlighted>,
}
impl Default for EditorHighlighter {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            pending: false,
            ready: None,
        }
    }
}
impl EditorHighlighter {
    pub fn poll(&mut self) -> bool {
        if let Ok(result) = self.rx.try_recv() {
            self.pending = false;
            self.ready = Some(result);
            true
        } else {
            false
        }
    }
    pub fn name(&self) -> Option<&str> {
        self.ready.as_ref().and_then(|r| r.syntax.as_deref())
    }
    pub fn lines<'a>(
        &mut self,
        snapshot: &yy_buffer::Snapshot,
        version: yy_core::Version,
        path: Option<&Path>,
        manual: Option<&str>,
        rows: &'a [(u64, u64, String)],
        selection: Range<u64>,
    ) -> Vec<Line<'a>> {
        let key = Key {
            version,
            path: path.map(Path::to_owned),
            language: manual.map(str::to_owned),
            start: rows.first().map_or(0, |r| r.0),
            end: rows.last().map_or(0, |r| r.1),
        };
        let matches_document = |ready: &Highlighted| {
            ready.key.version == key.version
                && ready.key.path == key.path
                && ready.key.language == key.language
        };
        let current = self.ready.as_ref().is_some_and(|ready| {
            matches_document(ready) && ready.key.start <= key.start && ready.key.end >= key.end
        });
        if !current && !self.pending {
            self.pending = true;
            let mut request = key.clone();
            request.start = request.start.saturating_sub(32 * 1024);
            request.end = request
                .end
                .saturating_add(64 * 1024)
                .min(request.start.saturating_add(VIEW_BYTES))
                .min(snapshot.len());
            let seed = self
                .ready
                .as_ref()
                .filter(|ready| matches_document(ready))
                .and_then(|ready| {
                    ready.checkpoints.iter().rev().find(|p| {
                        p.offset <= request.start && request.start - p.offset <= CONTEXT_BYTES
                    })
                })
                .cloned();
            let snapshot = snapshot.clone();
            let key = request;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(highlight_window(snapshot, key, seed));
            });
        }
        let tokens = self
            .ready
            .as_ref()
            .filter(|ready| matches_document(ready))
            .map_or(&[][..], |ready| ready.tokens.as_slice());
        if tokens.is_empty() && selection.is_empty() {
            return rows.iter().map(|r| Line::raw(r.2.as_str())).collect();
        }
        rows.iter()
            .map(|(start, end, _)| {
                let first = tokens.partition_point(|t| t.range.end <= *start);
                let last = tokens.partition_point(|t| t.range.start < *end);
                styled(
                    &snapshot.read(*start..*end),
                    *start,
                    &tokens[first..last],
                    selection.clone(),
                )
            })
            .collect()
    }
}
fn highlight_window(
    snapshot: yy_buffer::Snapshot,
    key: Key,
    seed: Option<Checkpoint>,
) -> Highlighted {
    let head = snapshot.read(0..snapshot.len().min(4096));
    let syntax = match key.language.as_deref() {
        Some("none" | "plain" | "text") => None,
        Some(name) if name != "auto" => language(name),
        _ => registry()
            .detect(key.path.as_deref(), &head)
            .and_then(|id| language(&id)),
    };
    let mut result = Highlighted {
        key,
        syntax: syntax.as_ref().map(|s| s.name.clone()),
        tokens: vec![],
        checkpoints: vec![],
    };
    let Some(syntax) = syntax else {
        return result;
    };
    let first = seed.as_ref().map_or_else(
        || result.key.start.saturating_sub(CONTEXT_BYTES),
        |p| p.offset,
    );
    let end = result
        .key
        .end
        .min(result.key.start.saturating_add(VIEW_BYTES))
        .min(snapshot.len());
    let bytes = snapshot.read(first..end);
    // Discard an incomplete leading line when possible, retaining the full visible line prefix.
    let skip = if first > 0 && seed.is_none() {
        bytes
            .iter()
            .take((result.key.start - first) as usize)
            .position(|b| *b == b'\n')
            .map_or(0, |i| i + 1)
    } else {
        0
    };
    let mut offset = skip;
    let mut highlighter = seed.map_or_else(
        || CodeHighlighter::with_syntax(Some(syntax)),
        |p| p.highlighter,
    );
    let mut last_checkpoint = first + offset as u64;
    result.checkpoints.push(Checkpoint {
        offset: last_checkpoint,
        highlighter: highlighter.clone(),
    });
    for line in bytes[skip..].split_inclusive(|b| *b == b'\n') {
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        let ranges = highlighter.highlight(content, first + offset as u64);
        result.tokens.extend(
            ranges
                .into_iter()
                .filter(|t| t.range.end > result.key.start),
        );
        offset += line.len();
        if line.ends_with(b"\n") && first + offset as u64 - last_checkpoint >= 64 * 1024 {
            last_checkpoint = first + offset as u64;
            result.checkpoints.push(Checkpoint {
                offset: last_checkpoint,
                highlighter: highlighter.clone(),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn has(line: &Line<'_>, text: &str, token: &str) -> bool {
        line.spans
            .iter()
            .any(|span| span.content.contains(text) && span.style.fg == token_style(token).fg)
    }
    #[test]
    fn nearby_scroll_reuses_tokens_without_plain_text_flash_or_worker_restart() {
        let source = "let value = 42; // note\n".repeat(2000);
        let doc = yy_core::Document::from_text(&source);
        let mut h = EditorHighlighter::default();
        let row = |at: usize| vec![(at as u64, (at + 23) as u64, source[at..at + 23].to_owned())];
        h.lines(
            doc.snapshot(),
            doc.version(),
            None,
            Some("rust"),
            &row(0),
            0..0,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !h.poll() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        for at in [23, 46, 230, 0] {
            let rows = row(at);
            let lines = h.lines(
                doc.snapshot(),
                doc.version(),
                None,
                Some("rust"),
                &rows,
                0..0,
            );
            assert!(has(&lines[0], "let", "keyword"));
            assert!(
                !h.pending,
                "Scrolling inside cached coverage spawned a worker"
            );
        }
    }
    #[test]
    fn builtin_detection_aliases_and_distinct_token_colors() {
        assert_eq!(
            registry()
                .detect(Some(Path::new("main.rs")), b"")
                .as_deref(),
            Some("rust")
        );
        assert_eq!(
            registry()
                .detect(None, b"#!/usr/bin/env python\n")
                .as_deref(),
            Some("python")
        );
        for (name, source, keyword) in [
            ("rs", "let count = 42; // note", "let"),
            ("sql", "SELECT 'hello', 42 -- note", "SELECT"),
            ("python", "return 'hello' # note", "return"),
            ("js", "const value = 42; // note", "const"),
        ] {
            let mut h = CodeHighlighter::new(name);
            let line = h.line(source);
            assert_eq!(line.to_string(), source);
            assert!(has(&line, keyword, "keyword"), "{name}: {line:?}");
            assert!(has(&line, "note", "comment"), "{name}: {line:?}");
        }
        assert!(languages().len() >= 38);
        let mut h = CodeHighlighter::new("unknown-example-language");
        let line = h.line("<script>let value=42</script>");
        assert!(line.spans.iter().all(|s| s.style == Style::default()));
        assert_eq!(line.to_string(), "<script>let value=42</script>");
    }
    #[test]
    fn multiline_comments_strings_and_markdown_embedded_code() {
        let mut h = CodeHighlighter::new("rust");
        h.line("/* opened");
        assert!(has(&h.line("let comment = 42;"), "let comment", "comment"));
        assert!(has(&h.line("*/ let code = 42;"), "let", "keyword"));
        let mut md = CodeHighlighter::new("markdown");
        assert!(
            md.line("# Heading")
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::BOLD))
        );
        md.line("````rust");
        assert!(has(&md.line("let text = \"日本語\";"), "日本語", "string"));
        md.line("```"); // A shorter fence does not close the block.
        assert!(has(&md.line("let count = 1;"), "let", "keyword"));
        md.line("````");
        assert!(!has(&md.line("let count = 1;"), "let", "keyword"));
        md.line("~~~python");
        assert!(has(&md.line("return 42"), "return", "keyword"));
        md.line("~~~");
    }
    #[test]
    fn token_boundaries_selection_tabs_and_unicode_preserve_display() {
        let text = "let value = \"日本語\";\t// note\r\n";
        let syntax = language("rust").unwrap();
        let (_, ranges) = tokens(&syntax, &LineState::default(), text.as_bytes(), 0);
        let line = styled(text.as_bytes(), 0, &ranges, 0..3);
        assert_eq!(line.to_string(), "let value = \"日本語\";    // note");
        assert!(line.spans.iter().any(|s| s.content == "let"
            && s.style.bg == Some(Color::Blue)
            && s.style.fg == token_style("keyword").fg));
        let source = "\"".to_owned() + &"x".repeat(yy_syntax::MAX_LINE_BYTES - 2) + "日本語\"";
        let result = CodeHighlighter::new("rust").line(&source);
        assert_eq!(result.to_string(), source);
        assert!(!result.to_string().contains('�'));
    }
    fn key(doc: &yy_core::Document, start: u64, end: u64) -> Key {
        Key {
            version: doc.version(),
            path: Some(PathBuf::from("sample.rs")),
            language: None,
            start,
            end,
        }
    }
    #[test]
    fn window_checkpoints_keep_long_comments_when_scrolling_and_edits_reset_state() {
        let source = "/* opened\n".to_owned()
            + &"still inside comment\n".repeat(40_000)
            + "*/ let final_value = 42;\n";
        let mut doc = yy_core::Document::from_text(&source);
        let mut previous = highlight_window(doc.snapshot().clone(), key(&doc, 0, 100_000), None);
        for start in (100_000..700_000).step_by(100_000) {
            let checkpoint = previous
                .checkpoints
                .iter()
                .rev()
                .find(|p| p.offset <= start)
                .cloned();
            previous = highlight_window(
                doc.snapshot().clone(),
                key(&doc, start, start + 100_000),
                checkpoint,
            );
            assert!(!previous.tokens.is_empty());
            assert!(
                previous
                    .tokens
                    .iter()
                    .all(|t| t.style.fg == token_style("comment").fg)
            );
            assert!(previous.checkpoints.len() < 12);
        }
        doc.select_all();
        doc.insert_text("let value = 42;", false);
        let changed = highlight_window(
            doc.snapshot().clone(),
            key(&doc, 0, doc.snapshot().len()),
            None,
        );
        assert!(
            changed
                .tokens
                .iter()
                .any(|t| t.style.fg == token_style("keyword").fg)
        );
    }
    #[test]
    fn asynchronous_results_never_color_a_different_document_version_or_language() {
        let mut doc = yy_core::Document::from_text("let value = 42;");
        let mut highlighter = EditorHighlighter::default();
        let rows = vec![(0, doc.snapshot().len(), "let value = 42;".into())];
        highlighter.lines(
            doc.snapshot(),
            doc.version(),
            Some(Path::new("sample.rs")),
            None,
            &rows,
            0..0,
        );
        doc.select_all();
        doc.insert_text("plain text now", false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !highlighter.poll() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let rows = vec![(0, doc.snapshot().len(), "plain text now".into())];
        let lines = highlighter.lines(
            doc.snapshot(),
            doc.version(),
            None,
            Some("none"),
            &rows,
            0..0,
        );
        assert_eq!(lines[0].to_string(), "plain text now");
        assert!(lines[0].spans.iter().all(|span| span.style.fg.is_none()));
        while !highlighter.poll() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(highlighter.name().is_none());
    }
}
