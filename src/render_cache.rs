//! Versioned text layout: wrapping happens once; scrolling touches visible rows only.
use crate::preview::Rendered;
use ratatui::text::Line;
#[derive(Default)]
pub struct TextViewport {
    key: Option<([u64; 3], u16)>,
    rendered: Rendered,
    raster: RasterViewport,
}
impl TextViewport {
    pub fn prepare(
        &mut self,
        key: [u64; 3],
        width: u16,
        build: impl FnOnce() -> Vec<Line<'static>>,
    ) {
        if self.key == Some((key, width)) {
            return;
        }
        let mut lines = vec![];
        for line in build() {
            let remaining = crate::preview::MAX_LINES.saturating_sub(lines.len());
            let (wrapped, cut) =
                crate::preview_layout::wrap(line, usize::from(width.max(1)), remaining);
            lines.extend(wrapped);
            if cut {
                lines.push(Line::raw(
                    "[Display limit reached; export the complete data]",
                ));
                break;
            }
        }
        self.rendered = Rendered {
            width: lines.iter().map(Line::width).max().unwrap_or(0),
            lines,
        };
        self.key = Some((key, width));
        self.raster.clear();
    }
    pub fn paint(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, top: usize) {
        self.raster.paint(frame, area, &self.rendered, top, 0);
    }
    pub fn viewport(&self, top: usize, width: u16, height: u16) -> Vec<Line<'_>> {
        self.rendered.viewport(top, 0, width, height)
    }
}
/// Bounded cache of encoded terminal cells; no grapheme processing for reused rows.
#[derive(Default)]
pub struct RasterViewport {
    geometry: Option<(u16, usize)>,
    rows: std::collections::HashMap<usize, Vec<ratatui::buffer::Cell>>,
    order: std::collections::VecDeque<usize>,
}
impl RasterViewport {
    pub fn clear(&mut self) {
        self.geometry = None;
        self.rows.clear();
        self.order.clear();
    }
    pub fn paint(
        &mut self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        source: &Rendered,
        top: usize,
        left: usize,
    ) {
        use ratatui::{
            buffer::Buffer,
            widgets::{Paragraph, Widget},
        };
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.geometry != Some((area.width, left)) {
            self.clear();
            self.geometry = Some((area.width, left));
        }
        let capacity = (65536 / usize::from(area.width)).clamp(1, 512);
        for dy in 0..area.height {
            let row = top.saturating_add(usize::from(dy));
            if row >= source.lines.len() {
                break;
            }
            if !self.rows.contains_key(&row) {
                while self.rows.len() >= capacity {
                    if let Some(old) = self.order.pop_front() {
                        self.rows.remove(&old);
                    } else {
                        break;
                    }
                }
                let rect = ratatui::layout::Rect::new(0, 0, area.width, 1);
                let mut buffer = Buffer::empty(rect);
                buffer.set_style(rect, source.lines[row].style);
                Paragraph::new(source.viewport(row, left, area.width, 1)).render(rect, &mut buffer);
                self.rows.insert(row, buffer.content);
                self.order.push_back(row);
            }
            for (dx, cell) in self.rows[&row].iter().enumerate() {
                frame.buffer_mut()[(area.x + dx as u16, area.y + dy)] = cell.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scroll_beyond_u16_is_indexed_and_version_width_changes_rebuild() {
        let mut c = TextViewport::default();
        c.prepare([1, 0, 0], 20, || {
            (0..70010).map(|i| Line::raw(format!("row {i}"))).collect()
        });
        c.prepare([1, 0, 0], 20, || panic!("unchanged layout rebuilt"));
        assert_eq!(c.viewport(70000, 20, 2)[0].to_string(), "row 70000");
        c.prepare([2, 0, 0], 20, || vec![Line::raw("changed")]);
        assert_eq!(c.viewport(0, 20, 1)[0].to_string(), "changed");
        c.prepare([2, 0, 0], 4, || vec![Line::raw("日本語")]);
        assert_eq!(
            c.viewport(0, 4, 2)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["日本", "語"]
        );
    }
    #[test]
    fn cached_cells_match_paragraph_through_scroll_resize_and_clear() {
        use ratatui::{
            Terminal,
            backend::TestBackend,
            style::{Color, Style},
            widgets::Paragraph,
        };
        let mut source = Rendered {
            width: 30,
            lines: (0..800)
                .map(|n| {
                    Line::styled(
                        format!("{n} 日本語 e\u{301} 👩‍💻"),
                        Style::default().fg(Color::Yellow).bg(Color::Blue),
                    )
                })
                .collect(),
        };
        let mut cache = RasterViewport::default();
        for (width, top, left) in [
            (30, 0, 0),
            (30, 1, 0),
            (30, 700, 0),
            (8, 700, 2),
            (1, 1, 1),
            (30, 0, 0),
        ] {
            let mut actual = Terminal::new(TestBackend::new(width, 8)).unwrap();
            let mut expected = Terminal::new(TestBackend::new(width, 8)).unwrap();
            actual
                .draw(|f| cache.paint(f, f.area(), &source, top, left))
                .unwrap();
            expected
                .draw(|f| {
                    for dy in 0..8 {
                        f.buffer_mut().set_style(
                            ratatui::layout::Rect::new(0, dy, width, 1),
                            source.lines[top + usize::from(dy)].style,
                        );
                    }
                    f.render_widget(
                        Paragraph::new(source.viewport(top, left, width, 8)),
                        f.area(),
                    )
                })
                .unwrap();
            assert_eq!(actual.backend().buffer(), expected.backend().buffer());
            assert!(cache.rows.len() <= 512);
        }
        source.lines[0] = Line::raw("changed");
        cache.clear();
        let mut actual = Terminal::new(TestBackend::new(30, 8)).unwrap();
        actual
            .draw(|f| cache.paint(f, f.area(), &source, 0, 0))
            .unwrap();
        assert_eq!(actual.backend().buffer()[(0, 0)].symbol(), "c");
    }
}
