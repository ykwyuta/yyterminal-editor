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
    app.hit_cells.clear();
    let [header, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new("yyte F1 Menu F2 Edit F3 Git F4 Shell F5 MD F6 Files F7 Table"),
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
        View::Editor | View::Table | View::Sheet => app
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
        View::Diff => "yymerge comparison".into(),
        View::Preview => "Markdown preview".into(),
        View::Help => "Help".into(),
    };
    let border = block(title, !app.explorer);
    let inner = border.inner(right);
    frame.render_widget(border, right);
    if inner.width > 0 && inner.height > 0 {
        match app.view {
            View::Diff => draw_diff(frame, app, inner),
            View::Sheet => draw_book(frame, app, inner),
            View::Table => draw_grid(frame, app, inner),
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
            if app.tabs[app.tab].raw {
                "raw bytes"
            } else {
                app.tabs[app.tab].doc.encoding().name()
            },
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
                Some(Prompt::LayoutOpen) => "Apply .yyl layout path to current data file",
                Some(Prompt::LayoutSave) => "Save layout to a NEW .yyl path",
                Some(Prompt::LayoutInfer) => {
                    "Infer: LENGTH or auto;MS932 or IBM-xxx;none/crlf/lf/nl"
                }
                Some(Prompt::BookCell) => "Cell / formula: Ctrl+A clears; Enter applies",
                Some(Prompt::BookFilter) => "Wildcard filter on current column (* and ?)",
                Some(Prompt::BookFill) => "Additional rows to autofill from selection",
                Some(Prompt::BookHeight) => "Row height: 1..20 terminal lines",
                Some(Prompt::FixedExport) => "Export fixed-length data to a NEW path",
                Some(Prompt::DiffPath) => "yymerge: compare current buffer with file path",
                Some(Prompt::BookWidth) => "Column width: 4..80 terminal cells",
                Some(Prompt::BookFormat) => "Excel number format (e.g. #,##0.00)",
                Some(Prompt::BookSheet) => "New sheet name",
                Some(Prompt::BookMove) => "Move selected column to (A,B,...)",
                Some(Prompt::BookLayout) => "Layout name for the selected record",
                Some(Prompt::BookExport) => "Export active sheet to a new CSV path",
                Some(Prompt::Cell) => "Edit cell: Ctrl+A clear / Alt+Enter newline / Enter apply",
                Some(Prompt::Fixed) => "COPYBOOK;RECORD_BYTES;CHARSET;SEPARATOR",
                Some(Prompt::Delimiter) => "Delimiter (e.g. , or \\t or |)",
                Some(Prompt::Encoding) => "Next open: auto, CP932, IBM-930/fixed:80, ...",
                _ => "Unsaved changes: type discard to quit",
            };
            let displayed = app
                .input
                .chars()
                .map(|c| match c {
                    '\n' => '↵',
                    '\r' => '␍',
                    '\t' => '→',
                    c if c.is_control() => '�',
                    c => c,
                })
                .collect::<String>();
            frame.render_widget(
                Paragraph::new(displayed.as_str()).block(block(title, true)),
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

fn draw_grid(frame: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::widgets::{Cell, Row, Table};
    let editor = &mut app.tabs[app.tab];
    let Some(grid) = &mut editor.table else {
        frame.render_widget(
            Paragraph::new("F7: CSV/TSV table; menu: Fixed-length table"),
            area,
        );
        return;
    };
    let Some(page) = &grid.page else {
        frame.render_widget(
            Paragraph::new(grid.error.as_deref().unwrap_or("Loading table page...")),
            area,
        );
        return;
    };
    let visible_rows = (area.height.saturating_sub(3) / grid.height).max(1) as usize;
    grid.top = grid.top.min(grid.row);
    if grid.row >= grid.top + visible_rows {
        grid.top = grid.row + 1 - visible_rows;
    }
    let visible_cols = (area.width.saturating_sub(8) / (grid.width + 1)).max(1) as usize;
    grid.left = grid.left.min(grid.col);
    if grid.col >= grid.left + visible_cols {
        grid.left = grid.col + 1 - visible_cols;
    }
    let columns = page.rows.iter().map(|r| r.fields.len()).max().unwrap_or(1);
    let headers = grid.mode.headers(columns);
    let header = Row::new(
        std::iter::once("Record".to_owned())
            .chain(headers.into_iter().skip(grid.left).take(visible_cols)),
    )
    .style(Style::default().fg(Color::Cyan));
    let rows = page
        .rows
        .iter()
        .enumerate()
        .skip(grid.top)
        .take(visible_rows)
        .map(|(r, row)| {
            let mut cells = vec![Cell::from((grid.base + r as u64 + 1).to_string())];
            for col in grid.left..(grid.left + visible_cols).min(row.fields.len()) {
                let text = wrap_cell(&grid.value_at(row, col), grid.width, grid.height);
                let cell = Cell::from(text);
                let (t, l, b, rr) = grid.rectangle();
                cells.push(if r >= t && r <= b && col >= l && col <= rr {
                    cell.style(Style::default().bg(Color::Blue).fg(Color::White))
                } else {
                    cell
                });
            }
            Row::new(cells).height(grid.height)
        });
    let widths = std::iter::once(Constraint::Length(7))
        .chain((0..visible_cols).map(|_| Constraint::Length(grid.width)));
    let [label, table, detail] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    for (n, _) in page
        .rows
        .iter()
        .enumerate()
        .skip(grid.top)
        .take(visible_rows)
    {
        for c in grid.left..(grid.left + visible_cols).min(columns) {
            let x = table.x + 8 + (c - grid.left) as u16 * (grid.width + 1);
            let y = table.y + 1 + (n - grid.top) as u16 * grid.height;
            if x < table.right() && y < table.bottom() {
                app.hit_cells.push((
                    Rect::new(
                        x,
                        y,
                        grid.width.min(table.right() - x),
                        grid.height.min(table.bottom() - y),
                    ),
                    n as u64,
                    c as u32,
                ));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{} / Enter: edit / PgUp,PgDn: page",
            grid.mode.name()
        )),
        label,
    );
    frame.render_widget(
        Table::new(rows, widths).header(header).column_spacing(1),
        table,
    );
    let info = if !page.notice.is_empty() {
        page.notice.clone()
    } else {
        format!(
            "Record {} / column {} | F8 delete / F9 append / F7 text",
            grid.base + grid.row as u64 + 1,
            grid.col + 1
        )
    };
    frame.render_widget(Paragraph::new(info), detail);
}

fn draw_book(frame: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::widgets::{Cell, Row, Table};
    let Some(book) = &mut app.tabs[app.tab].book else {
        frame.render_widget(Paragraph::new("Working on workbook..."), area);
        return;
    };
    let visible = area.height.saturating_sub(3).max(1) as u64;
    book.top = book.top.min(book.row);
    if book.row >= book.top + visible {
        book.top = book.row + 1 - visible;
    }
    while (book.top..=book.row)
        .take(visible as usize + 1)
        .map(|r| u64::from(book.row_height(r)))
        .sum::<u64>()
        > visible
        && book.top < book.row
    {
        book.top += 1;
    }
    let mut row_sizes = vec![];
    let mut remaining = visible as u16;
    for r in book.top..book.top + visible {
        let height = book.row_height(r).min(remaining);
        if height == 0 {
            break;
        }
        row_sizes.push((r, height));
        remaining -= height;
    }
    book.left = book.left.min(book.col);
    let mut columns = vec![];
    let mut used = 8;
    let mut col = book.left;
    while used + 4 < u32::from(area.width)
        && col
            < if book.current().fixed.is_some() {
                book.data_cols()
            } else {
                16384
            }
    {
        let width = book
            .current()
            .col_widths
            .get(&col)
            .copied()
            .unwrap_or(16.0)
            .clamp(4.0, 80.0) as u16;
        if used + u32::from(width) + 1 > u32::from(area.width) && !columns.is_empty() {
            break;
        }
        columns.push((col, width));
        used += u32::from(width) + 1;
        col += 1;
    }
    if !columns.iter().any(|(c, _)| *c == book.col) {
        book.left = book.col;
        columns.clear();
        columns.push((book.col, area.width.saturating_sub(9).max(4)));
    }
    let selected = book.range();
    let sheet = book.current();
    let [label, grid, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(format!(
            "{} ({}/{}) | Shift+arrows selects | Enter edits",
            sheet.name,
            book.visible_sheets()
                .iter()
                .position(|i| *i == book.sheet)
                .unwrap_or(0)
                + 1,
            book.visible_sheets().len()
        )),
        label,
    );
    let header = Row::new(
        std::iter::once("Row".to_owned())
            .chain(columns.iter().map(|(c, _)| yy_sheet::col_name(*c))),
    )
    .style(Style::default().fg(Color::Cyan));
    let rows = row_sizes.iter().map(|&(r, height)| {
        let mut cells = vec![Cell::from((r + 1).to_string())];
        for &(c, width) in &columns {
            let value = book
                .value(r, c)
                .map(|v| {
                    if let (Some(format), Some(n)) = (sheet.style_at(r, c).num_fmt, v.as_number()) {
                        yy_numfmt::format_number(&format, n, book.doc.book.date_system)
                    } else {
                        v.general_text()
                    }
                })
                .unwrap_or_else(|e| format!("#IO:{e}"));
            let text = wrap_cell(&value, width, height);
            let source_style = sheet.style_at(r, c);
            let mut style = Style::default();
            if source_style.bold == Some(true) {
                style = style.add_modifier(Modifier::BOLD);
            }
            if source_style.italic == Some(true) {
                style = style.add_modifier(Modifier::ITALIC);
            }
            let rgb = |n: u32| Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8);
            if let Some(color) = source_style.color_rgb() {
                style = style.fg(rgb(color));
            }
            if let Some(color) = source_style.fill_rgb() {
                style = style.bg(rgb(color));
            }
            if r >= selected.top
                && r <= selected.bottom
                && c >= selected.left
                && c <= selected.right
            {
                style = style.bg(Color::Blue).fg(Color::White);
            }
            cells.push(Cell::from(text).style(style));
        }
        Row::new(cells).height(height)
    });
    let widths = std::iter::once(Constraint::Length(7))
        .chain(columns.iter().map(|(_, w)| Constraint::Length(*w)));
    let mut y = grid.y + 1;
    for &(r, height) in &row_sizes {
        let mut x = grid.x + 8;
        for &(c, width) in &columns {
            if x < grid.right() && y < grid.bottom() {
                app.hit_cells.push((
                    Rect::new(
                        x,
                        y,
                        width.min(grid.right() - x),
                        height.min(grid.bottom() - y),
                    ),
                    r,
                    c,
                ));
            }
            x = x.saturating_add(width + 1);
        }
        y = y.saturating_add(height);
    }
    frame.render_widget(
        Table::new(rows, widths).header(header).column_spacing(1),
        grid,
    );
    let entry = book.entry().unwrap_or_default().replace('\n', "↵");
    frame.render_widget(
        Paragraph::new(format!(
            "{}{}: {}",
            yy_sheet::col_name(book.col),
            book.row + 1,
            entry
        )),
        footer,
    );
    app.editor_area = grid;
}

fn wrap_cell(value: &str, width: u16, height: u16) -> String {
    let mut text = String::new();
    let mut x = 0;
    let mut y = 0;
    for c in value.chars() {
        if c == '\r' {
            continue;
        }
        let c = if c == '\t' {
            '→'
        } else if c.is_control() && c != '\n' {
            '�'
        } else {
            c
        };
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) as u16;
        if c == '\n' || x + w > width {
            y += 1;
            if y >= height {
                break;
            }
            text.push('\n');
            x = 0;
            if c == '\n' {
                continue;
            }
        }
        text.push(c);
        x += w;
    }
    text
}
fn draw_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(diff) = &mut app.comparison else {
        return;
    };
    let [label, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(format!(
            "{} | {} rows, {} changed | N/P: difference",
            diff.title, diff.index.rows, diff.index.changes
        )),
        label,
    );
    let rows = match diff.rows(body.height as usize) {
        Ok(rows) => rows,
        Err(e) => {
            app.status = e.to_string();
            return;
        }
    };
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let line = |row: yy_merge_core::large::Row, is_left: bool| {
        let number = if is_left {
            row.left_line
        } else {
            row.right_line
        };
        let prefix = if number == yy_merge_core::large::NONE {
            "       ".into()
        } else {
            format!("{:>6} ", number + 1)
        };
        let color = match row.kind {
            yy_merge_core::large::Kind::LeftOnly => Color::Red,
            yy_merge_core::large::Kind::RightOnly => Color::Green,
            _ if crate::diff::changed(row.kind) => Color::Yellow,
            _ => Color::Reset,
        };
        Line::styled(
            format!("{prefix}{}", diff.text(row, is_left)),
            Style::default().fg(color),
        )
    };
    frame.render_widget(
        Paragraph::new(rows.iter().map(|r| line(*r, true)).collect::<Vec<_>>()),
        left,
    );
    frame.render_widget(
        Paragraph::new(rows.iter().map(|r| line(*r, false)).collect::<Vec<_>>()),
        right,
    );
}
