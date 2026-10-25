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
pub const MAX_MARKDOWN: usize = 1024 * 1024;
const MAX_IMAGE: u64 = 20 * 1024 * 1024;
const MAX_LINES: usize = 20_000;
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
            width: width.clamp(1, 240),
            image_height: 32,
            color: true,
            remote_images: false,
        }
    }
}
#[derive(Default)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
}
impl Rendered {
    pub fn plain(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
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
    let mut lines = vec![];
    let mut spans = vec![];
    let mut heading = false;
    let mut code: Option<(String, String)> = None;
    let mut image: Option<(String, String)> = None;
    let mut cache: HashMap<String, Vec<Line<'static>>> = HashMap::new();
    let mut image_count = 0usize;
    let mut list_depth = 0usize;
    let mut end = markdown.len().min(MAX_MARKDOWN);
    while !markdown.is_char_boundary(end) {
        end -= 1;
    }
    for event in pulldown_cmark::Parser::new_ext(&markdown[..end], MdOptions::all()) {
        if lines.len() >= MAX_LINES {
            lines.push(Line::raw("[Preview line limit reached]"));
            break;
        }
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                if !spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
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
                        lines.extend(
                            crate::mermaid::render(&source, usize::from(opts.width))
                                .into_iter()
                                .map(|s| Line::styled(s, Style::default().fg(Color::Cyan))),
                        );
                    } else {
                        lines.extend(
                            source
                                .lines()
                                .map(|s| Line::styled(text(s), Style::default().fg(Color::Yellow))),
                        );
                    }
                }
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
                    let result = if let Some(hit) = cache.get(&dest) {
                        hit.clone()
                    } else if image_count > 16 {
                        vec![Line::raw("[Image limit: at most 16 per preview]")]
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
            Event::Start(Tag::Heading { .. }) => heading = true,
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => spans.push(Span::raw(format!(
                "{}* ",
                "  ".repeat(list_depth.saturating_sub(1).min(16))
            ))),
            Event::Start(Tag::TableCell) => {
                if !spans.is_empty() {
                    spans.push(Span::raw(" | "));
                }
            }
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
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::TableRow
                | TagEnd::TableHead,
            ) => {
                if !spans.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                }
                heading = false;
            }
            Event::Rule => lines.push(Line::raw("-".repeat(usize::from(opts.width)))),
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
        lines.push(Line::raw("[Markdown truncated at 1 MiB]"));
    }
    lines.truncate(MAX_LINES + 1);
    Rendered { lines }
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
    fn controls_limits_and_unclosed_fences_are_safe() {
        let opts = Options::new(PathBuf::from("."), 80);
        assert!(!render("bad\u{1b}[2J", &opts).plain().contains('\u{1b}'));
        assert!(
            render("```mermaid\nunknown\n", &opts)
                .plain()
                .contains("unsupported")
        );
        let s = "x".repeat(MAX_MARKDOWN + 10);
        assert!(render(&s, &opts).plain().contains("truncated"));
    }
}
