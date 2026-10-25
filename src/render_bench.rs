//! Manual, reproducible end-to-end rendering measurements (no flaky timing assertions).
use super::*;
use ratatui::{Terminal, backend::TestBackend};
use yy_db_core::{DbKind, Value};
fn app(root: &std::path::Path) -> App {
    let mut a = App::new(root.to_owned());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while a.busy {
        a.poll();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    a.tabs[0].syntax_language = Some("none".into());
    a.follow_caret = false;
    a
}
fn configure(a: &mut App, name: &str, root: &std::path::Path) -> Result<()> {
    match name {
        "editor_ascii" => {
            a.tabs[0].doc = yy_core::Document::from_text(
                &"let value = 123456; // plain long line ".repeat(50000),
            );
        }
        "editor_unicode" => {
            a.tabs[0].doc = yy_core::Document::from_text(
                &"日本語 e\u{301} 👩‍💻\t値の表示とスクロール\n".repeat(20000),
            );
        }
        "markdown" => {
            a.tabs[0].doc=yy_core::Document::from_text(&"## Heading\n\nParagraph **bold** and `code`. 日本語の段落。\n\n- First item\n- Second item\n\n| Name | Value |\n|---|---:|\n| Tokyo | 123.45 |\n\n".repeat(500));
            a.view = View::Preview;
        }
        "markdown_images" => {
            let path = root.join("gradient.png");
            image::RgbImage::from_fn(128, 64, |x, y| {
                image::Rgb([(x * 2) as u8, (y * 4) as u8, 128])
            })
            .save(&path)?;
            let md = format!(
                "# Image\n\n![gradient]({})\n\n",
                path.to_string_lossy().replace('\\', "/")
            );
            a.tabs[0].doc = yy_core::Document::from_text(&md.repeat(12));
            a.view = View::Preview;
        }
        "csv" | "fixed" => {
            let mut g = if name == "csv" {
                data::Grid::new(data::Mode::Delimited(yy_delimited::Dialect::csv()))
            } else {
                let spec = yy_sheet::fixed::FixedSpec::new(
                    "01 REC.\n 05 ID PIC 9(8).\n 05 NAME PIC X(24).\n 05 VALUE PIC 9(8).\n",
                    yy_cobol::Codec::new(yy_cobol::Charset::Ms932),
                    yy_sheet::fixed::RecordSep::None,
                )
                .map_err(anyhow::Error::msg)?;
                data::Grid::new(data::Mode::Fixed(std::sync::Arc::new(spec)))
            };
            let text = if name == "csv" {
                "1,Tokyo,\"quoted,text\",123.45,Japan,2026-01-01\n".repeat(10000)
            } else {
                "00000001Tokyo                   00000123".repeat(10000)
            };
            a.tabs[0].doc = yy_core::Document::from_text(&text);
            g.page = Some(data::read_page(
                a.tabs[0].doc.snapshot(),
                a.tabs[0].doc.version(),
                &g.mode,
                0,
            )?);
            a.tabs[0].table = Some(g);
            a.view = View::Table;
        }
        "sheet" => {
            let mut b = workbook::Book::empty()?;
            for r in 0..300 {
                for c in 0..16 {
                    b.doc.book.sheets[0]
                        .set(
                            &b.doc.ctx,
                            r,
                            c,
                            if c % 2 == 0 {
                                yy_sheet::Value::Number((r * 16 + c as u64) as f64)
                            } else {
                                yy_sheet::Value::Text(format!("東京 {r}:{c}").into())
                            },
                        )
                        .map_err(anyhow::Error::msg)?;
                }
            }
            a.tabs[0].book = Some(b);
            a.tabs[0].spreadsheet = true;
            a.view = View::Sheet;
        }
        "diff_unicode_sparse" | "diff_dense_insertions" => {
            let left = (0..10000)
                .map(|i| format!("{i} 日本語 👩‍💻\r\n"))
                .collect::<String>();
            let right = if name == "diff_unicode_sparse" {
                left.replace("7000 日本語", "7000 変更")
            } else {
                left.replace("\r\n", "\r\ninserted\n")
            };
            a.comparison = Some(diff::Comparison::new(
                yy_buffer::Snapshot::from_bytes(left.into_bytes()),
                yy_buffer::Snapshot::from_bytes(right.into_bytes()),
                name.into(),
            )?);
            a.view = View::Diff;
        }
        "diff_long_lines" => {
            let l = yy_buffer::Snapshot::from_bytes(
                ("long-line-".repeat(500) + "\n").repeat(1000).into_bytes(),
            );
            let r = yy_buffer::Snapshot::from_bytes(
                ("long-LINE-".repeat(500) + "\n").repeat(1000).into_bytes(),
            );
            a.comparison = Some(diff::Comparison::new(l, r, "large lines".into())?);
            a.view = View::Diff;
        }
        "terminal_colors" => {
            let mut sh = shell::Shell::new(root)?;
            sh.screen.resize(160, 48);
            let out = (0..2000)
                .map(|i| {
                    format!(
                        "\x1b[38;2;{};100;180m{i:05} {}\x1b[0m\r\n",
                        i % 255,
                        "terminal 日本語 ".repeat(8)
                    )
                })
                .collect::<String>();
            sh.screen.feed(out.as_bytes());
            a.tabs[0].terminal = Some(sh);
            a.view = View::Terminal;
        }
        "db_tree" | "db_results_long_cells" | "er_200_tables" | "plan_511_nodes" => {
            a.ensure_db();
            a.view = View::Database;
            let d = a.db.as_mut().unwrap();
            if name == "db_tree" {
                d.objects = (0..20000)
                    .map(|i| yy_db_core::catalog::Object {
                        schema: "public".into(),
                        kind: "TABLE".into(),
                        name: format!("table_{i:05}_long_name"),
                    })
                    .collect();
                d.expanded.insert("public".into());
                d.pane = db_app::Pane::Results;
            } else if name == "db_results_long_cells" {
                d.summaries = vec![yyterminal_editor::database::Summary {
                    columns: (0..8)
                        .map(|i| yy_db_core::Column {
                            name: format!("value_{i}"),
                            type_name: "VARCHAR".into(),
                        })
                        .collect(),
                    count: 128,
                }];
                d.page = Some(yyterminal_editor::database::Page {
                    set: 0,
                    start: 0,
                    rows: (0..128)
                        .map(|r| {
                            (0..8)
                                .map(|c| {
                                    Value::Text(
                                        format!("{r}:{c} ") + &"日本語 Long text ".repeat(1000),
                                    )
                                })
                                .collect()
                        })
                        .collect(),
                });
                d.pane = db_app::Pane::Results;
            } else if name == "er_200_tables" {
                let sample = yy_db_core::schema::Entity {
                    size: None,
                    id: yy_db_core::schema::TableId {
                        schema: "public".into(),
                        name: "table".into(),
                    },
                    logical_name: String::new(),
                    view: false,
                    fields: (0..16)
                        .map(|i| yy_db_core::schema::Field {
                            important: false,
                            name: format!("column_{i}"),
                            logical_name: String::new(),
                            data_type: "VARCHAR(100)".into(),
                            nullable: true,
                            default_sql: None,
                        })
                        .collect(),
                    keys: vec![],
                    position: serde_json::from_str(r#"{"x":0,"y":0}"#)?,
                    visible: true,
                    indexes: vec![],
                    catalog_definition: true,
                };
                let entities = (0..200)
                    .map(|i| {
                        let mut e = sample.clone();
                        e.id.name = format!("table_{i}");
                        e
                    })
                    .collect();
                d.model = Some(yy_db_core::schema::Model {
                    version: 4,
                    kind: DbKind::PostgreSql,
                    entities,
                    relations: vec![],
                    names: Default::default(),
                    level: Default::default(),
                });
                d.pane = db_app::Pane::Er;
            } else {
                let plan = yy_db_core::plan::Plan {
                    version: 1,
                    kind: DbKind::PostgreSql,
                    actual: false,
                    raw: String::new(),
                    nodes: (0..511)
                        .map(|i| yy_db_core::plan::Node {
                            id: i,
                            parent: if i == 0 { None } else { Some((i - 1) / 2) },
                            operator: "Hash Join".into(),
                            object: "public.orders".into(),
                            estimated_rows: Some(1200.),
                            actual_rows: None,
                            cost: Some(20.),
                            properties: Default::default(),
                        })
                        .collect(),
                };
                d.canvas = Some(
                    yyterminal_editor::database_plan::Canvas::new(&plan)
                        .map_err(anyhow::Error::msg)?,
                );
                d.plan = Some(plan);
                d.pane = db_app::Pane::Plan;
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}
fn advance(a: &mut App, name: &str, i: usize) {
    match name {
        "editor_ascii" | "editor_unicode" => {
            a.tabs[0].scroll_rows(true, 3, a.editor_area.width as usize)
        }
        "markdown" | "markdown_images" => a.scroll = i * 3,
        "csv" | "fixed" => {
            let g = a.tabs[0].table.as_mut().unwrap();
            g.row = (i * 3) % 100;
        }
        "sheet" => a.tabs[0].book.as_mut().unwrap().row = (i * 3) as u64,
        "diff_long_lines" | "diff_unicode_sparse" | "diff_dense_insertions" => {
            a.comparison.as_mut().unwrap().top = (i * 3) as u64
        }
        "terminal_colors" => a.tabs[0].term_back = i * 3,
        "db_tree" => a.db.as_mut().unwrap().tree_offset = i * 3,
        "db_results_long_cells" => {
            let d = a.db.as_mut().unwrap();
            d.top = (i * 3 % 80) as u64;
            d.row = d.top;
        }
        "er_200_tables" => a.db.as_mut().unwrap().property_top = i * 100,
        "plan_511_nodes" => {
            let d = a.db.as_mut().unwrap();
            d.plan_top = i * 13;
            d.plan_left = 44 * (i % 6);
        }
        _ => {}
    }
}
#[test]
#[ignore = "manual multi-scenario rendering benchmark"]
fn drawing_scenarios_benchmark() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut results = vec![];
    for name in [
        "editor_ascii",
        "editor_unicode",
        "markdown",
        "markdown_images",
        "csv",
        "fixed",
        "sheet",
        "diff_long_lines",
        "diff_unicode_sparse",
        "diff_dense_insertions",
        "terminal_colors",
        "db_tree",
        "db_results_long_cells",
        "er_200_tables",
        "plan_511_nodes",
    ] {
        for (width, height) in [(160, 48), (60, 18)] {
            let mut a = app(dir.path());
            configure(&mut a, name, dir.path())?;
            let mut t = Terminal::new(TestBackend::new(width, height))?;
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                t.draw(|f| ui::draw(f, &mut a))?;
                a.poll();
                if !a.preview_pending && !a.busy {
                    break;
                }
                anyhow::ensure!(
                    std::time::Instant::now() < deadline,
                    "Scenario warmup: {name}: {}",
                    a.status
                );
                std::thread::yield_now();
            }
            let mut times = vec![];
            for i in 0..40 {
                let now = std::time::Instant::now();
                advance(&mut a, name, i);
                t.draw(|f| ui::draw(f, &mut a))?;
                times.push(now.elapsed().as_secs_f64() * 1000.);
            }
            times.sort_by(f64::total_cmp);
            let row = serde_json::json!({"scenario":name,"width":width,"height":height,"frames":times.len(),"median_ms":times[20],"p95_ms":times[38],"max_ms":times[39]});
            eprintln!("RENDER {row}");
            results.push(row);
        }
    }
    if let Some(path) = std::env::var_os("YYTE_BENCH_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&results)?)?;
    }
    Ok(())
}
