//! Explicit opt-in: Db2 actual plans against yydbmanager's dedicated validation Compose.
//! The activity monitor is created there as upstream docker_live does, never in the shared demo.
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::{path::PathBuf, time::Duration};
use yy_db_core::{DbKind, native::Config};
use yyterminal_editor::{
    database::{Page, Profile, Reply, Request, Worker},
    database_plan::Canvas,
};

fn call(w: &Worker, request: Request) -> Result<Reply> {
    w.send(request).map_err(|e| anyhow!(e))?;
    w.rx.recv_timeout(Duration::from_secs(120))
        .context("Database worker did not finish within 120 seconds")?
        .map_err(|e| anyhow!(e))
}
fn execute(w: &Worker, sql: impl Into<String>) -> Result<Page> {
    let sql = sql.into();
    match call(w, Request::Execute(sql.clone())).with_context(|| sql.clone())? {
        Reply::Results(_, page) => Ok(page),
        _ => bail!("Expected result grid: {sql}"),
    }
}
fn count(w: &Worker, sql: &str) -> Result<u64> {
    let page = execute(w, sql)?;
    let cell = page
        .rows
        .first()
        .and_then(|r| r.first())
        .context(sql.to_owned())?;
    Ok(cell.display().trim().parse()?)
}

#[test]
#[ignore = "requires yydbmanager's validation Compose db2 service and YYDBMANAGER_VALIDATION"]
fn db2_actual_plan() -> Result<()> {
    let compose = PathBuf::from(
        std::env::var("YYDBMANAGER_VALIDATION")
            .context("Set YYDBMANAGER_VALIDATION to yyeditor/apps/yydbmanager/tests/docker")?,
    );
    ensure!(
        compose.join("compose.yml").is_file(),
        "Missing yydbmanager validation Compose"
    );
    let temporary = tempfile::tempdir()?;
    let dir = std::env::var_os("YYDBMANAGER_REPORTDIR")
        .map(|p| PathBuf::from(p).join("db2-actual"))
        .unwrap_or_else(|| temporary.path().to_path_buf());
    std::fs::create_dir_all(&dir)?;
    let config = Config {
        host: std::env::var("YYDBMANAGER_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: 15000,
        database: "YYTEST".into(),
        username: "db2inst1".into(),
        password: String::new(),
        tls: "disable".into(),
        ca_file: None,
        trust_server_certificate: false,
    };
    let worker = Worker::new();
    let profile = Profile {
        name: "validation/db2".into(),
        kind: DbKind::Db2,
        target: config.encode().map_err(|e| anyhow!(e))?,
        read_only: false,
        password_env: String::new(),
    };
    let Reply::Connected(_, cancel, _) = call(
        &worker,
        Request::Connect(profile, Some("Yydb_Test_2026!".into())),
    )?
    else {
        bail!("Expected native connection");
    };
    let table = format!("YYTE_ACTUAL_{}", std::process::id());
    let query = format!(
        "SELECT GRP, COUNT(*), SUM(AMOUNT) FROM {table} WHERE ID > 50 GROUP BY GRP ORDER BY GRP"
    );
    let run = (|| -> Result<()> {
        execute(
            &worker,
            format!(
                "CREATE TABLE {table} (ID INTEGER NOT NULL PRIMARY KEY, GRP INTEGER NOT NULL, AMOUNT DECIMAL(12,2) NOT NULL)"
            ),
        )?;
        execute(
            &worker,
            format!(
                "INSERT INTO {table} WITH N(I) AS (VALUES 1 UNION ALL SELECT I + 1 FROM N WHERE I < 200) SELECT I, MOD(I, 7), I * 1.5 FROM N"
            ),
        )?;
        execute(&worker, "COMMIT")?;
        let rows = format!("SELECT COUNT(*) FROM {table}");
        ensure!(count(&worker, &rows)? == 200, "Probe rows");
        let monitor =
            "SELECT COUNT(*) FROM SYSCAT.EVENTMONITORS WHERE EVMONNAME='YYDBMANAGER_ACTIVITY'";
        if count(&worker, monitor)? == 0 {
            // Before setup the plan is refused without running the statement.
            let error = call(
                &worker,
                Request::Explain(format!("DELETE FROM {table}"), true),
            )
            .err()
            .context("Db2 actual plan succeeded before setup")?;
            ensure!(
                count(&worker, &rows)? == 200,
                "Refused actual plan ran the DELETE: {error:#}"
            );
            eprintln!("PASS Db2 actual plan refused before setup, rows unchanged: {error}");
        }
        // The statements of yy_db_core::plan::DB2_ACTUAL_SETUP_SQL, each only when needed.
        if count(
            &worker,
            "SELECT COUNT(*) FROM SYSCAT.TABLESPACES WHERE DATATYPE='U'",
        )? == 0
        {
            execute(
                &worker,
                "CREATE USER TEMPORARY TABLESPACE YYDBMANAGER_TEMP MANAGED BY AUTOMATIC STORAGE",
            )?;
        }
        if count(
            &worker,
            "SELECT COUNT(*) FROM SYSCAT.TABLES WHERE TABSCHEMA=CURRENT SCHEMA AND TABNAME='EXPLAIN_OPERATOR'",
        )? == 0
        {
            execute(
                &worker,
                "CALL SYSPROC.SYSINSTALLOBJECTS('EXPLAIN','C',CAST(NULL AS VARCHAR(128)),CURRENT SCHEMA)",
            )?;
        }
        if count(&worker, monitor)? == 0 {
            execute(
                &worker,
                "CREATE EVENT MONITOR YYDBMANAGER_ACTIVITY FOR ACTIVITIES WRITE TO TABLE MANUALSTART",
            )?;
        }
        execute(&worker, "COMMIT")?;
        execute(&worker, "SET EVENT MONITOR YYDBMANAGER_ACTIVITY STATE 1")?;
        for actual in [false, true] {
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
                    "No measured row counts: {:?}",
                    plan.nodes
                );
            }
            let canvas = Canvas::new(&plan).map_err(|e| anyhow!(e))?;
            ensure!(
                canvas.cards.len() == plan.nodes.len(),
                "Lost terminal plan operators"
            );
            let path = dir.join(format!("{actual}.yyplan"));
            let _ = std::fs::remove_file(&path);
            call(&worker, Request::SavePlan(path.clone(), plan.clone()))?;
            let svg = path.with_extension("svg");
            let _ = std::fs::remove_file(&svg);
            call(&worker, Request::PlanSvg(svg, plan.clone()))?;
            let Reply::Plan(restored) = call(&worker, Request::LoadPlan(path))? else {
                bail!("Plan load");
            };
            ensure!(
                serde_json::to_value(&plan)? == serde_json::to_value(restored)?,
                "Plan information loss"
            );
            eprintln!(
                "PASS Db2 {} plan: {} terminal operator cards, {} with measured rows, SVG and lossless plan file",
                if actual { "actual" } else { "estimated" },
                plan.nodes.len(),
                plan.nodes
                    .iter()
                    .filter(|n| n.actual_rows.is_some())
                    .count()
            );
        }
        // The connection stays usable after the actual plan restores its settings.
        ensure!(
            count(
                &worker,
                &format!("SELECT COUNT(*) FROM {table} WHERE ID > 50")
            )? == 150,
            "Rows after actual plan"
        );
        Ok(())
    })();
    if run.is_err() {
        cancel.cancel();
    }
    // Only this process's probe table is dropped; the monitor stays like upstream's validation.
    let cleanup =
        execute(&worker, format!("DROP TABLE {table}")).and_then(|_| execute(&worker, "COMMIT"));
    call(&worker, Request::Disconnect)?;
    run?;
    cleanup?;
    Ok(())
}
