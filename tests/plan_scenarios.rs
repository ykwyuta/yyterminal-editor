//! Live engine plans: topology, every terminal card/edge, clipping and file round trips.
use anyhow::{Context, Result, anyhow, bail, ensure};
use ratatui::{Terminal, backend::TestBackend};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};
use yy_db_core::{
    DbKind,
    plan::Plan,
    plan_visual::{self, Hit},
};
use yyterminal_editor::{
    database::{Profile, Reply, Request, Worker},
    database_plan::{CARD_H, CARD_W, Canvas},
};

#[derive(Deserialize)]
struct Scenario {
    name: String,
    sql: String,
}
fn call(worker: &Worker, request: Request) -> Result<Reply> {
    worker.send(request).map_err(|e| anyhow!(e))?;
    worker
        .rx
        .recv_timeout(Duration::from_secs(120))
        .context("Database worker deadline")?
        .map_err(|e| anyhow!(e))
}
fn execute(worker: &Worker, sql: &str) -> Result<()> {
    call(worker, Request::Execute(sql.into()))?;
    Ok(())
}
fn text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn render(plan: &Plan, directory: &Path) -> Result<()> {
    plan_visual::validate(plan).map_err(|e| anyhow!(e))?;
    let canvas = Canvas::new(plan).map_err(|e| anyhow!(e))?;
    let map: HashMap<_, _> = plan
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();
    ensure!(canvas.cards.len() == plan.nodes.len(), "Lost cards");
    let mut screenshots = String::new();
    for (index, card) in canvas.cards.iter().enumerate() {
        let mut terminal = Terminal::new(TestBackend::new(CARD_W as u16, CARD_H as u16))?;
        let mut hits = vec![];
        terminal.draw(|frame| {
            hits = canvas.draw(frame, frame.area(), plan, index, card.x, card.y);
        })?;
        ensure!(
            hits.iter()
                .any(|(_, h)| matches!(h,Hit::Node(i) if *i==index)),
            "Missing card {index}: {}",
            text(&terminal)
        );
        screenshots.push_str(&format!(
            "\nNODE #{}\n{}\n",
            plan.nodes[index].id,
            text(&terminal)
        ));
        let Some(parent) = plan.nodes[index].parent.map(|id| canvas.cards[map[&id]]) else {
            continue;
        };
        let (sx, sy) = (card.x, card.y + 2);
        let (tx, ty) = (parent.x + CARD_W, parent.y + 2);
        ensure!(sx > tx, "Reversed or overlapping edge #{index}");
        let mid = (sx + tx) / 2;
        let top = sy.min(ty);
        let mut terminal = Terminal::new(TestBackend::new(
            (sx - tx) as u16,
            (sy.abs_diff(ty) + 1) as u16,
        ))?;
        terminal.draw(|frame| {
            hits = canvas.draw(frame, frame.area(), plan, index, tx, top);
        })?;
        let screen = text(&terminal);
        ensure!(
            hits.iter()
                .any(|(_, h)| matches!(h,Hit::Edge(i) if *i==index)),
            "Missing edge #{index}: {screen}"
        );
        let connected = |x: usize, y: usize| {
            matches!(
                terminal.backend().buffer()[((x - tx) as u16, (y - top) as u16)].symbol(),
                "─" | "━"
                    | "│"
                    | "┃"
                    | "◀"
                    | "┼"
                    | "├"
                    | "┤"
                    | "┬"
                    | "┴"
                    | "┌"
                    | "┐"
                    | "└"
                    | "┘"
            )
        };
        for (x, y) in (tx..mid)
            .map(|x| (x, ty))
            .chain((top..sy.max(ty)).map(|y| (mid, y)))
            .chain((mid..sx).map(|x| (x, sy)))
        {
            ensure!(
                connected(x, y),
                "Disconnected edge {} -> {} at ({x},{y}):\n{screen}",
                plan.nodes[index].id,
                plan.nodes[map[&plan.nodes[index].parent.unwrap()]].id
            );
        }
        screenshots.push_str(&format!(
            "\nEDGE #{} -> #{}\n{screen}\n",
            plan.nodes[index].id,
            plan.nodes[index].parent.unwrap()
        ));
    }
    for (width, height, left, top) in [
        (120, 40, 0, 0),
        (40, 12, 20, 10),
        (8, 4, 0, 0),
        (8, 4, 100000, 100000),
    ] {
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        terminal.draw(|frame| {
            canvas.draw(frame, frame.area(), plan, 0, left, top);
        })?;
        screenshots.push_str(&format!(
            "\nVIEW {width}x{height} offset {left},{top}\n{}\n",
            text(&terminal)
        ));
    }
    std::fs::write(directory.join("screens.txt"), screenshots)?;
    Ok(())
}
fn setup_memory(worker: &Worker) -> Result<()> {
    // Small independent test data, not a copy of yydbmanager's shared demo assets.
    for sql in [
        "CREATE TABLE departments(department_id INTEGER PRIMARY KEY,name VARCHAR(100))",
        "CREATE TABLE employees(employee_id INTEGER PRIMARY KEY,department_id INTEGER,manager_id INTEGER,name VARCHAR(100),salary INTEGER)",
        "CREATE TABLE customers(customer_id INTEGER PRIMARY KEY,name VARCHAR(100),region VARCHAR(20))",
        "CREATE TABLE orders(order_id INTEGER PRIMARY KEY,customer_id INTEGER,sales_rep_id INTEGER,total_amount INTEGER)",
        "CREATE TABLE products(product_id INTEGER PRIMARY KEY,name VARCHAR(100))",
        "CREATE TABLE order_items(order_id INTEGER,line_no INTEGER,product_id INTEGER,quantity INTEGER,unit_price INTEGER)",
        "INSERT INTO departments VALUES(1,'Sales'),(2,'Engineering'),(3,'Support')",
        "INSERT INTO employees VALUES(1,1,NULL,'A',100),(2,1,1,'B',90),(3,2,1,'C',110),(4,3,NULL,'D',80)",
        "INSERT INTO customers VALUES(1,'One','East'),(2,'Two','West'),(3,'Three','East')",
        "INSERT INTO orders VALUES(1,1,1,100),(2,1,2,200),(3,2,3,300),(4,3,4,100)",
        "INSERT INTO products VALUES(1,'Pencil'),(2,'Notebook'),(3,'Book')",
        "INSERT INTO order_items VALUES(1,1,1,2,50),(2,1,2,2,100),(3,1,3,3,100),(4,1,1,2,50)",
        "CREATE INDEX plan_test_orders_customer ON orders(customer_id)",
    ] {
        execute(worker, sql)?;
    }
    Ok(())
}
fn scenarios(profile: Profile, password: Option<String>) -> Result<()> {
    let kind = profile.kind;
    let temporary = tempfile::tempdir()?;
    let root = std::env::var_os("YYTE_PLAN_REPORTDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned())
        .join(kind.name().replace(' ', "-"));
    std::fs::create_dir_all(&root)?;
    let worker = Worker::new();
    let Reply::Connected(_, cancel, _) = call(&worker, Request::Connect(profile, password))? else {
        bail!("No connection")
    };
    let run = (|| -> Result<()> {
        if matches!(kind, DbKind::Sqlite | DbKind::DuckDb) {
            setup_memory(&worker)?;
        }
        let scenarios: Vec<Scenario> =
            serde_json::from_str(include_str!("../demo/database/plan-scenarios.json"))?;
        ensure!(scenarios.len() == 15, "Scenario count");
        let mut failures = vec![];
        let mut report = vec![];
        for scenario in scenarios {
            for actual in [false, true] {
                // Native upstream support boundaries: never pretend these are actual plans.
                if actual && matches!(kind, DbKind::Sqlite | DbKind::Db2) {
                    continue;
                }
                let dir = root.join(format!(
                    "{}-{}",
                    scenario.name,
                    if actual { "actual" } else { "estimated" }
                ));
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join("query.sql"), &scenario.sql)?;
                let result = (|| -> Result<Plan> {
                    let Reply::Results(_, page) =
                        call(&worker, Request::Execute(scenario.sql.clone()))?
                    else {
                        bail!("No query result")
                    };
                    ensure!(!page.rows.is_empty(), "Empty SELECT result");
                    let Reply::Plan(plan) =
                        call(&worker, Request::Explain(scenario.sql.clone(), actual))?
                    else {
                        bail!("No plan")
                    };
                    ensure!(
                        plan.kind == kind && plan.actual == actual,
                        "Wrong engine or plan mode"
                    );
                    if actual {
                        ensure!(
                            plan.nodes.iter().any(|n| n.actual_rows.is_some()),
                            "No actual row metrics"
                        );
                    }
                    ensure!(
                        plan.nodes.iter().filter(|n| n.parent.is_none()).count() == 1,
                        "One SELECT was split into query bands"
                    );
                    if kind == DbKind::MySql && !actual {
                        fn ids(value: &serde_json::Value, found: &mut Vec<String>) {
                            match value {
                                serde_json::Value::Object(map) => {
                                    if let Some(id) = map.get("select_id") {
                                        found.push(id.to_string());
                                    }
                                    for child in map.values() {
                                        ids(child, found);
                                    }
                                }
                                serde_json::Value::Array(items) => {
                                    for child in items {
                                        ids(child, found);
                                    }
                                }
                                _ => {}
                            }
                        }
                        let raw: serde_json::Value = serde_json::from_str(&plan.raw)?;
                        let mut expected = vec![];
                        ids(&raw, &mut expected);
                        for id in expected {
                            ensure!(
                                plan.nodes
                                    .iter()
                                    .any(|n| n.properties.get("select_id") == Some(&id)),
                                "Missing native MySQL SELECT #{id}"
                            );
                        }
                    }
                    if kind == DbKind::Sqlite {
                        ensure!(
                            plan.nodes.iter().filter(|n| n.parent.is_none()).count() == 1,
                            "Split SQLite statement"
                        );
                        let native: Vec<yy_db_core::plan::Node> = serde_json::from_str(&plan.raw)?;
                        for node in native {
                            let shown = plan
                                .nodes
                                .iter()
                                .find(|n| n.id == node.id)
                                .context("Lost native SQLite node")?;
                            ensure!(
                                node.operator == shown.operator,
                                "Changed native SQLite operator"
                            );
                            if node.parent.is_some() {
                                ensure!(node.parent == shown.parent, "Changed native SQLite edge");
                            }
                        }
                    }
                    let path = dir.join("plan.yyplan");
                    call(&worker, Request::SavePlan(path.clone(), plan.clone()))?;
                    render(&plan, &dir)?;
                    call(
                        &worker,
                        Request::PlanSvg(dir.join("plan.svg"), plan.clone()),
                    )?;
                    let Reply::Plan(restored) = call(&worker, Request::LoadPlan(path))? else {
                        bail!("No restored plan")
                    };
                    ensure!(
                        serde_json::to_value(&plan)? == serde_json::to_value(restored)?,
                        "Plan round trip changed data"
                    );
                    Ok(plan)
                })();
                match result {
                    Ok(plan) => {
                        eprintln!(
                            "PASS {} {} actual={actual}: {} nodes, {} edges",
                            kind.name(),
                            scenario.name,
                            plan.nodes.len(),
                            plan.nodes.iter().filter(|n| n.parent.is_some()).count()
                        );
                        report.push(serde_json::json!({"scenario":scenario.name,"actual":actual,"status":"passed","nodes":plan.nodes.len(),"edges":plan.nodes.iter().filter(|n|n.parent.is_some()).count()}));
                    }
                    Err(error) => {
                        let message = format!(
                            "{} {} actual={actual}: {error:#}",
                            kind.name(),
                            scenario.name
                        );
                        eprintln!("FAIL {message}");
                        std::fs::write(dir.join("error.txt"), &message)?;
                        report.push(serde_json::json!({"scenario":scenario.name,"actual":actual,"status":"failed","error":message}));
                        failures.push(message);
                    }
                }
            }
        }
        std::fs::write(
            root.join("verification.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        ensure!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    })();
    if run.is_err() {
        cancel.cancel();
    }
    let cleanup = call(&worker, Request::Disconnect);
    run?;
    cleanup?;
    Ok(())
}
#[test]
fn sqlite_fifteen_plans() -> Result<()> {
    scenarios(Profile::memory(DbKind::Sqlite), None)
}
#[test]
fn duckdb_fifteen_plans() -> Result<()> {
    scenarios(Profile::memory(DbKind::DuckDb), None)
}
fn external(kind: DbKind, port: u16, database: &str, username: &str) -> Result<()> {
    let config = yy_db_core::native::Config {
        host: std::env::var("YYDBMANAGER_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port,
        database: database.into(),
        username: username.into(),
        password: String::new(),
        tls: "disable".into(),
        ca_file: None,
        trust_server_certificate: false,
    };
    let profile = Profile {
        name: kind.name().into(),
        kind,
        target: config.encode().map_err(|e| anyhow!(e))?,
        read_only: false,
        password_env: String::new(),
    };
    scenarios(profile, Some("YyDemo_2026!".into()))
}
macro_rules! live {
    ($test:ident,$kind:ident,$port:literal,$database:literal,$username:literal) => {
        #[test]
        #[ignore = "requires original yydbmanager demo databases"]
        fn $test() -> Result<()> {
            external(DbKind::$kind, $port, $database, $username)
        }
    };
}
live!(
    postgres_fifteen_plans,
    PostgreSql,
    25432,
    "yydemo",
    "yy_demo"
);
live!(mysql_fifteen_plans, MySql, 23306, "yydemo", "yy_demo");
live!(sqlserver_fifteen_plans, SqlServer, 21433, "yydemo", "sa");
live!(oracle_fifteen_plans, Oracle, 21521, "FREEPDB1", "yy_demo");
live!(db2_fifteen_plans, Db2, 25000, "YYDEMO", "db2inst1");
