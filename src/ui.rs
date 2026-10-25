use crate::{App, HELP, Language, MENUS, MouseAction, Prompt, View, label};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use unicode_width::UnicodeWidthStr;
fn block(title: impl Into<String>, active: bool) -> Block<'static> {
    Block::bordered()
        .title(title.into())
        .border_style(Style::default().fg(if active { Color::Cyan } else { Color::DarkGray }))
}
pub(crate) fn entry_lines(app: &App, index: usize, width: u16) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let Some(path) = app.entries.get(index) else {
        return vec![];
    };
    if width == 0 {
        return vec![];
    }
    let depth = if app.tree {
        path.strip_prefix(&app.root)
            .map_or(0, |p| p.components().count().saturating_sub(1))
    } else {
        0
    };
    let indent = (depth * 2).min(width as usize / 3);
    let marker = if path.is_dir() {
        if app.tree && app.expanded.contains(path) {
            "[-] "
        } else {
            "[+] "
        }
    } else {
        "    "
    };
    let parent = !app.tree && app.root.parent() == Some(path.as_path());
    let name = if parent {
        "[..]".to_owned()
    } else {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let prefix = format!("{}{}", " ".repeat(indent), if parent { "" } else { marker });
    // Reserve at least two cells for a wide character even in very narrow panes.
    let prefix = if prefix.width() + 2 > width as usize {
        String::new()
    } else {
        prefix
    };
    let continuation = " ".repeat(prefix.width());
    let mut line = prefix;
    let mut result = Vec::new();
    for ch in name.chars() {
        let ch = if ch.is_control() { '�' } else { ch };
        let size = ch.width().unwrap_or(0);
        if line.width() + size > width as usize && line.width() > continuation.len() {
            result.push(line);
            line = continuation.clone();
        }
        line.push(ch);
    }
    result.push(line);
    result
}
pub(crate) fn draw(frame: &mut Frame, app: &mut App) {
    app.hit_cells.clear();
    app.mouse_actions.clear();
    app.explorer_area = Rect::default();
    app.palette_area = Rect::default();
    if app.palette.is_none() {
        app.palette_offset = 0;
    }
    let width = frame.area().width.max(1);
    let mut header_rows = 1u16;
    let mut used = 0u16;
    for menu in MENUS {
        let text = if app.language == Language::Japanese {
            menu.japanese
        } else {
            menu.name
        };
        let size = (text.width() as u16 + 2).min(width);
        if used + size > width {
            header_rows += 1;
            used = 0;
        }
        used += size;
    }
    let [header, tab_bar, body, status] = Layout::vertical([
        Constraint::Length(header_rows),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let (mut x, mut y) = (header.x, header.y);
    for (index, menu) in MENUS.iter().enumerate() {
        let text = if app.language == Language::Japanese {
            menu.japanese
        } else {
            menu.name
        };
        let size = (text.width() as u16 + 2).min(width);
        if x + size > header.right() {
            x = header.x;
            y += 1;
        }
        if y < header.bottom() {
            let area = Rect::new(x, y, size, 1);
            app.mouse_actions.push((area, MouseAction::Menu(index)));
            frame.render_widget(
                Paragraph::new(text).style(Style::default().fg(Color::Cyan).add_modifier(
                    if app.palette.is_some() && app.menu_group == Some(index) {
                        Modifier::REVERSED
                    } else {
                        Modifier::empty()
                    },
                )),
                area,
            );
        }
        x += size;
    }
    draw_tabs(frame, app, tab_bar);
    app.preview_area = Rect::default();
    let show_explorer = body.width >= 65 || app.explorer;
    let [left, right] = Layout::horizontal([
        Constraint::Length(if show_explorer { body.width.min(28) } else { 0 }),
        Constraint::Min(0),
    ])
    .areas(body);
    if show_explorer {
        let title = match (app.language, app.tree) {
            (Language::Japanese, true) => "エクスプローラー・ツリー",
            (Language::Japanese, false) => "エクスプローラー",
            (_, true) => "Explorer / Tree",
            (_, false) => "Explorer",
        };
        let border = block(title, app.explorer);
        let inner = border.inner(left);
        app.explorer_area = left;
        frame.render_widget(border, left);
        let mut start = app.selected.min(app.entries.len().saturating_sub(1));
        let mut height = entry_lines(app, start, inner.width).len();
        while start > 0 {
            let previous = entry_lines(app, start - 1, inner.width).len();
            if height + previous > inner.height as usize {
                break;
            }
            height += previous;
            start -= 1;
        }
        let mut y = inner.y;
        for index in start..app.entries.len() {
            for line in entry_lines(app, index, inner.width) {
                if y >= inner.bottom() || inner.width == 0 {
                    break;
                }
                let area = Rect::new(inner.x, y, inner.width, 1);
                app.mouse_actions.push((area, MouseAction::Entry(index)));
                frame.render_widget(
                    Paragraph::new(line).style(if app.explorer && index == app.selected {
                        Style::default().bg(Color::Blue)
                    } else {
                        Style::default()
                    }),
                    area,
                );
                y += 1;
            }
            if y >= inner.bottom() {
                break;
            }
        }
    }
    let title = match app.view {
        View::Editor => "Editor",
        View::Table => "Table",
        View::Sheet => "Workbook",
        View::Terminal => "Terminal",
        View::Git => "Git",
        View::Diff => "yymerge comparison",
        View::Preview => "Markdown preview",
        View::Help => "Help",
    };
    let border = block(title, !app.explorer);
    let inner = border.inner(right);
    app.content_area = inner;
    frame.render_widget(border, right);
    if inner.width > 0 && inner.height > 0 {
        match app.view {
            View::Diff => draw_diff(frame, app, inner),
            View::Sheet => draw_book(frame, app, inner),
            View::Table => draw_grid(frame, app, inner),
            View::Editor => {
                if app.preview_side && inner.width >= 24 && inner.height >= 6 {
                    let [editor, preview] = if inner.width >= 48 {
                        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                            .areas(inner)
                    } else {
                        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                            .areas(inner)
                    };
                    draw_editor(frame, app, editor);
                    let border = block("Preview", app.preview_focus);
                    let content = border.inner(preview);
                    frame.render_widget(border, preview);
                    if preview.width >= 6 {
                        let close = Rect::new(preview.right() - 5, preview.y, 3, 1);
                        frame.render_widget(Paragraph::new("[x]"), close);
                        app.mouse_actions
                            .push((close, MouseAction::Run("Split Markdown preview")));
                    }
                    draw_preview(frame, app, content);
                } else {
                    app.preview_focus = false;
                    draw_editor(frame, app, inner);
                }
            }
            View::Terminal => {
                let tab = &mut app.tabs[app.tab];
                if let Some(shell) = &mut tab.terminal {
                    if let Err(e) = shell.resize(inner.width, inner.height) {
                        app.status = e.to_string();
                    }
                    for y in 0..inner.height {
                        let row = shell.screen.view_line(tab.term_back, y as usize);
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
                    if !app.explorer && tab.term_back == 0 && shell.screen.modes().cursor_visible {
                        let (y, x) = shell.screen.cursor();
                        if x < inner.width as usize && y < inner.height as usize {
                            frame.set_cursor_position((inner.x + x as u16, inner.y + y as u16));
                        }
                    }
                }
            }
            View::Git => frame.render_widget(
                Paragraph::new(app.output.as_str())
                    .scroll((app.scroll.min(u16::MAX as usize) as u16, 0)),
                inner,
            ),
            View::Help => frame.render_widget(
                Paragraph::new(HELP)
                    .wrap(Wrap { trim: false })
                    .scroll((app.scroll.min(u16::MAX as usize) as u16, 0)),
                inner,
            ),
            View::Preview => draw_preview(frame, app, inner),
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
            (app.menu_commands().len() + 2) as u16
        } else {
            5
        };
        let w = frame.area().width.saturating_sub(4).min(72);
        let rect = Rect::new(
            frame.area().width.saturating_sub(w) / 2,
            frame.area().height.saturating_sub(h) / 2,
            w,
            h.min(frame.area().height),
        );
        app.palette_area = rect;
        frame.render_widget(Clear, rect);
        if let Some(selected) = app.palette {
            let items = app
                .menu_commands()
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    ListItem::new(label(c, app.language)).style(if i == selected {
                        Style::default().bg(Color::Blue)
                    } else {
                        Style::default()
                    })
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(selected));
            *state.offset_mut() = app.palette_offset;
            frame.render_stateful_widget(
                List::new(items).block(block(
                    if app.context_path.is_some() {
                        if app.language == Language::Japanese {
                            "開くモード"
                        } else {
                            "Open as"
                        }
                    } else if let Some(group) = app.menu_group {
                        if app.language == Language::Japanese {
                            MENUS[group].japanese
                        } else {
                            MENUS[group].name
                        }
                    } else if app.language == Language::Japanese {
                        "すべてのコマンド：矢印 / Enter / Esc"
                    } else {
                        "Commands: arrows / Enter / Esc"
                    },
                    true,
                )),
                rect,
                &mut state,
            );
            app.palette_offset = state.offset();
            let area = block("", true).inner(rect);
            for (row, i) in (state.offset()..app.menu_commands().len())
                .take(area.height as usize)
                .enumerate()
            {
                if area.width > 0 {
                    app.mouse_actions.push((
                        Rect::new(area.x, area.y + row as u16, area.width, 1),
                        MouseAction::Command(i),
                    ));
                }
            }
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
            if rect.width > 2 && rect.height >= 4 {
                let area = Rect::new(rect.x + 1, rect.bottom() - 2, rect.width - 2, 1);
                let mut x = area.x;
                for (label, action) in [
                    (
                        if matches!(app.prompt, Some(Prompt::Quit)) {
                            "[Discard]"
                        } else {
                            "[OK]"
                        },
                        MouseAction::Accept,
                    ),
                    ("[Cancel]", MouseAction::Cancel),
                ] {
                    if x < area.right() {
                        let width = (label.len() as u16).min(area.right() - x);
                        let target = Rect::new(x, area.y, width, 1);
                        frame.render_widget(
                            Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                            target,
                        );
                        app.mouse_actions.push((target, action));
                        x += width + 1;
                    }
                }
            }
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
    let drag = app.resize_drag;
    let editor = &mut app.tabs[app.tab];
    let Some(grid) = &mut editor.table else {
        frame.render_widget(Paragraph::new("Menu: CSV/TSV or Fixed-length table"), area);
        return;
    };
    let Some(page) = &grid.page else {
        frame.render_widget(
            Paragraph::new(grid.error.as_deref().unwrap_or("Loading table page...")),
            area,
        );
        return;
    };
    let budget = area.height.saturating_sub(3).max(1);
    let height =
        |r: usize| drag.map_or(grid.row_height(r), |d| d.row(r as u64, grid.row_height(r)));
    let mut top = grid.top.min(grid.row);
    while top < grid.row
        && (top..=grid.row).map(|r| u32::from(height(r))).sum::<u32>() > u32::from(budget)
    {
        top += 1;
    }
    let mut row_sizes = vec![];
    let mut remain = budget;
    for r in top..page.rows.len() {
        if remain == 0 {
            break;
        }
        let h = height(r).min(remain);
        row_sizes.push((r, h));
        remain -= h;
    }
    grid.top = top;
    let column_count = page.rows.iter().map(|r| r.fields.len()).max().unwrap_or(1);
    grid.left = grid.left.min(grid.col);
    let columns_for = |left: usize| {
        let mut used = 8;
        let mut cols = vec![];
        for c in left..column_count {
            let w = drag.map_or(grid.column_width(c), |d| {
                d.column(c as u32, grid.column_width(c))
            });
            if used + w + 1 > area.width && !cols.is_empty() {
                break;
            }
            cols.push((c, w));
            used += w + 1;
            if used >= area.width {
                break;
            }
        }
        cols
    };
    let mut columns = columns_for(grid.left);
    if !columns.iter().any(|(c, _)| *c == grid.col) {
        columns = columns_for(grid.col);
    }
    if let Some((c, _)) = columns.first() {
        grid.left = *c;
    }
    let headers = grid.mode.headers(column_count);
    let header = Row::new(
        std::iter::once("Record".to_owned()).chain(
            columns
                .iter()
                .map(|(c, _)| headers.get(*c).cloned().unwrap_or_default()),
        ),
    )
    .style(Style::default().fg(Color::Cyan));
    let (top, left, bottom, right) = grid.rectangle();
    let rows = row_sizes.iter().map(|&(r, h)| {
        let row = &page.rows[r];
        let mut cells = vec![Cell::from((grid.base + r as u64 + 1).to_string())];
        for &(c, w) in &columns {
            let cell = Cell::from(wrap_cell(&grid.value_at(row, c), w, h));
            cells.push(if r >= top && r <= bottom && c >= left && c <= right {
                cell.style(Style::default().bg(Color::Blue).fg(Color::White))
            } else {
                cell
            });
        }
        Row::new(cells).height(h)
    });
    let [label, table, detail] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    let widths = std::iter::once(Constraint::Length(7))
        .chain(columns.iter().map(|(_, w)| Constraint::Length(*w)));
    frame.render_widget(
        Paragraph::new(format!(
            "{} | Menu: edit / drag headers to resize",
            grid.mode.name()
        )),
        label,
    );
    frame.render_widget(
        Table::new(rows, widths).header(header).column_spacing(1),
        table,
    );
    let mut x = table.x + 8;
    for &(c, w) in &columns {
        if x + w < table.right() {
            let target = Rect::new(x + w, table.y, 1, 1);
            frame.render_widget(Paragraph::new("|"), target);
            app.mouse_actions
                .push((target, MouseAction::ColumnSize(c as u32, w)));
        }
        x = x.saturating_add(w + 1);
    }
    let mut y = table.y + 1;
    for &(r, h) in &row_sizes {
        let mut x = table.x + 8;
        for &(c, w) in &columns {
            if x < table.right() && y < table.bottom() {
                app.hit_cells.push((
                    Rect::new(x, y, w.min(table.right() - x), h.min(table.bottom() - y)),
                    r as u64,
                    c as u32,
                ));
            }
            x = x.saturating_add(w + 1);
        }
        if table.width > 6 && y + h - 1 < table.bottom() {
            let target = Rect::new(table.x + 6, y + h - 1, 1, 1);
            frame.render_widget(Paragraph::new("_"), target);
            app.mouse_actions
                .push((target, MouseAction::RowSize(r as u64, grid.row_height(r))));
        }
        y = y.saturating_add(h);
    }
    frame.render_widget(
        Paragraph::new(if page.notice.is_empty() {
            format!(
                "Record {} / column {} | row grip: _ / column grip: |",
                grid.base + grid.row as u64 + 1,
                grid.col + 1
            )
        } else {
            page.notice.clone()
        }),
        detail,
    );
    app.editor_area = table;
}

fn draw_book(frame: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::widgets::{Cell, Row, Table};
    let drag = app.resize_drag;
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
        .map(|r| u64::from(drag.map_or(book.row_height(r), |d| d.row(r, book.row_height(r)))))
        .sum::<u64>()
        > visible
        && book.top < book.row
    {
        book.top += 1;
    }
    let mut row_sizes = vec![];
    let mut remaining = visible as u16;
    for r in book.top..book.top + visible {
        let height = drag
            .map_or(book.row_height(r), |d| d.row(r, book.row_height(r)))
            .min(remaining);
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
        let width = drag.map_or(width, |d| d.column(col, width));
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
    let mut x = grid.x + 8;
    for &(c, w) in &columns {
        if x + w < grid.right() {
            let target = Rect::new(x + w, grid.y, 1, 1);
            frame.render_widget(Paragraph::new("|"), target);
            app.mouse_actions
                .push((target, MouseAction::ColumnSize(c, w)));
        }
        x = x.saturating_add(w + 1);
    }
    let mut y = grid.y + 1;
    for &(r, h) in &row_sizes {
        if grid.width > 6 && y + h - 1 < grid.bottom() {
            let target = Rect::new(grid.x + 6, y + h - 1, 1, 1);
            frame.render_widget(Paragraph::new("_"), target);
            app.mouse_actions
                .push((target, MouseAction::RowSize(r, book.row_height(r))));
        }
        y = y.saturating_add(h);
    }
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

fn draw_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let e = &mut app.tabs[app.tab];
    let at = e.caret();
    let mut rows = e.rows(area.height as usize, area.width as usize);
    if app.follow_caret
        && !rows
            .iter()
            .any(|r| at >= r.0 && (at < r.1 || (r.1 == e.doc.snapshot().len() && at == r.1)))
    {
        e.top = at;
        rows = e.rows(area.height as usize, area.width as usize);
    }
    let selection = e.doc.selections().primary().range();
    let show = |bytes: Vec<u8>| {
        String::from_utf8_lossy(&bytes)
            .replace('\t', "    ")
            .chars()
            .filter(|c| *c != '\r' && *c != '\n')
            .map(|c| if c.is_control() { '�' } else { c })
            .collect::<String>()
    };
    let lines = rows
        .iter()
        .map(|(start, end, text)| {
            if selection.start < *end && selection.end > *start {
                let a = selection.start.clamp(*start, *end);
                let b = selection.end.clamp(a, *end);
                let snapshot = e.doc.snapshot();
                Line::from(vec![
                    Span::raw(show(snapshot.read(*start..a))),
                    Span::styled(show(snapshot.read(a..b)), Style::default().bg(Color::Blue)),
                    Span::raw(show(snapshot.read(b..*end))),
                ])
            } else {
                Line::raw(text.clone())
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
    if !app.explorer
        && !app.preview_focus
        && let Some((row, (start, _, _))) = rows
            .iter()
            .enumerate()
            .rfind(|(_, r)| at >= r.0 && at <= r.1)
    {
        let prefix = e.doc.snapshot().read(*start..at);
        let text = String::from_utf8_lossy(&prefix)
            .replace('\t', "    ")
            .replace('\r', "");
        let x =
            UnicodeWidthStr::width(text.as_str()).min(area.width.saturating_sub(1) as usize) as u16;
        frame.set_cursor_position((area.x + x, area.y + row as u16));
    }
    app.rows = rows;
    app.editor_area = area;
}

fn draw_preview(frame: &mut Frame, app: &mut App, area: Rect) {
    app.preview_area = area;
    if area.width == 0 || area.height == 0 {
        return;
    }
    app.request_preview(area.width);
    if !app.preview_pending {
        app.scroll = app.scroll.min(
            app.preview
                .lines
                .len()
                .saturating_sub(usize::from(area.height)),
        );
        app.preview_left = app.preview_left.min(
            app.preview
                .max_left()
                .saturating_sub(usize::from(area.width)),
        );
    }
    frame.render_widget(
        Paragraph::new(if app.preview_pending {
            vec![Line::raw("Rendering Markdown...")]
        } else {
            app.preview
                .viewport(app.scroll, app.preview_left, area.width, area.height)
        }),
        area,
    );
}
fn draw_tabs(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    app.tab_offset = app.tab_offset.min(app.tabs.len().saturating_sub(1));
    let room = area.width.saturating_sub(6);
    let labels = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let name = if e.terminal.is_some() {
                format!("Terminal {}", i + 1)
            } else {
                e.path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or("Untitled".into())
            };
            let mut shown = String::new();
            for c in name.chars() {
                if UnicodeWidthStr::width(shown.as_str())
                    + unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
                    > 24
                {
                    break;
                }
                shown.push(c);
            }
            format!("[{}{}]", shown, if e.modified() { " *" } else { "" })
        })
        .collect::<Vec<_>>();
    let visible = |offset: usize| {
        let mut used = 0;
        let mut out = vec![];
        for (i, label) in labels.iter().enumerate().skip(offset) {
            let w = (UnicodeWidthStr::width(label.as_str()) as u16 + 3).min(room);
            if w == 0 || used + w > room {
                break;
            }
            out.push((i, w));
            used += w;
        }
        out
    };
    let mut shown = visible(app.tab_offset);
    if !app.tab_manual_scroll && !shown.iter().any(|(i, _)| *i == app.tab) {
        app.tab_offset = app.tab;
        shown = visible(app.tab_offset);
    }
    frame.render_widget(
        Paragraph::new("[<]"),
        Rect::new(area.x, area.y, 3.min(area.width), 1),
    );
    app.mouse_actions.push((
        Rect::new(area.x, area.y, 3.min(area.width), 1),
        MouseAction::TabPage(false),
    ));
    let mut x = area.x + 3.min(area.width);
    for (i, w) in shown {
        let label_width = w.saturating_sub(3);
        if label_width > 0 {
            let rect = Rect::new(x, area.y, label_width, 1);
            frame.render_widget(
                Paragraph::new(labels[i].clone()).style(if i == app.tab {
                    Style::default().bg(Color::Blue)
                } else {
                    Style::default()
                }),
                rect,
            );
            app.mouse_actions.push((rect, MouseAction::Tab(i)));
        }
        let close = Rect::new(x + label_width, area.y, 3.min(w), 1);
        frame.render_widget(Paragraph::new(" x "), close);
        app.mouse_actions.push((close, MouseAction::CloseTab(i)));
        x += w;
    }
    if area.width >= 6 {
        let rect = Rect::new(area.right() - 3, area.y, 3, 1);
        frame.render_widget(Paragraph::new("[>]"), rect);
        app.mouse_actions.push((rect, MouseAction::TabPage(true)));
    }
}
