//! Grapheme-aware preview layout and bounded viewport extraction.
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

pub(crate) fn append(spans: &mut Vec<Span<'static>>, symbol: &str, style: Style) {
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(symbol);
    } else {
        spans.push(Span::styled(symbol.to_owned(), style));
    }
}
pub(crate) fn wrap(line: Line<'static>, width: usize, limit: usize) -> (Vec<Line<'static>>, bool) {
    if limit == 0 {
        return (vec![], true);
    }
    let width = width.max(1);
    if line.width() <= width {
        return (vec![line], false);
    }
    let mut result = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let mut last_break: Option<(usize, usize, usize)> = None;
    for g in line.styled_graphemes(Style::default()) {
        let size = g.symbol.width();
        while used + size > width && used > 0 {
            if let Some((span_index, byte, break_width)) = last_break.take() {
                let mut tail = spans.split_off(span_index + 1);
                let style = spans[span_index].style;
                let suffix = spans[span_index].content.to_mut().split_off(byte);
                if !suffix.is_empty() {
                    tail.insert(0, Span::styled(suffix, style));
                }
                result.push(Line::from(std::mem::replace(&mut spans, tail)));
                used = used.saturating_sub(break_width);
            } else {
                result.push(Line::from(std::mem::take(&mut spans)));
                used = 0;
            }
            if result.len() == limit {
                return (result, true);
            }
        }
        append(&mut spans, g.symbol, g.style);
        used += size;
        if g.symbol.chars().all(char::is_whitespace) {
            last_break = Some((spans.len() - 1, spans.last().unwrap().content.len(), used));
        }
    }
    if !spans.is_empty() {
        result.push(Line::from(spans));
    }
    (result, false)
}
pub(crate) fn clip(line: &Line<'_>, left: usize, width: usize) -> Line<'static> {
    let mut spans = vec![];
    let mut position = 0usize;
    let right = left.saturating_add(width);
    for g in line.styled_graphemes(Style::default()) {
        let size = g.symbol.width();
        if position >= right {
            break;
        }
        let end = position + size;
        if position >= left && end <= right {
            append(&mut spans, g.symbol, g.style);
        } else if end > left && position < right {
            append(
                &mut spans,
                &" ".repeat(end.min(right) - position.max(left)),
                g.style,
            );
        }
        position = end;
    }
    Line::from(spans).style(line.style)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapping_keeps_graphemes_styles_spaces_and_word_boundaries() {
        let original = "alpha beta e\u{301}👩‍💻 日本語abcdefghijklmnop";
        let (lines, clipped) = wrap(
            Line::styled(original, Style::default().fg(ratatui::style::Color::Cyan)),
            10,
            100,
        );
        assert!(!clipped);
        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<String>(),
            original
        );
        assert!(lines.iter().all(|line| line.width() <= 10));
        assert!(lines.iter().any(|line| line.to_string().contains("👩‍💻")));
        assert!(lines.iter().all(|line| {
            line.spans
                .iter()
                .all(|s| s.style.fg == Some(ratatui::style::Color::Cyan))
        }));
        assert!(lines[0].to_string().starts_with("alpha"));
    }
    #[test]
    fn horizontal_clipping_handles_wide_character_edges_and_large_offsets() {
        assert_eq!(clip(&Line::raw("日本語"), 1, 3).to_string(), " 本");
        let line = Line::raw("x".repeat(70_000) + "TARGET");
        assert_eq!(clip(&line, 70_000, 6).to_string(), "TARGET");
    }
}
