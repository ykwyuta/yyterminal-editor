//! A bounded, native Mermaid-to-terminal conversion engine. No JavaScript executes.
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
#[derive(Default)]
struct Graph {
    nodes: Vec<(String, String)>,
    edges: Vec<(usize, usize, String)>,
}
impl Graph {
    fn node(&mut self, id: &str, label: &str) -> Result<usize, String> {
        if let Some(i) = self.nodes.iter().position(|n| n.0 == id) {
            if label != id {
                self.nodes[i].1 = label.into();
            }
            return Ok(i);
        }
        if self.nodes.len() >= 64 {
            return Err("at most 64 nodes".into());
        }
        self.nodes.push((id.into(), label.into()));
        Ok(self.nodes.len() - 1)
    }
    fn edge(&mut self, a: usize, b: usize, label: String) -> Result<(), String> {
        if self.edges.len() >= 128 {
            return Err("at most 128 edges".into());
        }
        self.edges.push((a, b, label));
        Ok(())
    }
}
pub fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
fn fit(text: &str, width: usize) -> String {
    let mut s = String::new();
    let mut used = 0;
    for c in safe(text).chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        s.push(c);
        used += w;
    }
    s
}
struct Canvas {
    cells: Vec<Vec<String>>,
    width: usize,
}
impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            cells: vec![vec![" ".into(); w]; h],
            width: w,
        }
    }
    fn text(&mut self, x: usize, y: usize, text: &str) {
        if y >= self.cells.len() {
            return;
        }
        let mut x = x;
        for c in safe(text).chars() {
            let w = c.width().unwrap_or(0);
            if w == 0 {
                continue;
            }
            if x + w > self.width {
                break;
            }
            self.cells[y][x] = c.to_string();
            for n in 1..w {
                self.cells[y][x + n].clear();
            }
            x += w;
        }
    }
    fn h(&mut self, a: usize, b: usize, y: usize) {
        for x in a.min(b)..=a.max(b) {
            self.stroke(x, y, '-');
        }
    }
    fn v(&mut self, x: usize, a: usize, b: usize) {
        for y in a.min(b)..=a.max(b) {
            self.stroke(x, y, '|');
        }
    }
    fn stroke(&mut self, x: usize, y: usize, c: char) {
        if y >= self.cells.len() || x >= self.width {
            return;
        }
        let old = self.cells[y][x].as_str();
        self.cells[y][x] = if (old == "|" && c == '-') || (old == "-" && c == '|') || old == "+" {
            "+".into()
        } else {
            c.to_string()
        };
    }
    fn boxed(&mut self, x: usize, y: usize, w: usize, label: &str) {
        self.h(x, x + w - 1, y);
        self.h(x, x + w - 1, y + 2);
        self.text(x, y, "+");
        self.text(x + w - 1, y, "+");
        self.text(x, y + 2, "+");
        self.text(x + w - 1, y + 2, "+");
        self.text(x, y + 1, "|");
        self.text(x + 1, y + 1, &" ".repeat(w - 2));
        self.text(x + 2, y + 1, &fit(label, w - 4));
        self.text(x + w - 1, y + 1, "|");
    }
    fn lines(self) -> Vec<String> {
        self.cells
            .into_iter()
            .map(|r| r.concat().trim_end().to_owned())
            .collect()
    }
}
fn statements(source: &str) -> Vec<String> {
    let mut out = vec![];
    let mut s = String::new();
    let mut quote = false;
    let mut depth = 0i32;
    for c in source.chars() {
        if c == '"' {
            quote = !quote;
        }
        if !quote {
            if matches!(c, '[' | '(' | '{') {
                depth += 1;
            }
            if matches!(c, ']' | ')' | '}') {
                depth = (depth - 1).max(0);
            }
        }
        if (c == ';' && depth == 0 && !quote) || c == '\n' {
            if !s.trim().is_empty() {
                out.push(s.trim().into());
            }
            s.clear();
        } else {
            s.push(c);
        }
    }
    if !s.trim().is_empty() {
        out.push(s.trim().into());
    }
    out
}
fn node(input: &str) -> Result<(String, String, &str), String> {
    let s = input.trim_start();
    if let Some(rest) = s.strip_prefix("[*]") {
        return Ok(("[*]".into(), "(*)".into(), rest));
    }
    let n = s
        .char_indices()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    if n == 0 {
        return Err(format!("expected node near {s}"));
    }
    // An arrow directly after an identifier must not be absorbed as an ID suffix.
    let n = s[..n].find("--").or_else(|| s[..n].find("-.")).unwrap_or(n);
    let id = &s[..n];
    let mut rest = s[n..].trim_start();
    let mut label = id.to_string();
    if let Some(open) = rest.chars().next().filter(|c| matches!(c, '[' | '(' | '{')) {
        let close = match open {
            '[' => ']',
            '(' => ')',
            _ => '}',
        };
        let mut d = 0;
        let mut quoted = false;
        let mut end = None;
        for (i, c) in rest.char_indices() {
            if c == '"' {
                quoted = !quoted;
            }
            if !quoted {
                if c == open {
                    d += 1;
                }
                if c == close {
                    d -= 1;
                    if d == 0 {
                        end = Some(i);
                        break;
                    }
                }
            }
        }
        let end = end.ok_or("unclosed node label")?;
        label = rest[1..end]
            .trim_matches(['"', '[', ']', '(', ')', '{', '}'])
            .replace("<br/>", " / ")
            .replace("<br>", " / ");
        rest = &rest[end + 1..];
    }
    Ok((id.into(), label, rest))
}
fn flow(source: &[String], width: usize) -> Result<Vec<String>, String> {
    let mut g = Graph::default();
    let mut notes = vec![];
    for stmt in source.iter().skip(1) {
        let s = stmt.split("%%").next().unwrap_or("").trim();
        if s.is_empty() {
            continue;
        }
        if let Some(name) = s.strip_prefix("subgraph ") {
            notes.push(format!("[subgraph: {}]", safe(name)));
            continue;
        }
        if s == "end" {
            continue;
        }
        if s.starts_with("style ")
            || s.starts_with("classDef ")
            || s.starts_with("class ")
            || s.starts_with("linkStyle ")
        {
            notes.push(format!("[style omitted: {}]", safe(s)));
            continue;
        }
        let (id, label, mut rest) = node(s)?;
        let mut from = g.node(&id, &label)?;
        while !rest.trim().is_empty() {
            rest = rest.trim_start();
            let arrows = ["<-->", "-.->", "==>", "-->", "---", "--", "-."];
            let arrow = arrows
                .iter()
                .find(|a| rest.starts_with(**a))
                .ok_or_else(|| format!("unsupported edge: {rest}"))?;
            rest = &rest[arrow.len()..];
            let mut edge = String::new();
            if rest.trim_start().starts_with('|') {
                rest = rest.trim_start();
                let e = rest[1..].find('|').ok_or("unclosed edge label")? + 1;
                edge = rest[1..e].into();
                rest = &rest[e + 1..];
            } else if *arrow == "--" || *arrow == "-." {
                let e = rest
                    .find(if *arrow == "--" { "-->" } else { "->" })
                    .ok_or("expected labelled arrow")?;
                edge = rest[..e].trim().into();
                rest = &rest[e + if *arrow == "--" { 3 } else { 2 }..];
            }
            let (id, label, r) = node(rest)?;
            let to = g.node(&id, &label)?;
            if *arrow == "---" {
                edge = format!("[undirected] {edge}");
            }
            g.edge(from, to, edge)?;
            if *arrow == "<-->" {
                g.edge(to, from, String::new())?;
            }
            from = to;
            rest = r;
        }
    }
    let direction = source[0].split_whitespace().nth(1).unwrap_or("TB");
    let mut lines = layered(&g, width, direction).unwrap_or_else(|| layout(&g, width));
    lines.extend(notes);
    Ok(lines)
}
/// Layer DAGs once; cycles and diagrams wider than the viewport use reference cards.
fn layered(g: &Graph, width: usize, direction: &str) -> Option<Vec<String>> {
    let n = g.nodes.len();
    if n == 0 {
        return None;
    }
    let mut degree = vec![0usize; n];
    let mut rank = vec![0usize; n];
    for (_, b, _) in &g.edges {
        degree[*b] += 1;
    }
    let mut ready = std::collections::VecDeque::new();
    for (i, &d) in degree.iter().enumerate() {
        if d == 0 {
            ready.push_back(i);
        }
    }
    let mut visited = 0;
    while let Some(a) = ready.pop_front() {
        visited += 1;
        for (_, b, _) in g.edges.iter().filter(|e| e.0 == a) {
            rank[*b] = rank[*b].max(rank[a] + 1);
            degree[*b] -= 1;
            if degree[*b] == 0 {
                ready.push_back(*b);
            }
        }
    }
    if visited != n {
        return None;
    }
    let levels = rank.iter().max().copied()? + 1;
    let mut slots = vec![0usize; levels];
    let mut slot = vec![0usize; n];
    for i in 0..n {
        slot[i] = slots[rank[i]];
        slots[rank[i]] += 1;
    }
    let breadth = slots.iter().max().copied()?;
    let horizontal = matches!(direction, "LR" | "RL");
    let reverse = matches!(direction, "BT" | "RL");
    let columns = if horizontal { levels } else { breadth };
    let bw = g
        .nodes
        .iter()
        .map(|(id, label)| id.width() + label.width() + 6)
        .max()?
        .clamp(8, 30);
    let gap = 5;
    let canvas_width = columns * (bw + gap) - gap;
    if canvas_width > width {
        return None;
    }
    let height = if horizontal { breadth } else { levels } * 6 - 3;
    let pos = (0..n)
        .map(|i| {
            let level = if reverse {
                levels - 1 - rank[i]
            } else {
                rank[i]
            };
            if horizontal {
                (level * (bw + gap), slot[i] * 6)
            } else {
                (slot[i] * (bw + gap), level * 6)
            }
        })
        .collect::<Vec<_>>();
    let mut canvas = Canvas::new(canvas_width, height);
    for (a, b, label) in &g.edges {
        let (x1, y1) = pos[*a];
        let (x2, y2) = pos[*b];
        if horizontal {
            let start = if reverse { x1 } else { x1 + bw - 1 };
            let end = if reverse { x2 + bw } else { x2 - 1 };
            let mid = if reverse {
                start.saturating_sub(3)
            } else {
                start + 3
            };
            canvas.h(start, mid, y1 + 1);
            canvas.v(mid, y1 + 1, y2 + 1);
            canvas.h(mid, end, y2 + 1);
            if !label.starts_with("[undirected]") {
                canvas.text(end, y2 + 1, if reverse { "<" } else { ">" });
            }
        } else {
            let start = if reverse { y1 } else { y1 + 2 };
            let end = if reverse { y2 + 3 } else { y2 - 1 };
            let mid = if reverse {
                start.saturating_sub(2)
            } else {
                start + 2
            };
            canvas.v(x1 + bw / 2, start, mid);
            canvas.h(x1 + bw / 2, x2 + bw / 2, mid);
            canvas.v(x2 + bw / 2, mid, end);
            if !label.starts_with("[undirected]") {
                canvas.text(x2 + bw / 2, end, if reverse { "^" } else { "v" });
            }
        }
    }
    for (i, (id, label)) in g.nodes.iter().enumerate() {
        let (x, y) = pos[i];
        canvas.boxed(x, y, bw, &format!("{id}: {label}"));
    }
    let mut lines = vec!["Connections:".to_owned()];
    lines.extend(canvas.lines());
    for (id, label) in &g.nodes {
        if id.width() + label.width() + 2 > bw - 4 {
            lines.push(format!("{id}: {}", safe(label)));
        }
    }
    for (a, b, label) in &g.edges {
        if !label.is_empty() {
            lines.push(format!(
                "{} -> {}: {}",
                g.nodes[*a].0,
                g.nodes[*b].0,
                safe(label)
            ));
        }
    }
    Some(lines)
}
fn layout(g: &Graph, width: usize) -> Vec<String> {
    if g.nodes.is_empty() {
        return vec!["(empty diagram)".into()];
    }
    // One shared node catalogue plus edge cards keeps cycles/branches legible at narrow widths.
    // References repeat on edge cards; IDs make identical labels unambiguous.
    let mut out = vec![];
    for (id, label) in &g.nodes {
        let w = (label.width() + id.width() + 7).min(width).max(6);
        let mut c = Canvas::new(width, 3);
        c.boxed(0, 0, w, &format!("{id}: {label}"));
        out.extend(c.lines());
        if label.width() + id.width() + 2 > w - 4 {
            out.push(format!("{id}: {}", safe(label)));
        }
    }
    if !g.edges.is_empty() {
        out.push("Connections:".into());
    }
    for (a, b, label) in &g.edges {
        let from = &g.nodes[*a].0;
        let to = &g.nodes[*b].0;
        let w1 = (from.width() + 4).max(6);
        let w2 = (to.width() + 4).max(6);
        if w1 + w2 + 7 <= width {
            let mut c = Canvas::new(width, if label.is_empty() { 3 } else { 4 });
            let y = usize::from(!label.is_empty());
            if !label.is_empty() {
                c.text(0, 0, &fit(label, width));
            }
            c.boxed(0, y, w1, from);
            c.boxed(w1 + 7, y, w2, to);
            c.h(w1, w1 + 6, y + 1);
            if !label.starts_with("[undirected]") {
                c.text(w1 + 6, y + 1, ">");
            }
            out.extend(c.lines());
        } else {
            out.push(safe(from));
            out.push(format!("  | {}", safe(label)));
            out.push(if label.starts_with("[undirected]") {
                "  |".into()
            } else {
                "  v".into()
            });
            out.push(safe(to));
        }
    }
    out
}
fn sequence(source: &[String], width: usize) -> Result<Vec<String>, String> {
    let mut g = Graph::default();
    let mut messages = vec![];
    for s in source.iter().skip(1) {
        if s.starts_with("%%") {
            continue;
        }
        if let Some(s) = s
            .strip_prefix("participant ")
            .or_else(|| s.strip_prefix("actor "))
        {
            let (id, label) = s.split_once(" as ").unwrap_or((s, s));
            g.node(id.trim(), label.trim())?;
            continue;
        }
        if let Some((left, label)) = s.split_once(':') {
            let op = ["-->>", "->>", "-->", "->", "--x", "-x", "--)", "-)"]
                .iter()
                .find_map(|op| left.split_once(op).map(|(a, b)| (a, b, *op)));
            if let Some((a, b, op)) = op {
                let a = a.trim();
                let b = b.trim().trim_start_matches(['+', '-']);
                let from = g.node(a, a)?;
                let to = g.node(b, b)?;
                messages.push((from, to, format!("{} {}", op, label.trim())));
                continue;
            }
        }
        if !s.trim().is_empty() {
            messages.push((usize::MAX, 0, format!("[{}]", safe(s))));
        }
    }
    let col = (width / g.nodes.len().max(1)).clamp(6, 22);
    if col * g.nodes.len() > width || g.nodes.is_empty() {
        return Err("too many sequence participants for this width".into());
    }
    let mut c = Canvas::new(width, 3 + messages.len() * 3);
    let center = |i: usize| i * col + col / 2;
    for (i, (_, label)) in g.nodes.iter().enumerate() {
        c.boxed(i * col, 0, col, label);
        c.v(center(i), 3, 2 + messages.len() * 3);
    }
    for (i, (from, to, label)) in messages.iter().enumerate() {
        let y = 3 + i * 3;
        if *from == usize::MAX {
            c.text(0, y, &fit(label, width));
            continue;
        }
        let a = center(*from);
        let b = center(*to);
        c.text(a.min(b), y, &fit(label, width - a.min(b)));
        if a == b {
            c.h(a, a + 3, y + 1);
            c.text(a + 3, y + 1, "+");
            c.text(a, y + 2, "<--+");
        } else {
            c.h(a, b, y + 1);
            c.text(b, y + 1, if a < b { ">" } else { "<" });
        }
    }
    Ok(c.lines())
}
fn relations(source: &[String], width: usize, er: bool) -> Result<Vec<String>, String> {
    let mut g = Graph::default();
    let mut current = None;
    for s in source.iter().skip(1) {
        if s.starts_with("%%") {
            continue;
        }
        if let Some(name) = s.strip_suffix('{') {
            let name = name.trim().strip_prefix("class ").unwrap_or(name.trim());
            let i = g.node(name, name)?;
            current = Some(i);
            continue;
        }
        if s == "}" {
            current = None;
            continue;
        }
        if let Some(i) = current {
            g.nodes[i].1.push_str(&format!(" / {s}"));
            continue;
        }
        if er {
            let Some((relation, label)) = s.split_once(':') else {
                return Err(format!("unsupported ER statement: {s}"));
            };
            let parts = relation.split_whitespace().collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err(format!("unsupported ER relation: {s}"));
            }
            let a = g.node(parts[0], parts[0])?;
            let b = g.node(parts[2], parts[2])?;
            g.edge(a, b, format!("{} {}", parts[1], label.trim()))?;
        } else {
            let op = ["<|--", "*--", "o--", "..>", "-->", "--", "..|>", "<|.."]
                .iter()
                .find_map(|op| s.split_once(op).map(|(a, b)| (a, b, *op)));
            if let Some((a, b, op)) = op {
                let (b, label) = b.split_once(':').unwrap_or((b, ""));
                let a = a.trim();
                let b = b.trim();
                let from = g.node(a, a)?;
                let to = g.node(b, b)?;
                g.edge(from, to, format!("{op} {}", label.trim()))?;
            } else if let Some(name) = s.strip_prefix("class ") {
                g.node(name, name)?;
            } else {
                return Err(format!("unsupported class statement: {s}"));
            }
        }
    }
    let mut result = layout(&g, width);
    for (id, label) in &g.nodes {
        if label.contains(" / ") {
            result.push(format!("{id} definition:"));
            result.extend(label.split(" / ").skip(1).map(|s| format!("  {}", safe(s))));
        }
    }
    Ok(result)
}
/// Unsupported syntax keeps the original text visible instead of inventing a diagram.
pub fn render(source: &str, width: usize) -> Vec<String> {
    let width = width.clamp(6, 240);
    let stmts = statements(source);
    let kind = stmts.first().map_or("", String::as_str);
    let result = if kind.starts_with("flowchart ") || kind.starts_with("graph ") {
        flow(&stmts, width)
    } else if kind == "sequenceDiagram" {
        sequence(&stmts, width)
    } else if kind == "erDiagram" {
        relations(&stmts, width, true)
    } else if kind == "classDiagram" {
        relations(&stmts, width, false)
    } else if kind.starts_with("stateDiagram") {
        let mut s = stmts.clone();
        s[0] = "flowchart TB".into();
        for line in &mut s[1..] {
            if let Some((edge, label)) = line.split_once(':')
                && let Some((a, b)) = edge.split_once("-->")
            {
                *line = format!("{} -->|{}| {}", a.trim(), label.trim(), b.trim());
            }
        }
        flow(&s, width)
    } else {
        Err(format!("unsupported diagram type: {kind}"))
    };
    match result {
        Ok(mut lines) => {
            lines.insert(0, format!("[Mermaid / {}]", safe(kind)));
            lines
        }
        Err(error) => {
            let mut lines = vec![format!("[Mermaid source: {}]", safe(&error))];
            lines.extend(source.lines().map(safe));
            lines
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branches_cycles_chains_quotes_and_japanese() {
        let s = render(
            "flowchart LR\n A[\"開始;処理\"]-->B{判断}\n B -->|yes| C[終了]\n B -->|no| A",
            80,
        )
        .join("\n");
        assert!(s.contains("開始;処理"));
        assert!(s.contains("判断"));
        assert!(s.contains("yes"));
        assert!(s.contains("Connections:"));
        assert!(!s.contains("Mermaid source:"));
    }
    #[test]
    fn dag_direction_and_branching_have_shared_boxes() {
        for direction in ["TB", "BT", "LR", "RL"] {
            let s = render(
                &format!("flowchart {direction}\n A[Start]-->B[Left]\n A-->C[Right]"),
                100,
            )
            .join("\n");
            assert!(!s.contains("Mermaid source:"));
            assert!(s.contains("Start") && s.contains("Left") && s.contains("Right"));
            assert_eq!(s.matches("Start").count(), 1);
        }
    }
    #[test]
    fn undirected_connection_keeps_its_semantics() {
        let s = render("flowchart LR\n A---B", 80).join("\n");
        assert!(s.contains("undirected"));
        assert!(!s.contains("--->"));
    }
    #[test]
    fn sequence_and_er() {
        let s=render("sequenceDiagram\n participant A as Alice\n participant B as Bob\n A->>B: hello\n B-->>A: reply",80).join("\n");
        assert!(s.contains("Alice") && s.contains("hello") && s.contains("reply"));
        let s = render(
            "erDiagram\n CUSTOMER ||--o{ ORDER : places\n CUSTOMER {\n int id PK\n }",
            80,
        )
        .join("\n");
        assert!(s.contains("places") && s.contains("int id PK"));
    }
    #[test]
    fn unsupported_is_visible_and_controls_do_not_escape() {
        let s = render("gantt\n dateFormat YYYY-MM-DD\n bad\u{1b}[2J", 20).join("\n");
        assert!(s.contains("unsupported"));
        assert!(s.contains("dateFormat"));
        assert!(!s.contains('\u{1b}'));
    }
    #[test]
    fn node_limit_is_bounded_and_narrow_width_keeps_edges() {
        let source = format!(
            "flowchart TB\n{}",
            (0..70)
                .map(|i| format!("N{i}-->N{}\n", i + 1))
                .collect::<String>()
        );
        assert!(render(&source, 20)[0].contains("64 nodes"));
        assert!(render("flowchart TB\n A-->B", 6).join("\n").contains('v'));
    }
}
