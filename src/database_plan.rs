//! Terminal counterpart of yydbmanager's SSMS-style operator canvas.
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
};
use yy_db_core::{plan::Plan, plan_visual as visual};
pub const CARD_W: usize = 32;
pub const CARD_H: usize = 11;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub x: usize,
    pub y: usize,
    pub node: usize,
}
struct Appearance {
    parent: Option<usize>,
    warning: bool,
    heavy: bool,
    flow: String,
    badges: String,
    labels: Vec<String>,
}
pub struct Canvas {
    pub cards: Vec<Card>,
    pub width: usize,
    pub height: usize,
    appearance: Vec<Appearance>,
}
impl Canvas {
    pub fn new(plan: &Plan) -> Result<Self, String> {
        visual::validate(plan)?;
        // Reuse exactly the desktop ordering of siblings, roots and operators.
        let doc = visual::drawing(plan);
        let cards = doc
            .nodes
            .iter()
            .take(plan.nodes.len())
            .enumerate()
            .map(|(node, n)| Card {
                x: ((n.rect.x - 24.) / 272.).round().max(0.) as usize * 44,
                y: ((n.rect.y - 72.) / 180.).round().max(0.) as usize * 13 + 2,
                node,
            })
            .collect::<Vec<_>>();
        let width = cards.iter().map(|c| c.x + CARD_W).max().unwrap_or(0);
        let height = cards.iter().map(|c| c.y + CARD_H).max().unwrap_or(0);
        let map = plan
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect::<std::collections::HashMap<_, _>>();
        let appearance = plan
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let operator = visual::display_operator(plan.kind, node).to_lowercase();
                let symbol = if operator.contains("join") {
                    "[⋈]"
                } else if operator.contains("sort") {
                    "[↕]"
                } else if operator.contains("index") || operator.contains("seek") {
                    "[⌕]"
                } else if operator.contains("scan") {
                    "[▤]"
                } else if operator.contains("aggregate") {
                    "[Σ]"
                } else {
                    "[→]"
                };
                let warning = visual::warning(node).is_some();
                Appearance {
                    parent: node.parent.and_then(|id| map.get(&id).copied()),
                    warning,
                    heavy: visual::flow_width(node) >= 4.,
                    flow: visual::number(visual::flow_rows(node)),
                    badges: format!(
                        "{symbol} #{} {}{}",
                        node.id,
                        if warning { "! " } else { "" },
                        if visual::parallel(node) { "∥" } else { "" }
                    ),
                    labels: visual::labels(plan, i),
                }
            })
            .collect();
        Ok(Self {
            cards,
            width,
            height,
            appearance,
        })
    }
    pub fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        plan: &Plan,
        selected: usize,
        left: usize,
        top: usize,
    ) -> Vec<(Rect, visual::Hit)> {
        let mut hits = vec![];
        let mut painter = Painter {
            frame,
            area,
            left,
            top,
        };
        // Arrows run from input on the right towards the result on the left.
        for card in &self.cards {
            let appearance = &self.appearance[card.node];
            let Some(parent) = appearance.parent.map(|i| self.cards[i]) else {
                continue;
            };
            let (sx, sy) = (card.x, card.y + 2);
            let (tx, ty) = (parent.x + CARD_W, parent.y + 2);
            let mid = (sx + tx) / 2;
            let heavy = appearance.heavy;
            let rule = if heavy { "━" } else { "─" };
            painter.horizontal(tx, mid, ty, rule, Color::DarkGray);
            painter.vertical(
                mid,
                ty.min(sy),
                ty.max(sy),
                if heavy { "┃" } else { "│" },
                Color::DarkGray,
            );
            painter.horizontal(mid, sx, sy, rule, Color::DarkGray);
            painter.put(tx, ty, "◀", Style::default().fg(Color::Cyan));
            if let Some(rect) = painter.visible(mid, sy, card.x.saturating_sub(mid).max(1), 1) {
                hits.push((rect, visual::Hit::Edge(card.node)));
            }
            let label = &appearance.flow;
            painter.text(
                mid + 1,
                sy.saturating_sub(1),
                label,
                Style::default().fg(Color::Gray),
                10,
            );
        }
        for card in &self.cards {
            let Some(hit) = painter.visible(card.x, card.y, CARD_W, CARD_H) else {
                continue;
            };
            let appearance = &self.appearance[card.node];
            let warning = appearance.warning;
            let color = if selected == card.node {
                Color::Yellow
            } else if warning {
                Color::Red
            } else {
                Color::Cyan
            };
            let style = Style::default().fg(color);
            painter.horizontal(card.x + 1, card.x + CARD_W - 1, card.y, "─", color);
            painter.horizontal(
                card.x + 1,
                card.x + CARD_W - 1,
                card.y + CARD_H - 1,
                "─",
                color,
            );
            painter.vertical(card.x, card.y + 1, card.y + CARD_H - 1, "│", color);
            painter.vertical(
                card.x + CARD_W - 1,
                card.y + 1,
                card.y + CARD_H - 1,
                "│",
                color,
            );
            for (x, y, symbol) in [
                (card.x, card.y, "┌"),
                (card.x + CARD_W - 1, card.y, "┐"),
                (card.x, card.y + CARD_H - 1, "└"),
                (card.x + CARD_W - 1, card.y + CARD_H - 1, "┘"),
            ] {
                painter.put(x, y, symbol, style);
            }
            painter.text(
                card.x + 1,
                card.y + 1,
                &appearance.badges,
                style.add_modifier(Modifier::BOLD),
                CARD_W - 2,
            );
            for (row, label) in appearance.labels.iter().take(CARD_H - 3).enumerate() {
                painter.text(
                    card.x + 1,
                    card.y + 2 + row,
                    label,
                    if row == 0 {
                        style.add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    },
                    CARD_W - 2,
                );
            }
            hits.push((hit, visual::Hit::Node(card.node)));
        }
        // Each desktop query band is retained, including multi-statement plans.
        let mut ordinal = 0;
        for card in &self.cards {
            if plan.nodes[card.node].parent.is_none() {
                ordinal += 1;
                painter.text(
                    0,
                    card.y.saturating_sub(2),
                    &format!(
                        "Query {ordinal} · {} · {}",
                        plan.kind.name(),
                        if plan.actual {
                            "Actual (cost is estimated)"
                        } else {
                            "Estimated"
                        }
                    ),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                    100,
                );
            }
        }
        hits
    }
}
struct Painter<'a, 'b> {
    frame: &'a mut Frame<'b>,
    area: Rect,
    left: usize,
    top: usize,
}
impl Painter<'_, '_> {
    fn visible(&self, x: usize, y: usize, w: usize, h: usize) -> Option<Rect> {
        let l = x.max(self.left);
        let t = y.max(self.top);
        let r = (x + w).min(self.left + self.area.width as usize);
        let b = (y + h).min(self.top + self.area.height as usize);
        (r > l && b > t).then(|| {
            Rect::new(
                self.area.x + (l - self.left) as u16,
                self.area.y + (t - self.top) as u16,
                (r - l) as u16,
                (b - t) as u16,
            )
        })
    }
    fn put(&mut self, x: usize, y: usize, symbol: &str, style: Style) {
        if let Some(r) = self.visible(x, y, 1, 1) {
            self.frame.buffer_mut()[(r.x, r.y)]
                .set_symbol(symbol)
                .set_style(style);
        }
    }
    fn horizontal(&mut self, x: usize, end: usize, y: usize, symbol: &str, color: Color) {
        if y < self.top || y >= self.top + self.area.height as usize {
            return;
        }
        for x in x.max(self.left)..end.min(self.left + self.area.width as usize) {
            self.put(x, y, symbol, Style::default().fg(color));
        }
    }
    fn vertical(&mut self, x: usize, y: usize, end: usize, symbol: &str, color: Color) {
        if x < self.left || x >= self.left + self.area.width as usize {
            return;
        }
        for y in y.max(self.top)..end.min(self.top + self.area.height as usize) {
            self.put(x, y, symbol, Style::default().fg(color));
        }
    }
    fn text(&mut self, x: usize, y: usize, text: &str, style: Style, width: usize) {
        if y < self.top || y >= self.top + self.area.height as usize {
            return;
        }
        let line = Line::styled(crate::mermaid::safe(text), style);
        let clipped = crate::preview_layout::clip(
            &line,
            self.left.saturating_sub(x),
            width.min(
                self.left + self.area.width as usize - x.min(self.left + self.area.width as usize),
            ),
        );
        let start = x.max(self.left);
        if start < self.left + self.area.width as usize {
            self.frame.render_widget(
                Paragraph::new(clipped),
                Rect::new(
                    self.area.x + (start - self.left) as u16,
                    self.area.y + (y - self.top) as u16,
                    ((self.left + self.area.width as usize - start).min(width)) as u16,
                    1,
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_seven_database_plans_preserve_desktop_layout_and_render_small_viewports()
    -> Result<(), String> {
        for name in [
            "sqlserver-hash-sort.sqlplan",
            "postgresql-complex.yyplan",
            "mysql-complex.yyplan",
            "oracle-complex.yyplan",
            "db2-complex.yyplan",
            "sqlite-complex.yyplan",
            "duckdb-complex.yyplan",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("demo/database")
                .join(name);
            let plan = yy_db_core::plan_file::load(&path)?;
            let canvas = Canvas::new(&plan)?;
            let desktop = visual::drawing(&plan);
            assert_eq!(canvas.cards.len(), plan.nodes.len());
            for (c, n) in canvas.cards.iter().zip(&desktop.nodes) {
                assert_eq!(c.x, ((n.rect.x - 24.) / 272.).round() as usize * 44);
            }
            for (w, h) in [(120, 30), (40, 12), (8, 4)] {
                let backend = ratatui::backend::TestBackend::new(w, h);
                let mut terminal = ratatui::Terminal::new(backend).map_err(|e| e.to_string())?;
                terminal
                    .draw(|f| {
                        canvas.draw(f, f.area(), &plan, 0, 0, 0);
                    })
                    .map_err(|e| e.to_string())?;
                terminal
                    .draw(|f| {
                        canvas.draw(f, f.area(), &plan, 0, 100000, 100000);
                    })
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
