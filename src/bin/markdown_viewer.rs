use anyhow::{Context, Result, ensure};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    widgets::Paragraph,
};
use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
    time::Duration,
};
use yyterminal_editor::{
    preview::{self, Options, Rendered},
    terminal_color,
};
fn print(rendered: &Rendered, color: bool, depth: terminal_color::Depth) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    if !color {
        for line in &rendered.lines {
            for span in &line.spans {
                write!(out, "{}", span.content)?;
            }
            writeln!(out)?;
        }
        out.flush()?;
        return Ok(());
    }
    for line in &rendered.lines {
        for span in &line.spans {
            write!(out, "\x1b[0m")?;
            let style = line.style.patch(span.style);
            for (modifier, code) in [
                (ratatui::style::Modifier::BOLD, 1),
                (ratatui::style::Modifier::ITALIC, 3),
                (ratatui::style::Modifier::UNDERLINED, 4),
                (ratatui::style::Modifier::CROSSED_OUT, 9),
            ] {
                if style.add_modifier.contains(modifier) {
                    write!(out, "\x1b[{code}m")?;
                }
            }
            for (prefix, c) in [(38, style.fg), (48, style.bg)] {
                if let Some(c) = c {
                    use ratatui::style::Color;
                    match terminal_color::adapt(c, depth) {
                        Color::Rgb(r, g, b) => write!(out, "\x1b[{prefix};2;{r};{g};{b}m")?,
                        Color::Indexed(i) => write!(out, "\x1b[{prefix};5;{i}m")?,
                        Color::Reset => {}
                        named => {
                            // SGR 30-37 / 90-97 in palette order; backgrounds add 10.
                            let index = [
                                Color::Black,
                                Color::Red,
                                Color::Green,
                                Color::Yellow,
                                Color::Blue,
                                Color::Magenta,
                                Color::Cyan,
                                Color::Gray,
                                Color::DarkGray,
                                Color::LightRed,
                                Color::LightGreen,
                                Color::LightYellow,
                                Color::LightBlue,
                                Color::LightMagenta,
                                Color::LightCyan,
                                Color::White,
                            ]
                            .iter()
                            .position(|&n| n == named)
                            .unwrap_or(7) as u8;
                            let code = if index < 8 { 30 + index } else { 82 + index }
                                + if prefix == 48 { 10 } else { 0 };
                            write!(out, "\x1b[{code}m")?;
                        }
                    }
                }
            }
            write!(out, "{}", span.content)?;
        }
        writeln!(out, "\x1b[0m")?;
    }
    out.flush()?;
    Ok(())
}
fn interactive(source: String, mut opts: Options) -> Result<()> {
    let source: std::sync::Arc<str> = source.into();
    let mut terminal = ratatui::try_init()?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = crossterm::execute!(std::io::stdout(), event::DisableMouseCapture);
            ratatui::restore();
        }
    }
    let _restore = Restore;
    crossterm::execute!(std::io::stdout(), event::EnableMouseCapture)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut rendered = Rendered::default();
    let mut raster = yyterminal_editor::render_cache::RasterViewport::default();
    let mut pending = false;
    let mut last_width = 0;
    let mut scroll = 0usize;
    let mut left = 0usize;
    let mut error = String::new();
    let mut dirty = true;
    let mut mouse_actions = vec![];
    let mut inputs = yyterminal_editor::input::Queue::default();
    let depth = terminal_color::detect();
    loop {
        let width = terminal.size()?.width.clamp(1, preview::MAX_WIDTH);
        if width != last_width && !pending {
            opts.width = width;
            last_width = width;
            pending = true;
            let source = source.clone();
            let opts = opts.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(preview::render(&source, &opts));
            });
        }
        if let Ok(value) = rx.try_recv() {
            rendered = value;
            raster.clear();
            pending = false;
            dirty = true;
            error.clear();
        }
        if dirty {
            terminal.draw(|f| {
                let [controls, body, status] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .areas(f.area());
                scroll = scroll.min(
                    rendered
                        .lines
                        .len()
                        .saturating_sub(usize::from(body.height)),
                );
                left = left.min(rendered.max_left().saturating_sub(usize::from(body.width)));
                mouse_actions.clear();
                let mut x = controls.x;
                for (label, key) in [
                    ("[Close]", KeyCode::Char('q')),
                    ("[Up]", KeyCode::PageUp),
                    ("[Down]", KeyCode::PageDown),
                    ("[Top]", KeyCode::Home),
                    ("[End]", KeyCode::End),
                    ("[<]", KeyCode::Left),
                    ("[>]", KeyCode::Right),
                ] {
                    if x < controls.right() && controls.height > 0 {
                        let w = (label.len() as u16).min(controls.right() - x);
                        let rect = ratatui::layout::Rect::new(x, controls.y, w, 1);
                        f.render_widget(Paragraph::new(label), rect);
                        mouse_actions.push((rect, key));
                        x += w + 1;
                    }
                }
                raster.paint(f, body, &rendered, scroll, left);
                f.render_widget(
                    Paragraph::new(format!(
                        "markdownViewer | {} cols | {}{} | q: quit | arrows/PgUp/PgDn",
                        width,
                        if pending { "Rendering..." } else { "Ready" },
                        error
                    )),
                    status,
                );
                terminal_color::adapt_buffer(f.buffer_mut(), depth);
            })?;
            dirty = false;
        }
        if let Some((input, repeats)) = inputs.read(Duration::from_millis(8))? {
            dirty = true;
            let rows = 3usize.saturating_mul(repeats);
            let columns = 4usize.saturating_mul(repeats);
            let input = match input {
                Event::Mouse(mouse) => match mouse.kind {
                    event::MouseEventKind::ScrollUp => {
                        scroll = scroll.saturating_sub(rows);
                        None
                    }
                    event::MouseEventKind::ScrollDown => {
                        scroll = scroll.saturating_add(rows);
                        None
                    }
                    event::MouseEventKind::ScrollLeft => {
                        left = left.saturating_sub(columns);
                        None
                    }
                    event::MouseEventKind::ScrollRight => {
                        left = left.saturating_add(columns);
                        None
                    }
                    event::MouseEventKind::Down(event::MouseButton::Left) => mouse_actions
                        .iter()
                        .find(|(r, _)| {
                            r.contains(ratatui::layout::Position::new(mouse.column, mouse.row))
                        })
                        .map(|(_, key)| Event::Key(event::KeyEvent::new(*key, KeyModifiers::NONE))),
                    _ => None,
                },
                other => Some(other),
            };
            match input {
                Some(Event::Resize(_, _)) => {}
                Some(Event::Key(k)) if k.kind != KeyEventKind::Release => match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up => scroll = scroll.saturating_sub(1),
                    KeyCode::Down => scroll = scroll.saturating_add(1),
                    KeyCode::PageUp => {
                        scroll = scroll
                            .saturating_sub(usize::from(terminal.size()?.height.saturating_sub(2)))
                    }
                    KeyCode::PageDown | KeyCode::Char(' ') => {
                        scroll = scroll
                            .saturating_add(usize::from(terminal.size()?.height.saturating_sub(2)))
                    }
                    KeyCode::Left => left = left.saturating_sub(4),
                    KeyCode::Right => left = left.saturating_add(4),
                    KeyCode::Home => {
                        scroll = 0;
                        left = 0;
                    }
                    KeyCode::End => {
                        scroll = rendered
                            .lines
                            .len()
                            .saturating_sub(usize::from(terminal.size()?.height.saturating_sub(2)))
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let mut path = None;
    let mut width = 80u16;
    let mut height = 32u16;
    let mut plain = false;
    let mut color = false;
    let mut remote = false;
    let mut force_print = false;
    let mut base = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "markdownViewer [OPTIONS] FILE.md|-\n\nTerminal Markdown viewer with pixel images and Mermaid ASCII diagrams.\nTTY: interactive; q/Esc quit, arrows/PgUp/PgDn/Home/End scroll.\nRedirected output: plain text. Markdown input: UTF-8, at most 128 MiB.\n\n--plain          Print plain text with ASCII image raster\n--print          Print without entering interactive viewer\n--color          Use truecolor half-block images, including redirected output\n--width N        Output width (1..4096, default 80)\n--image-height N Maximum image height (1..120 terminal rows, default 32)\n--base-dir DIR   Resolve relative images here (default: Markdown directory)\n--remote-images  Permit HTTP/HTTPS image downloads (5s timeout, 20 MiB each)\n--version        Show version\n\nSupported diagrams, resource limits and image formats: docs/user/help.md"
                );
                return Ok(());
            }
            Some("--version") => {
                println!("markdownViewer {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--plain") => {
                plain = true;
                force_print = true;
            }
            Some("--print") => force_print = true,
            Some("--color") => color = true,
            Some("--remote-images") => remote = true,
            Some("--width") => {
                width = args
                    .next()
                    .context("--width needs a number")?
                    .to_string_lossy()
                    .parse()?;
                ensure!((1..=preview::MAX_WIDTH).contains(&width), "width: 1..4096");
            }
            Some("--image-height") => {
                height = args
                    .next()
                    .context("--image-height needs a number")?
                    .to_string_lossy()
                    .parse()?;
                ensure!((1..=120).contains(&height), "height: 1..120");
            }
            Some("--base-dir") => {
                base = Some(PathBuf::from(
                    args.next().context("--base-dir needs a directory")?,
                ))
            }
            Some(s) if s.starts_with('-') && s != "-" => anyhow::bail!("Unknown option: {s}"),
            _ => {
                ensure!(path.is_none(), "Expected one Markdown file");
                path = Some(PathBuf::from(arg));
            }
        }
    }
    let path = path.context("Usage: markdownViewer [OPTIONS] FILE.md|- (see --help)")?;
    let source = preview::read_markdown(&path)?;
    let interactive_mode =
        !force_print && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let base = base.unwrap_or_else(|| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."))
            .to_owned()
    });
    let mut opts = Options::new(base, width);
    opts.color = !plain && (color || interactive_mode || std::io::stdout().is_terminal());
    opts.image_height = height;
    opts.remote_images = remote;
    if interactive_mode {
        interactive(source, opts)
    } else {
        // An explicit --color keeps truecolor; automatic color follows the terminal.
        let depth = if color {
            terminal_color::Depth::TrueColor
        } else {
            terminal_color::detect()
        };
        print(&preview::render(&source, &opts), opts.color, depth)
    }
}
