use crate::{App, COMMANDS, HELP, Prompt, View};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::path::Path;
use unicode_width::UnicodeWidthStr;
fn block(title: impl Into<String>, active: bool) -> Block<'static> {
    Block::bordered()
        .title(title.into())
        .border_style(Style::default().fg(if active { Color::Cyan } else { Color::DarkGray }))
}
fn markdown(text: &str) -> Vec<Line<'static>> {
    use pulldown_cmark::{Event as E, Tag, TagEnd};
    let mut lines = Vec::new();
    let mut spans = Vec::new();
    let mut heading = false;
    let mut code = false;
    for event in pulldown_cmark::Parser::new_ext(text, pulldown_cmark::Options::all()) {
        match event {
            E::Start(Tag::Heading { .. }) => heading = true,
            E::Start(Tag::CodeBlock(_)) => code = true,
            E::Text(t) | E::Code(t) => spans.push(Span::styled(
                t.into_string(),
                Style::default().fg(if code {
                    Color::Yellow
                } else if heading {
                    Color::Cyan
                } else {
                    Color::White
                }),
            )),
            E::SoftBreak | E::HardBreak => {
                lines.push(Line::from(std::mem::take(&mut spans)));
            }
            E::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::Item) => {
                lines.push(Line::from(std::mem::take(&mut spans)));
                heading = false;
                code = false;
            }
            E::Start(Tag::Item) => spans.push(Span::raw("* ")),
            _ => {}
        }
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}
pub(crate) fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new("yyte  F1 Menu  F2 Editor  F3 Git  F4 Terminal  F5 Preview  F6 Explorer"),
        header,
    );
    let show_explorer = body.width >= 65 || app.explorer;
    let [left, right] = Layout::horizontal([
        Constraint::Length(if show_explorer { body.width.min(28) } else { 0 }),
        Constraint::Min(0),
    ])
    .areas(body);
    if show_explorer {
        let visible = left.height.saturating_sub(2) as usize;
        let start = app.selected.saturating_sub(visible.saturating_sub(1));
        let items = app
            .entries
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(i, p)| {
                let label = if p == &app.root.parent().unwrap_or(Path::new("")).to_path_buf() {
                    "[..]".into()
                } else {
                    format!(
                        "{} {}",
                        if p.is_dir() { "[+]" } else { "   " },
                        p.file_name().unwrap_or_default().to_string_lossy()
                    )
                };
                ListItem::new(label).style(if app.explorer && i == app.selected {
                    Style::default().bg(Color::Blue)
                } else {
                    Style::default()
                })
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(block("Explorer", app.explorer)),
            left,
        );
    }
    let title = match app.view {
        View::Editor => app
            .tabs
            .iter()
            .enumerate()
            .map(|(i, e)| {
                format!(
                    "{}{}{}",
                    if i == app.tab { "[" } else { " " },
                    e.path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or("Untitled".into()),
                    if e.modified() { " *]" } else { "]" }
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
        View::Terminal => "Terminal".into(),
        View::Git => "Git".into(),
        View::Preview => "Markdown preview".into(),
        View::Help => "Help".into(),
    };
    let border = block(title, !app.explorer);
    let inner = border.inner(right);
    frame.render_widget(border, right);
    if inner.width > 0 && inner.height > 0 {
        match app.view {
            View::Editor => {
                let e = &mut app.tabs[app.tab];
                let at = e.caret();
                let mut rows = e.rows(inner.height as usize, inner.width as usize);
                if !rows.iter().any(|r| {
                    at >= r.0 && (at < r.1 || (r.1 == e.doc.snapshot().len() && at == r.1))
                }) {
                    e.top = at;
                    rows = e.rows(inner.height as usize, inner.width as usize);
                }
                let selection = e.doc.selections().primary().range();
                let lines = rows
                    .iter()
                    .map(|(start, end, text)| {
                        let style = if selection.start < *end && selection.end > *start {
                            Style::default().bg(Color::Blue)
                        } else {
                            Style::default()
                        };
                        Line::styled(text.clone(), style)
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(Paragraph::new(lines), inner);
                if !app.explorer
                    && let Some((row, (start, _, _))) =
                        rows.iter().enumerate().rfind(|(_, r)| at >= r.0)
                {
                    let prefix = e.doc.snapshot().read(*start..at);
                    let text = String::from_utf8_lossy(&prefix)
                        .replace('\t', "    ")
                        .replace('\r', "");
                    let x = UnicodeWidthStr::width(text.as_str())
                        .min(inner.width.saturating_sub(1) as usize)
                        as u16;
                    frame.set_cursor_position((inner.x + x, inner.y + row as u16));
                }
                app.rows = rows;
                app.editor_area = inner;
            }
            View::Terminal => {
                if let Some(shell) = &mut app.shell {
                    if let Err(e) = shell.resize(inner.width, inner.height) {
                        app.status = e.to_string();
                    }
                    for y in 0..inner.height {
                        let row = shell.screen.view_line(app.term_back, y as usize);
                        for (x, cell) in row.cells.iter().enumerate().take(inner.width as usize) {
                            if cell.is_continuation() {
                                continue;
                            }
                            let color = |c| match c {
                                yy_term::Color::Default => Color::Reset,
                                yy_term::Color::Indexed(i) => Color::Indexed(i),
                                yy_term::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
                            };
                            let mut style = Style::default()
                                .fg(color(cell.attr.fg))
                                .bg(color(cell.attr.bg));
                            if cell.attr.has(yy_term::screen::flags::BOLD) {
                                style = style.add_modifier(Modifier::BOLD);
                            }
                            if cell.attr.has(yy_term::screen::flags::INVERSE) {
                                style = style.add_modifier(Modifier::REVERSED);
                            }
                            frame.buffer_mut()[(inner.x + x as u16, inner.y + y)]
                                .set_symbol(&cell.text())
                                .set_style(style);
                        }
                    }
                    if !app.explorer && app.term_back == 0 && shell.screen.modes().cursor_visible {
                        let (y, x) = shell.screen.cursor();
                        if x < inner.width as usize && y < inner.height as usize {
                            frame.set_cursor_position((inner.x + x as u16, inner.y + y as u16));
                        }
                    }
                }
            }
            View::Git => frame.render_widget(
                Paragraph::new(app.output.as_str()).scroll((app.scroll, 0)),
                inner,
            ),
            View::Help => frame.render_widget(
                Paragraph::new(HELP)
                    .wrap(Wrap { trim: false })
                    .scroll((app.scroll, 0)),
                inner,
            ),
            View::Preview => {
                let e = &app.tabs[app.tab];
                let snap = e.doc.snapshot();
                let text = String::from_utf8_lossy(&snap.read(0..snap.len().min(1024 * 1024)))
                    .into_owned();
                frame.render_widget(
                    Paragraph::new(markdown(&text))
                        .wrap(Wrap { trim: false })
                        .scroll((app.scroll, 0)),
                    inner,
                );
            }
        }
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{}{} | {} | byte {}/{}",
            if app.busy { "[Working] " } else { "" },
            app.status,
            app.tabs[app.tab].doc.encoding().name(),
            app.tabs[app.tab].caret(),
            app.tabs[app.tab].doc.snapshot().len()
        )),
        status,
    );
    if app.palette.is_some() || app.prompt.is_some() {
        let h = if app.palette.is_some() {
            (COMMANDS.len() + 2) as u16
        } else {
            4
        };
        let w = frame.area().width.saturating_sub(4).min(72);
        let rect = Rect::new(
            frame.area().width.saturating_sub(w) / 2,
            frame.area().height.saturating_sub(h) / 2,
            w,
            h.min(frame.area().height),
        );
        frame.render_widget(Clear, rect);
        if let Some(selected) = app.palette {
            let items = COMMANDS
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    ListItem::new(*c).style(if i == selected {
                        Style::default().bg(Color::Blue)
                    } else {
                        Style::default()
                    })
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(selected));
            frame.render_stateful_widget(
                List::new(items).block(block("Commands: arrows / Enter / Esc", true)),
                rect,
                &mut state,
            );
        } else {
            let title = match app.prompt {
                Some(Prompt::Open) => "Open path",
                Some(Prompt::Save) => "Save as path (new file)",
                Some(Prompt::Find) => "Find next text (16 MiB)",
                Some(Prompt::Commit) => "Commit staged changes: message",
                Some(Prompt::Encoding) => "Next open: auto, CP932, IBM-930/fixed:80, ...",
                _ => "Unsaved changes: type discard to quit",
            };
            frame.render_widget(
                Paragraph::new(app.input.as_str()).block(block(title, true)),
                rect,
            );
            if rect.width > 2 && rect.height > 2 {
                frame.set_cursor_position((
                    rect.x
                        + 1
                        + UnicodeWidthStr::width(app.input.as_str())
                            .min(rect.width.saturating_sub(3) as usize)
                            as u16,
                    rect.y + 1,
                ));
            }
        }
    }
}
