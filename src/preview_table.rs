//! Spreadsheet-style, read-only Markdown table layout.
use crate::{
    preview::Output,
    preview_layout::{append, wrap},
};
use pulldown_cmark::{Alignment, Event, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
#[derive(Default)]
struct Cell {
    lines: Vec<Line<'static>>,
}
impl Cell {
    fn text(&mut self, value: &str, style: Style) {
        if self.lines.is_empty() {
            self.lines.push(Line::default());
        }
        append(
            &mut self.lines.last_mut().unwrap().spans,
            &crate::mermaid::safe(value),
            style,
        );
    }
    fn newline(&mut self) {
        self.lines.push(Line::default());
    }
    fn width(&self) -> usize {
        self.lines.iter().map(Line::width).max().unwrap_or(0)
    }
    fn numeric(&self) -> bool {
        if self.lines.len() != 1 {
            return false;
        }
        let text = self.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        let text = text.replace(',', "");
        !text.trim().is_empty() && text.trim().parse::<f64>().is_ok()
    }
}
pub(crate) struct Table {
    align: Vec<Alignment>,
    rows: Vec<Vec<Cell>>,
    bold: usize,
    italic: usize,
    strike: usize,
    link: usize,
}
impl Table {
    pub(crate) fn new(align: Vec<Alignment>) -> Self {
        Self {
            align,
            rows: vec![],
            bold: 0,
            italic: 0,
            strike: 0,
            link: 0,
        }
    }
    fn style(&self) -> Style {
        let mut s = Style::default();
        for (count, modifier) in [
            (self.bold, Modifier::BOLD),
            (self.italic, Modifier::ITALIC),
            (self.strike, Modifier::CROSSED_OUT),
            (self.link, Modifier::UNDERLINED),
        ] {
            if count > 0 {
                s = s.add_modifier(modifier);
            }
        }
        if self.link > 0 {
            s = s.fg(Color::Cyan);
        }
        s
    }
    pub(crate) fn event(&mut self, event: Event<'_>) {
        let style = self.style();
        match event {
            Event::Start(Tag::TableHead | Tag::TableRow) => self.rows.push(vec![]),
            Event::Start(Tag::TableCell) => {
                if let Some(row) = self.rows.last_mut() {
                    row.push(Cell::default());
                }
            }
            Event::Start(Tag::Strong) => self.bold += 1,
            Event::End(TagEnd::Strong) => self.bold = self.bold.saturating_sub(1),
            Event::Start(Tag::Emphasis) => self.italic += 1,
            Event::End(TagEnd::Emphasis) => self.italic = self.italic.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => self.strike += 1,
            Event::End(TagEnd::Strikethrough) => self.strike = self.strike.saturating_sub(1),
            Event::Start(Tag::Link { .. }) => self.link += 1,
            Event::End(TagEnd::Link) => self.link = self.link.saturating_sub(1),
            other => {
                let Some(cell) = self.rows.last_mut().and_then(|row| row.last_mut()) else {
                    return;
                };
                match other {
                    Event::Text(t) | Event::InlineMath(t) => cell.text(&t, style),
                    Event::Code(t) => cell.text(&t, style.fg(Color::Yellow)),
                    Event::SoftBreak | Event::HardBreak => cell.newline(),
                    Event::InlineHtml(t) | Event::Html(t)
                        if matches!(
                            t.trim().to_ascii_lowercase().as_str(),
                            "<br>" | "<br/>" | "<br />"
                        ) =>
                    {
                        cell.newline()
                    }
                    Event::InlineHtml(t) | Event::Html(t) => {
                        cell.text(&format!("[HTML: {t}]"), style.fg(Color::DarkGray))
                    }
                    Event::Start(Tag::Image { .. }) => cell.text("[Image: ", style),
                    Event::End(TagEnd::Image) => cell.text("]", style),
                    _ => {}
                }
            }
        }
    }
    pub(crate) fn render(self, output: &mut Output) {
        let cols = self.align.len();
        if cols == 0 {
            return;
        }
        let gutter = self.rows.len().saturating_sub(1).to_string().len().max(1) + 2;
        let desired = (0..cols)
            .map(|c| {
                self.rows
                    .iter()
                    .filter_map(|row| row.get(c))
                    .map(Cell::width)
                    .max()
                    .unwrap_or(4)
                    .clamp(4, 64)
            })
            .collect::<Vec<_>>();
        let mut widths = vec![4; cols];
        let mut spare = output.width.saturating_sub(gutter + 2 + cols * 7);
        for level in 4..64 {
            for (i, width) in widths.iter_mut().enumerate() {
                if spare == 0 {
                    break;
                }
                if desired[i] > level {
                    *width += 1;
                    spare -= 1;
                }
            }
            if spare == 0 {
                break;
            }
        }
        let border = |left: char, cross: char, right: char, fill: char| {
            let mut s = format!("{left}{}{cross}", fill.to_string().repeat(gutter));
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&fill.to_string().repeat(w + 2));
                s.push(if i + 1 == cols { right } else { cross });
            }
            Line::styled(s, Style::default().fg(Color::DarkGray))
        };
        output.grid(border('┌', '┬', '┐', '─'));
        let mut letters = vec![Span::styled(
            format!("│{}│", " ".repeat(gutter)),
            Style::default().fg(Color::DarkGray),
        )];
        for (i, w) in widths.iter().enumerate() {
            let name = column_name(i);
            letters.push(Span::styled(
                format!(" {:^width$} ", name, width = w),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            letters.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
        }
        output.grid(Line::from(letters));
        output.grid(border('├', '┼', '┤', '─'));
        for (r, row) in self.rows.iter().enumerate() {
            if output.full() {
                break;
            }
            let cells = (0..cols)
                .map(|c| {
                    let mut lines = vec![];
                    if let Some(cell) = row.get(c) {
                        for line in &cell.lines {
                            let (wrapped, _) = wrap(
                                line.clone(),
                                widths[c],
                                output.remaining().saturating_sub(lines.len()),
                            );
                            lines.extend(wrapped);
                            if lines.len() >= output.remaining() {
                                break;
                            }
                        }
                    }
                    lines
                })
                .collect::<Vec<_>>();
            let numeric = (0..cols)
                .map(|c| r > 0 && row.get(c).is_some_and(Cell::numeric))
                .collect::<Vec<_>>();
            let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for y in 0..height {
                if output.full() {
                    break;
                }
                let number = if r == 0 && y == 0 {
                    "#".to_owned()
                } else if r > 0 && y == 0 {
                    r.to_string()
                } else {
                    String::new()
                };
                let edge = Style::default().fg(Color::DarkGray);
                let mut spans = vec![Span::styled(
                    format!("│ {number:>width$} │", width = gutter - 2),
                    edge,
                )];
                for c in 0..cols {
                    let mut style = if r == 0 {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else if r % 2 == 0 {
                        Style::default().bg(Color::Rgb(24, 28, 36))
                    } else {
                        Style::default()
                    };
                    // Applying a row style retains bold, inline code and links in each cell.
                    if r == 0 {
                        style = style.bg(Color::Rgb(30, 42, 58));
                    }
                    let line = cells[c].get(y);
                    let used = line.map_or(0, Line::width);
                    let padding = widths[c].saturating_sub(used);
                    let alignment = self.align[c];
                    let before = match alignment {
                        Alignment::Right => padding,
                        Alignment::Center => padding / 2,
                        Alignment::None if numeric[c] => padding,
                        _ => 0,
                    };
                    spans.push(Span::styled(" ".repeat(before + 1), style));
                    if let Some(line) = line {
                        for span in &line.spans {
                            spans.push(span.clone().style(style.patch(span.style)));
                        }
                    }
                    spans.push(Span::styled(" ".repeat(padding - before + 1), style));
                    spans.push(Span::styled("│", edge));
                }
                output.grid(Line::from(spans));
            }
            if r + 1 < self.rows.len() {
                output.grid(if r == 0 {
                    border('╞', '╪', '╡', '═')
                } else {
                    border('├', '┼', '┤', '─')
                });
            }
        }
        output.grid(border('└', '┴', '┘', '─'));
    }
}
fn column_name(mut index: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    s
}
