//! Explicit opt-in: uses yydbmanager's unchanged demo Compose and SQL assets.
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use yy_db_core::{Column, DbKind, ResultSet, Value, native::Config};
use yyterminal_editor::{
    database::{Format, Page, Profile, Reply, Request, Worker},
    database_plan::Canvas,
};

fn call(w: &Worker, request: Request) -> Result<Reply> {
    w.send(request).map_err(|e| anyhow!(e))?;
    w.rx.recv_timeout(Duration::from_secs(120))
        .context("Database worker did not finish within 120 seconds")?
        .map_err(|e| anyhow!(e))
}
fn execute(
    w: &Worker,
    sql: impl Into<String>,
) -> Result<(Vec<yyterminal_editor::database::Summary>, Page)> {
    match call(w, Request::Execute(sql.into()))? {
        Reply::Results(sets, page) => Ok((sets, page)),
        _ => bail!("Expected result grid"),
    }
}
fn demo() -> Result<PathBuf> {
    let path = PathBuf::from(std::env::var("YYDBMANAGER_DEMO").context(
        "Set YYDBMANAGER_DEMO to yyeditor/demo/yydbmanager; no private test databases are used",
    )?);
    ensure!(
        path.join("compose.yaml").is_file(),
        "Missing original yydbmanager Compose"
    );
    Ok(path)
}
fn sample(
    w: &Worker,
    root: &Path,
    file: &str,
) -> Result<(Vec<yyterminal_editor::database::Summary>, Page)> {
    execute(w, std::fs::read_to_string(root.join("sql").join(file))?)
        .with_context(|| format!("yydbmanager sample {file}"))
}
fn scenario(kind: DbKind, service: &str, port: u16, database: &str, username: &str) -> Result<()> {
    let root = demo()?;
    let temporary = tempfile::tempdir()?;
    let directory = std::env::var_os("YYDBMANAGER_REPORTDIR")
        .map(PathBuf::from)
        .map(|p| p.join(service))
        .unwrap_or_else(|| temporary.path().to_path_buf());
    std::fs::create_dir_all(&directory)?;
    let dir = directory.as_path();
    let config = Config {
        host: std::env::var("YYDBMANAGER_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port,
        database: database.into(),
        username: username.into(),
        password: String::new(),
        tls: "disable".into(),
        ca_file: None,
        trust_server_certificate: false,
    };
    let worker = Worker::new();
    let profile = Profile {
        name: format!("live/{service}"),
        kind,
        target: config.encode().map_err(|e| anyhow!(e))?,
        read_only: false,
        password_env: String::new(),
    };
    let Reply::Connected(_, cancel, objects) = call(
        &worker,
        Request::Connect(profile, Some("YyDemo_2026!".into())),
    )?
    else {
        bail!("Expected native connection");
    };
    let run = (|| -> Result<()> {
        ensure!(
            objects
                .iter()
                .any(|o| o.name.eq_ignore_ascii_case("orders")),
            "Missing demo catalog"
        );
        let order = objects
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case("orders"))
            .unwrap();
        let Reply::Columns(columns) = call(
            &worker,
            Request::Columns(order.schema.clone(), order.name.clone()),
        )?
        else {
            bail!("Missing columns");
        };
        ensure!(
            columns
                .iter()
                .any(|c| c.name.eq_ignore_ascii_case("customer_id")),
            "Missing order column"
        );
        let expected: BTreeMap<String, u64> =
            serde_json::from_slice(&std::fs::read(root.join("data/expected-counts.json"))?)?;
        let (_, page) = sample(&worker, &root, "00-verify-counts.sql")?;
        let mut seen = BTreeMap::new();
        for row in page.rows {
            let name = row[0].display().trim().to_owned();
            let count: u64 = row[2].display().trim().parse()?;
            ensure!(
                Some(&count) == expected.get(&name),
                "Count mismatch {name}: {count}"
            );
            ensure!(seen.insert(name, count).is_none(), "Duplicate count row");
        }
        ensure!(seen == expected, "Incomplete demo counts");
        for file in [
            "01-overview.sql",
            "02-join-and-plan.sql",
            "03-index-selectivity.sql",
            "04-exact-values-and-logical-relation.sql",
        ] {
            let (sets, _) = sample(&worker, &root, file)?;
            ensure!(!sets.is_empty(), "Missing sample results {file}");
        }
        sample(&worker, &root, &format!("performance/{service}.sql"))?;
        eprintln!("PASS {service}: original nine counts and five sample SQL files through worker");
        // Exercise the actual terminal result paging/copy paths on the demo's 1200 orders.
        let (sets, page) = execute(&worker, "SELECT order_id FROM orders ORDER BY order_id")?;
        ensure!(
            sets[page.set].count == 1200 && page.rows.len() == 128,
            "Result paging"
        );
        let Reply::Page(next) = call(
            &worker,
            Request::Page {
                set: page.set,
                start: 1199,
            },
        )?
        else {
            bail!("Missing last page");
        };
        ensure!(
            next.rows.len() == 1 && next.rows[0][0].display().trim() == "1200",
            "Last row"
        );
        let Reply::Copied(copied) = call(
            &worker,
            Request::Copy {
                set: page.set,
                start: 126,
                end: 130,
                left: 0,
                right: 0,
            },
        )?
        else {
            bail!("Missing TSV copy");
        };
        ensure!(
            copied == "127\n128\n129\n130\n131",
            "Copy across page boundary"
        );
        let Reply::Schema(doc) = call(&worker, Request::Schema)? else {
            bail!("Missing ER metadata");
        };
        ensure!(
            doc.model
                .entities
                .iter()
                .any(|e| e.id.name.eq_ignore_ascii_case("orders")),
            "Missing ER table"
        );
        ensure!(
            doc.model
                .relations
                .iter()
                .any(|r| r.child.name.eq_ignore_ascii_case("orders")
                    && r.parent.name.eq_ignore_ascii_case("customers")),
            "Missing physical FK"
        );
        let yaml = dir.join("schema.yaml");
        call(&worker, Request::SaveEr(yaml.clone(), doc.clone()))?;
        let Reply::Schema(restored) = call(&worker, Request::LoadEr(yaml))? else {
            bail!("Missing ER round trip");
        };
        ensure!(
            doc.to_yaml().map_err(|e| anyhow!(e))? == restored.to_yaml().map_err(|e| anyhow!(e))?,
            "ER information loss"
        );
        let query = std::fs::read_to_string(root.join("sql/02-join-and-plan.sql"))?;
        for actual in [false, true] {
            // Same boundary as yydbmanager's native_live: Db2 actual needs activity-monitor setup.
            if actual && kind == DbKind::Db2 {
                continue;
            }
            let Reply::Plan(plan) = call(&worker, Request::Explain(query.clone(), actual))? else {
                bail!("Missing plan");
            };
            ensure!(
                !plan.nodes.is_empty() && plan.actual == actual,
                "Plan mode mismatch"
            );
            if actual {
                ensure!(
                    plan.nodes.iter().any(|n| n.actual_rows.is_some()),
                    "No measured row counts"
                );
            }
            let canvas = Canvas::new(&plan).map_err(|e| anyhow!(e))?;
            ensure!(
                canvas.cards.len() == plan.nodes.len(),
                "Lost terminal plan operators"
            );
            let path = dir.join(format!("{actual}.yyplan"));
            call(&worker, Request::SavePlan(path.clone(), plan.clone()))?;
            call(
                &worker,
                Request::PlanSvg(path.with_extension("svg"), plan.clone()),
            )?;
            let Reply::Plan(restored) = call(&worker, Request::LoadPlan(path))? else {
                bail!("Plan load");
            };
            ensure!(
                serde_json::to_value(&plan)? == serde_json::to_value(restored)?,
                "Plan information loss"
            );
            eprintln!(
                "PASS {service}: {} plan, {} terminal operator cards, SVG and lossless plan file",
                if actual { "actual" } else { "estimated" },
                plan.nodes.len()
            );
        }
        exact_values(&worker, kind, dir)?;
        Ok(())
    })();
    if run.is_err() {
        cancel.cancel();
    }
    // Connections are closed even when a step fails. No demo tables are modified.
    call(&worker, Request::Disconnect)?;
    run.with_context(|| format!("{} ({service})", kind.name()))
}
fn exact_values(w: &Worker, kind: DbKind, dir: &Path) -> Result<()> {
    let mut source = ResultSet::new(
        [
            ("id", "BIGINT"),
            ("amount", "DECIMAL(20,4)"),
            ("label", "VARCHAR"),
            ("payload", "VARBINARY"),
        ]
        .into_iter()
        .map(|(name, type_name)| Column {
            name: name.into(),
            type_name: type_name.into(),
        })
        .collect(),
    )
    .map_err(|e| anyhow!(e))?;
    let expected = vec![
        Value::Integer("9223372036854775807".into()),
        Value::Decimal("1234567890123456.1234".into()),
        Value::Text("日本語'引用\\path".into()),
        Value::Binary(vec![0, 255, 65, 127]),
    ];
    source.push(&expected).map_err(|e| anyhow!(e))?;
    source
        .push(&[Value::Null, Value::Null, Value::Null, Value::Null])
        .map_err(|e| anyhow!(e))?;
    source.finish().map_err(|e| anyhow!(e))?;
    let input = dir.join("exact.parquet");
    yy_db_core::transfer::export_parquet(&source, &input).map_err(|e| anyhow!(e))?;
    let name = format!("yyte_probe_{}", std::process::id());
    let table = yy_db_core::sql::identifier_for(kind, &name).map_err(|e| anyhow!(e))?;
    let q = |s| yy_db_core::sql::identifier_for(kind, s).map_err(|e| anyhow!(e));
    let columns = ["id", "amount", "label", "payload"]
        .into_iter()
        .map(q)
        .collect::<Result<Vec<_>>>()?
        .join(",");
    let run = (|| -> Result<()> {
        let Reply::Notice(message) = call(
            w,
            Request::Import {
                path: input,
                format: Format::Parquet,
                table: name,
                create: true,
            },
        )?
        else {
            bail!("No import response");
        };
        ensure!(message == "Imported 2 rows", "Import count: {message}");
        let (_, page) = execute(
            w,
            format!(
                "SELECT {columns} FROM {table} WHERE {} IS NOT NULL",
                q("id")?
            ),
        )?;
        ensure!(page.rows.len() == 1, "Imported row count");
        let mut displayed = page.rows[0].iter().map(Value::display).collect::<Vec<_>>();
        // Parquet stores BIGINT as DECIMAL(38,0); TDS prints its scale-zero value with .0.
        if let Some(integer) = displayed[0].strip_suffix(".0") {
            displayed[0] = integer.to_owned();
        }
        ensure!(
            displayed == expected.iter().map(Value::display).collect::<Vec<_>>(),
            "Exact value mismatch: {:?}",
            page.rows
        );
        let (_, page) = execute(
            w,
            format!("SELECT {columns} FROM {table} WHERE {} IS NULL", q("id")?),
        )?;
        ensure!(page.rows == vec![vec![Value::Null; 4]], "NULL mismatch");
        if !matches!(kind, DbKind::Oracle | DbKind::Db2) {
            execute(
                w,
                if kind == DbKind::SqlServer {
                    "BEGIN TRANSACTION"
                } else {
                    "BEGIN"
                },
            )?;
        }
        execute(w, format!("DELETE FROM {table}"))?;
        execute(w, "ROLLBACK")?;
        let (_, page) = execute(w, format!("SELECT COUNT(*) FROM {table}"))?;
        ensure!(page.rows[0][0].display() == "2", "Rollback failed");
        if kind == DbKind::Db2 {
            // Match native_live's unsupported-CLOB/cursor recovery regression.
            ensure!(
                execute(
                    w,
                    format!("SELECT CAST({} AS CLOB(128)) FROM {table}", q("label")?)
                )
                .is_err(),
                "Db2 CLOB boundary changed; review upstream test"
            );
        }
        eprintln!(
            "PASS {}: Parquet import, exact integer/decimal, Japanese, binary, NULL and rollback",
            kind.name()
        );
        Ok(())
    })();
    // Only this process's own probe table is dropped, including on scenario failure.
    let cleanup = execute(w, format!("DROP TABLE {table}"));
    if matches!(kind, DbKind::Oracle | DbKind::Db2) {
        execute(w, "COMMIT")?;
    }
    run?;
    cleanup?;
    Ok(())
}
macro_rules! live {
    ($test:ident,$kind:ident,$service:literal,$port:literal,$db:literal,$user:literal) => {
        #[test]
        #[ignore = "requires yydbmanager's original demo Compose and YYDBMANAGER_DEMO"]
        fn $test() -> Result<()> {
            scenario(DbKind::$kind, $service, $port, $db, $user)
        }
    };
}
live!(postgres, PostgreSql, "postgres", 25432, "yydemo", "yy_demo");
live!(mysql, MySql, "mysql", 23306, "yydemo", "yy_demo");
live!(sqlserver, SqlServer, "sqlserver", 21433, "yydemo", "sa");
live!(oracle, Oracle, "oracle", 21521, "FREEPDB1", "yy_demo");
live!(db2, Db2, "db2", 25000, "YYDEMO", "db2inst1");
