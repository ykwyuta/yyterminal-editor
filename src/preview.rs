//! Shared Markdown renderer for yyte and markdownViewer.
use anyhow::{Context, Result, ensure};
use base64::Engine;
use pulldown_cmark::{CodeBlockKind, Event, Options as MdOptions, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::Duration,
};
pub const MAX_MARKDOWN: usize = 128 * 1024 * 1024;
pub const MAX_WIDTH: u16 = 4096;
pub const MAX_IMAGES: usize = 256;
const MAX_IMAGE: u64 = 20 * 1024 * 1024;
pub const MAX_LINES: usize = 1_000_000;
#[derive(Clone)]
pub struct Options {
    pub base: PathBuf,
    pub width: u16,
    pub image_height: u16,
    pub color: bool,
    pub remote_images: bool,
}
impl Options {
    pub fn new(base: PathBuf, width: u16) -> Self {
        Self {
            base,
            width: width.clamp(1, MAX_WIDTH),
            image_height: 32,
            color: true,
            remote_images: false,
        }
    }
}
#[derive(Default)]
pub struct Rendered {
    pub width: usize,
    pub lines: Vec<Line<'static>>,
}
impl Rendered {
    pub fn plain(&self) -> String {
        let mut result = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                result.push('\n');
            }
            for span in &line.spans {
                result.push_str(&span.content);
            }
        }
        result
    }
}
/// Worker-side layout. UI frames only copy the visible viewport, never this entire output.
pub(crate) struct Output {
    lines: Vec<Line<'static>>,
    pub(crate) width: usize,
    limit: usize,
    clipped: bool,
    block_gap: bool,
}
impl Output {
    fn new(width: u16, limit: usize) -> Self {
        Self {
            lines: vec![],
            width: usize::from(width).max(1),
            limit,
            clipped: false,
            block_gap: false,
        }
    }
    pub(crate) fn full(&self) -> bool {
        self.lines.len() >= self.limit
    }
    pub(crate) fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.lines.len())
    }
    // Defer separators until another block is rendered: no leading/trailing
    // blank rows, and nested lists or adjacent block boundaries cannot double them.
    fn separate(&mut self) {
        self.block_gap = !self.lines.is_empty();
    }
    fn before_content(&mut self) {
        if std::mem::take(&mut self.block_gap)
            && self.lines.last().is_some_and(|line| {
                line.spans
                    .iter()
                    .any(|span| !span.content.trim().is_empty())
            })
        {
            if self.full() {
                self.clipped = true;
            } else {
                self.lines.push(Line::default());
            }
        }
    }
    fn push(&mut self, line: Line<'static>) {
        self.before_content();
        let (wrapped, clipped) = crate::preview_layout::wrap(line, self.width, self.remaining());
        self.lines.extend(wrapped);
        self.clipped |= clipped;
    }
    pub(crate) fn grid(&mut self, line: Line<'static>) {
        self.before_content();
        if self.full() {
            self.clipped = true;
        } else {
            self.lines.push(line);
        }
    }
    fn extend(&mut self, lines: impl IntoIterator<Item = Line<'static>>) {
        for line in lines {
            if self.full() {
                self.clipped = true;
                break;
            }
            self.push(line);
        }
    }
    fn finish(mut self) -> Rendered {
        if self.clipped {
            self.lines.push(Line::raw(format!(
                "[Preview line limit reached: {} lines]",
                self.limit
            )));
        }
        Rendered {
            width: self.lines.iter().map(Line::width).max().unwrap_or(0),
            lines: self.lines,
        }
    }
}
impl Rendered {
    /// O(viewport size), including positions beyond u16::MAX.
    pub fn viewport(&self, top: usize, left: usize, width: u16, height: u16) -> Vec<Line<'static>> {
        self.lines
            .iter()
            .skip(top)
            .take(usize::from(height))
            .map(|line| crate::preview_layout::clip(line, left, usize::from(width)))
            .collect()
    }
    pub fn max_left(&self) -> usize {
        self.width
    }
}
fn text(value: &str) -> String {
    crate::mermaid::safe(value)
}
fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut b = vec![];
    reader.take(limit + 1).read_to_end(&mut b)?;
    ensure!(
        b.len() as u64 <= limit,
        "resource exceeds {} MiB",
        limit / 1024 / 1024
    );
    Ok(b)
}
fn decode_url(value: &str) -> Result<String> {
    let mut out = vec![];
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure!(i + 2 < bytes.len(), "invalid URL escape");
            let part = std::str::from_utf8(&bytes[i + 1..i + 3])?;
            out.push(u8::from_str_radix(part, 16)?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(String::from_utf8(out)?)
}
fn image_bytes(destination: &str, opts: &Options) -> Result<Vec<u8>> {
    if destination.starts_with("https://") || destination.starts_with("http://") {
        ensure!(
            opts.remote_images,
            "remote image disabled; enable remote images explicitly"
        );
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()?;
        let response = client.get(destination).send()?.error_for_status()?;
        if let Some(n) = response.content_length() {
            ensure!(n <= MAX_IMAGE, "image exceeds 20 MiB");
        }
        return read_limited(response, MAX_IMAGE);
    }
    if destination.starts_with("data:image/") {
        let (head, data) = destination.split_once(',').context("invalid data image")?;
        ensure!(head.ends_with(";base64"), "data image must be base64");
        ensure!(
            data.len() <= (MAX_IMAGE.div_ceil(3) * 4) as usize,
            "data image exceeds 20 MiB"
        );
        let data = base64::engine::general_purpose::STANDARD.decode(data)?;
        ensure!(data.len() as u64 <= MAX_IMAGE, "image exceeds 20 MiB");
        return Ok(data);
    }
    ensure!(
        !destination.contains("://") && !destination.starts_with("//"),
        "unsupported image URL scheme"
    );
    let name = decode_url(destination.split('#').next().unwrap_or(destination))?;
    let path = opts.base.join(name);
    let file =
        std::fs::File::open(&path).with_context(|| format!("cannot open {}", path.display()))?;
    ensure!(file.metadata()?.is_file(), "image is not a regular file");
    read_limited(file, MAX_IMAGE)
}
fn image_lines(destination: &str, alt: &str, opts: &Options) -> Result<Vec<Line<'static>>> {
    let bytes = image_bytes(destination, opts)?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode()?;
    let max_width = u32::from(opts.width).clamp(1, 160);
    let max_height = u32::from(opts.image_height).clamp(1, 120);
    let (original_w, original_h) = (img.width(), img.height());
    ensure!(original_w > 0 && original_h > 0, "empty image");
    // A terminal cell is approximately twice as tall as it is wide. Two pixels
    // per colored half-block cell preserve square pixels without terminal protocols.
    let scale = (f64::from(max_width) / f64::from(original_w))
        .min(f64::from(max_height * 2) / f64::from(original_h))
        .min(1.0);
    let w = (f64::from(original_w) * scale).round().max(1.0) as u32;
    let h = (f64::from(original_h) * scale).round().max(1.0) as u32;
    let pixels = img
        .resize_exact(w, h, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let rgb = |x: u32, y: u32| {
        let p = pixels.get_pixel(x, y);
        let a = u32::from(p[3]);
        let blend = |v: u8| ((u32::from(v) * a + 24 * (255 - a)) / 255) as u8;
        [blend(p[0]), blend(p[1]), blend(p[2])]
    };
    let mut lines = vec![Line::styled(
        format!("[Image: {} / {}x{}]", text(alt), original_w, original_h),
        Style::default().fg(Color::Cyan),
    )];
    for y in (0..h).step_by(2) {
        let mut spans = vec![];
        for x in 0..w {
            let a = rgb(x, y);
            let b = if y + 1 < h {
                rgb(x, y + 1)
            } else {
                [24, 24, 24]
            };
            if opts.color {
                spans.push(Span::styled(
                    "▀",
                    Style::default()
                        .fg(Color::Rgb(a[0], a[1], a[2]))
                        .bg(Color::Rgb(b[0], b[1], b[2])),
                ));
            } else {
                let n = (u32::from(a[0]) + u32::from(b[0])) * 299
                    + (u32::from(a[1]) + u32::from(b[1])) * 587
                    + (u32::from(a[2]) + u32::from(b[2])) * 114;
                let ramp = b" .:-=+*#%@";
                spans.push(Span::raw(
                    (ramp[(n as usize * 9 / 510000).min(9)] as char).to_string(),
                ));
            }
        }
        lines.push(Line::from(spans));
    }
    Ok(lines)
}
pub fn render(markdown: &str, opts: &Options) -> Rendered {
    render_limited(markdown, opts, MAX_MARKDOWN, MAX_LINES)
}
fn render_limited(
    markdown: &str,
    opts: &Options,
    max_markdown: usize,
    max_lines: usize,
) -> Rendered {
    let mut lines = Output::new(opts.width, max_lines);
    let mut table: Option<crate::preview_table::Table> = None;
    let mut spans = vec![];
    let mut heading = false;
    let mut code: Option<(String, String)> = None;
    let mut image: Option<(String, String)> = None;
    let mut cache: HashMap<String, Vec<Line<'static>>> = HashMap::new();
    let mut image_count = 0usize;
    let mut list_depth = 0usize;
    let mut end = markdown.len().min(max_markdown);
    while !markdown.is_char_boundary(end) {
        end -= 1;
    }
    for event in pulldown_cmark::Parser::new_ext(&markdown[..end], MdOptions::all()) {
        if lines.full() {
            lines.clipped = true;
            break;
        }
        if matches!(event, Event::End(TagEnd::Table)) {
            if let Some(table) = table.take() {
                table.render(&mut lines);
                lines.separate();
            }
            continue;
        }
        if let Some(table) = &mut table {
            table.event(event);
            continue;
        }
        match event {
            Event::Start(Tag::Table(alignments)) => {
                if !spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
                lines.separate();
                table = Some(crate::preview_table::Table::new(alignments));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                if !spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
                lines.separate();
                code = Some((
                    match kind {
                        CodeBlockKind::Fenced(lang) => lang.to_string(),
                        _ => String::new(),
                    },
                    String::new(),
                ));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, source)) = code.take() {
                    if lang.trim().eq_ignore_ascii_case("mermaid") {
                        for line in crate::mermaid::render(&source, usize::from(opts.width)) {
                            lines.grid(Line::styled(line, Style::default().fg(Color::Cyan)));
                        }
                    } else {
                        let mut highlighter = crate::syntax::CodeHighlighter::new(&lang);
                        lines.extend(source.lines().map(|s| highlighter.line(s)));
                    }
                }
                lines.separate();
            }
            Event::Text(t) if code.is_some() => {
                if let Some((_, s)) = &mut code {
                    s.push_str(&t);
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                if !spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
                image = Some((dest_url.into_string(), String::new()));
            }
            Event::Text(t) | Event::Code(t) if image.is_some() => {
                if let Some((_, alt)) = &mut image {
                    alt.push_str(&t);
                }
            }
            Event::End(TagEnd::Image) => {
                if let Some((dest, alt)) = image.take() {
                    image_count += 1;
                    let result = if image_count > MAX_IMAGES {
                        vec![Line::raw(format!(
                            "[Image limit: at most {MAX_IMAGES} per preview]"
                        ))]
                    } else if let Some(hit) = cache.get(&dest) {
                        hit.clone()
                    } else {
                        let result = image_lines(&dest, &alt, opts).unwrap_or_else(|e| {
                            vec![Line::styled(
                                format!("[Image {}: {}]", text(&alt), text(&format!("{e:#}"))),
                                Style::default().fg(Color::Red),
                            )]
                        });
                        cache.insert(dest, result.clone());
                        result
                    };
                    lines.extend(result);
                }
            }
            Event::Start(Tag::Paragraph) => lines.separate(),
            Event::Start(Tag::Heading { .. }) => {
                lines.separate();
                heading = true;
            }
            Event::Start(Tag::List(_)) => {
                if !spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
                if list_depth == 0 {
                    lines.separate();
                }
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                if list_depth == 0 {
                    lines.separate();
                }
            }
            Event::Start(Tag::Item) => spans.push(Span::raw(format!(
                "{}* ",
                "  ".repeat(list_depth.saturating_sub(1).min(16))
            ))),
            Event::Text(t) => spans.push(Span::styled(
                text(&t),
                if heading {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            )),
            Event::Code(t) => {
                spans.push(Span::styled(text(&t), Style::default().fg(Color::Yellow)))
            }
            Event::SoftBreak | Event::HardBreak => {
                lines.push(Line::from(std::mem::take(&mut spans)))
            }
            Event::End(end @ (TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item)) => {
                if !spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
                if matches!(end, TagEnd::Heading(_)) || end == TagEnd::Paragraph {
                    lines.separate();
                }
                heading = false;
            }
            Event::Rule => {
                lines.separate();
                lines.push(Line::raw("-".repeat(usize::from(opts.width))));
                lines.separate();
            }
            Event::TaskListMarker(done) => {
                spans.push(Span::raw(if done { "[x] " } else { "[ ] " }))
            }
            Event::Html(t) | Event::InlineHtml(t) => spans.push(Span::styled(
                format!("[HTML: {}]", text(&t)),
                Style::default().fg(Color::DarkGray),
            )),
            _ => {}
        }
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    if end < markdown.len() {
        lines.grid(Line::raw(format!(
            "[Markdown truncated at {} MiB]",
            MAX_MARKDOWN / 1024 / 1024
        )));
    }
    lines.finish()
}
pub fn read_markdown(path: &Path) -> Result<String> {
    let mut data = if path == Path::new("-") {
        read_limited(std::io::stdin().lock(), MAX_MARKDOWN as u64)?
    } else {
        read_limited(std::fs::File::open(path)?, MAX_MARKDOWN as u64)?
    };
    if data.starts_with(&[0xef, 0xbb, 0xbf]) {
        data.drain(..3);
    }
    String::from_utf8(data).context("Markdown must be UTF-8")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_dimensions_color_transparency_and_plain_raster() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let img = image::RgbaImage::from_fn(4, 4, |x, y| {
            if x < 2 {
                image::Rgba([255, 0, 0, 255])
            } else if y < 2 {
                image::Rgba([0, 255, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 0])
            }
        });
        img.save(dir.path().join("a b.png"))?;
        let mut opts = Options::new(dir.path().to_owned(), 4);
        opts.image_height = 2;
        let md = "# Header\n\n![sample](a%20b.png)\n\n```mermaid\nflowchart LR\n A-->B\n```";
        let r = render(md, &opts);
        assert!(r.plain().contains("4x4"));
        assert!(
            r.lines
                .iter()
                .flat_map(|l| &l.spans)
                .any(|s| s.style.fg == Some(Color::Rgb(255, 0, 0)))
        );
        assert!(r.plain().contains("Connections:"));
        opts.color = false;
        let r = render(md, &opts);
        assert!(!r.plain().contains('▀'));
        assert!(r.plain().contains('@') || r.plain().contains(':'));
        opts.width = 80;
        let repeated = render(&"![sample](a%20b.png)\n\n".repeat(MAX_IMAGES + 1), &opts).plain();
        assert_eq!(repeated.matches("[Image: sample").count(), MAX_IMAGES);
        assert!(repeated.contains("at most 256"));
        Ok(())
    }
    #[test]
    fn errors_keep_following_text_and_do_not_execute_html() {
        let opts = Options::new(PathBuf::from("."), 80);
        let s=render("![missing](does-not-exist.png)\n\nafter\n\n<script>alert(1)</script>\n\n![remote](https://example.com/a.png)",&opts).plain();
        assert!(
            s.contains("cannot open")
                && s.contains("after")
                && s.contains("remote image disabled")
                && s.contains("HTML:")
        );
    }
    #[test]
    fn remote_opt_in_reads_bounded_image_over_http() -> Result<()> {
        use std::{io::Write, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let mut buffer = Cursor::new(vec![]);
        image::DynamicImage::new_rgb8(2, 2).write_to(&mut buffer, image::ImageFormat::Png)?;
        let png = buffer.into_inner();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut b = [0; 4096];
            let _ = stream.read(&mut b);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                png.len()
            )
            .unwrap();
            stream.write_all(&png).unwrap();
        });
        let mut opts = Options::new(PathBuf::from("."), 80);
        opts.remote_images = true;
        let bytes = image_bytes(&format!("http://{addr}/a.png"), &opts)?;
        server.join().unwrap();
        assert!(image::load_from_memory(&bytes).is_ok());
        Ok(())
    }
    #[test]
    fn large_input_and_viewport_pass_former_byte_and_u16_limits() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut source = String::from("```text\n");
        for i in 0..70_100 {
            source.push_str(&format!(
                "row {i:06} 日本語 content beyond previous byte and line limits\n"
            ));
        }
        source.push_str("TAIL_COMPLETE\n```\n");
        assert!(source.len() > 1024 * 1024);
        let path = dir.path().join("large.md");
        std::fs::write(&path, &source)?;
        let source = read_markdown(&path)?;
        let rendered = render(&source, &Options::new(dir.path().to_owned(), 120));
        assert_eq!(rendered.lines.len(), 70_101);
        let page = rendered.viewport(70_095, 0, 120, 10);
        assert_eq!(page.len(), 6);
        assert!(page[0].to_string().contains("row 070095"));
        assert!(page.last().unwrap().to_string().contains("TAIL_COMPLETE"));
        assert!(!rendered.lines.last().unwrap().to_string().contains("limit"));
        Ok(())
    }
    #[test]
    fn tables_align_numbers_style_headers_and_keep_rectangular_cells() {
        let md = "| 名称 | 数量 | 金額 | 状態 |\n| :--- | --- | ---: | :---: |\n| **顧客** | 1,234 | 12.50 | [Ready](https://example.com) |\n| `records.dat` | 42 | -9 | A<br>B |\n| 空欄 | | | |\n";
        let rendered = render(md, &Options::new(PathBuf::from("."), 80));
        let text = rendered.plain();
        for needle in [
            "┌",
            "╞",
            "└",
            "名称",
            "│ 1 │",
            "│ 2 │",
            "│ 3 │",
            "1,234",
            "12.50",
            "records.dat",
        ] {
            assert!(text.contains(needle), "{needle}: {text}");
        }
        let widths = rendered.lines.iter().map(Line::width).collect::<Vec<_>>();
        assert!(widths.iter().all(|w| *w == widths[0]), "{text}");
        assert!(widths[0] <= 80);
        let row = rendered
            .lines
            .iter()
            .find(|line| line.to_string().contains("1,234"))
            .unwrap()
            .to_string();
        let cells = row.split('│').collect::<Vec<_>>();
        assert!(cells[3].ends_with("1,234 "), "{row}");
        assert!(cells[4].ends_with("12.50 "), "{row}");
        assert!(
            rendered
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.content == "顧客"
                    && span.style.add_modifier.contains(Modifier::BOLD))
        );
        assert!(
            rendered
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.content == "Ready"
                    && span.style.add_modifier.contains(Modifier::UNDERLINED))
        );
        assert!(
            rendered
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.content == "records.dat" && span.style.fg == Some(Color::Yellow))
        );
    }
    #[test]
    fn narrow_and_wide_tables_preserve_text_and_bound_output() {
        let md = "|項目|説明|\n|---|---|\n|日本語|alpha beta gamma delta<br>次の行|\n";
        let rendered = render(md, &Options::new(PathBuf::from("."), 26));
        let text = rendered.plain();
        for needle in ["alpha", "beta", "gamma", "delta", "日本語", "次の行"] {
            assert!(text.contains(needle), "{needle}: {text}");
        }
        assert!(
            rendered
                .lines
                .iter()
                .all(|line| line.width() == rendered.width)
        );
        let header = (0..28)
            .map(|i| format!("H{i}"))
            .collect::<Vec<_>>()
            .join("|");
        let separator = vec!["---"; 28].join("|");
        let wide = render(
            &format!("|{header}|\n|{separator}|\n"),
            &Options::new(PathBuf::from("."), 20),
        );
        assert!(wide.width > 20);
        assert!(wide.plain().contains("AA"));
        assert!(
            wide.viewport(1, wide.width - 20, 20, 1)[0]
                .to_string()
                .contains("AB")
        );
        for source in [
            "```text\n".to_owned() + &"long line of text\n".repeat(1000) + "```",
            md.repeat(100),
        ] {
            let bounded = render_limited(
                &source,
                &Options::new(PathBuf::from("."), 10),
                MAX_MARKDOWN,
                50,
            );
            assert_eq!(bounded.lines.len(), 51);
            assert!(
                bounded
                    .lines
                    .last()
                    .unwrap()
                    .to_string()
                    .contains("line limit")
            );
        }
    }
    #[test]
    fn fenced_code_uses_language_tokens_and_keeps_multiline_state_after_wrapping() {
        let source = "```rust\n/* open\nstill comment\n*/ let text = \"日本語\";\n```\n\n```sql\nSELECT 42, 'text';\n```\n";
        let rendered = render(source, &Options::new(PathBuf::from("."), 80));
        assert!(
            rendered
                .lines
                .iter()
                .flat_map(|l| &l.spans)
                .any(|s| s.content.contains("still comment")
                    && s.style.fg == crate::syntax::token_style("comment").fg)
        );
        for keyword in ["let", "SELECT"] {
            assert!(
                rendered
                    .lines
                    .iter()
                    .flat_map(|l| &l.spans)
                    .any(|s| s.content.contains(keyword)
                        && s.style.fg == crate::syntax::token_style("keyword").fg)
            );
        }
        assert!(rendered.plain().contains("日本語"));
    }
    #[test]
    fn block_spacing_separates_headings_paragraphs_and_lists_once() {
        let source = "# Heading\n\nFirst paragraph.\n\n- one\n- two\n\nSecond paragraph.\n\nThird paragraph.\n\n## Next heading\n\nLast paragraph.\n";
        let rendered = render(source, &Options::new(PathBuf::from("."), 80));
        assert_eq!(
            rendered.plain(),
            "Heading\n\nFirst paragraph.\n\n* one\n* two\n\nSecond paragraph.\n\nThird paragraph.\n\nNext heading\n\nLast paragraph."
        );
        assert_eq!(
            render("# One\n## Two\n", &Options::new(PathBuf::from("."), 80)).plain(),
            "One\n\nTwo"
        );
    }
    #[test]
    fn spacing_keeps_nested_lists_soft_breaks_and_code_lines_intact() {
        let source = "Before\ncontinued\n\n- parent\n  - child\n- sibling\n\n```text\nalpha\n\nbeta\n```\n\nAfter\n";
        let text = render(source, &Options::new(PathBuf::from("."), 80)).plain();
        assert_eq!(
            text,
            "Before\ncontinued\n\n* parent\n  * child\n* sibling\n\nalpha\n\nbeta\n\nAfter"
        );
        assert!(!text.contains("\n\n\n"));
        assert_eq!(
            render(
                "- one\n\n- two\n\nAfter",
                &Options::new(PathBuf::from("."), 80)
            )
            .plain(),
            "* one\n\n* two\n\nAfter"
        );
        let bounded = render_limited(
            "one\n\ntwo\n\nthree",
            &Options::new(PathBuf::from("."), 80),
            MAX_MARKDOWN,
            3,
        );
        assert_eq!(bounded.lines.len(), 4);
        assert!(
            bounded
                .lines
                .last()
                .unwrap()
                .to_string()
                .contains("line limit")
        );
    }
    #[test]
    fn controls_limits_and_unclosed_fences_are_safe() {
        let opts = Options::new(PathBuf::from("."), 80);
        assert!(!render("bad\u{1b}[2J", &opts).plain().contains('\u{1b}'));
        assert!(
            render("```mermaid\nunknown\n", &opts)
                .plain()
                .contains("unsupported")
        );
        let rendered = render_limited("日本語日本語", &opts, 8, 100).plain();
        assert!(rendered.contains("truncated"));
        assert!(!rendered.contains('�'));
    }
}
