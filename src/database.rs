//! Persistent database worker shared with yydbmanager; never runs SQL on the UI thread.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};
use yy_db_core::{
    Column, Database, DbKind, Options, ResultSet, Value, catalog::Object, plan::Plan,
    schema::Model, session::Cancellation,
};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub kind: DbKind,
    pub target: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub password_env: String,
}
impl Profile {
    pub fn memory(kind: DbKind) -> Self {
        Self {
            name: format!("{} / memory", kind.name()),
            kind,
            target: ":memory:".into(),
            read_only: false,
            password_env: String::new(),
        }
    }
    pub fn options(&self, password: Option<String>) -> Result<Options> {
        let mut target = self.target.clone();
        if let Some(mut config) = yy_db_core::native::Config::decode(&target)? {
            if let Some(password) = password {
                config.password = password;
            } else if !self.password_env.is_empty() {
                config.password = std::env::var(&self.password_env).map_err(|_| {
                    format!(
                        "Password environment variable {} is not set",
                        self.password_env
                    )
                })?;
            }
            target = config.encode()?;
        }
        Ok(Options {
            kind: self.kind,
            path: matches!(self.kind, DbKind::Sqlite | DbKind::DuckDb)
                .then(|| PathBuf::from(&self.target)),
            connection_string: target,
            read_only: self.read_only,
        })
    }
}
pub fn load_profiles(path: &Path) -> Result<Vec<Profile>> {
    if !path.exists() {
        return Ok(vec![
            Profile::memory(DbKind::Sqlite),
            Profile::memory(DbKind::DuckDb),
        ]);
    }
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("Connection profiles exceed 1 MiB".into());
    }
    let raw = std::fs::read(path).map_err(|e| e.to_string())?;
    if raw.len() > 1024 * 1024 {
        return Err("Connection profiles exceed 1 MiB".into());
    }
    serde_json::from_slice(&raw).map_err(|e| e.to_string())
}
pub fn save_profiles(path: &Path, profiles: &[Profile]) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    for p in profiles {
        if p.name.trim().is_empty()
            || p.name.chars().any(char::is_control)
            || !names.insert(&p.name)
        {
            return Err(
                "Connection names must be unique, nonempty and contain no control characters"
                    .into(),
            );
        }
        if let Some(c) = yy_db_core::native::Config::decode(&p.target)?
            && !c.password.is_empty()
        {
            return Err("Passwords are not saved: use a password environment variable or enter at connect time".into());
        }
        let t = p.target.to_lowercase();
        if !t.starts_with("yynative:") && (t.contains("pwd=") || t.contains("password=")) {
            return Err("Do not store passwords in ODBC profiles".into());
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(profiles).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Clone)]
pub enum Format {
    Delimited(yy_db_core::transfer::Delimited),
    Parquet,
    Fixed {
        layout: yy_cobol::Layout,
        codec: yy_cobol::Codec,
        separator: Vec<u8>,
    },
}
impl Format {
    fn read(&self, path: &Path) -> Result<ResultSet> {
        match self {
            Self::Delimited(p) => yy_db_core::transfer::read_delimited(path, p),
            Self::Parquet => {
                let mut set = yy_db_core::transfer::read_parquet(path)?;
                // Arrow's Decimal128(38, 4) is storage metadata, not SQL DECIMAL(128,38).
                // The native importer's SQL type parser expects precision and scale first.
                for column in &mut set.columns {
                    column.type_name = parquet_sql_type(&column.type_name);
                }
                Ok(set)
            }
            Self::Fixed {
                layout,
                codec,
                separator,
            } => yy_db_core::transfer::read_fixed(path, layout, *codec, separator),
        }
    }
    fn write(&self, set: &ResultSet, path: &Path) -> Result<()> {
        match self {
            Self::Delimited(p) => yy_db_core::transfer::export_delimited(set, path, p),
            Self::Parquet => yy_db_core::transfer::export_parquet(set, path),
            Self::Fixed {
                layout,
                codec,
                separator,
            } => yy_db_core::transfer::export_fixed(set, path, layout, *codec, separator),
        }
    }
}
fn parquet_sql_type(name: &str) -> String {
    for prefix in ["Decimal128(", "Decimal256("] {
        if let Some(args) = name.strip_prefix(prefix).and_then(|s| s.strip_suffix(')')) {
            return format!("DECIMAL({args})");
        }
    }
    name.into()
}

// Parquet decimal storage can have more precision than the destination supports.
// Narrow only when every actual value fits; never round or truncate the input.
fn import_decimal_limits(set: &mut ResultSet, kind: DbKind) -> Result<()> {
    let limit: usize = match kind {
        DbKind::Db2 => 31,
        DbKind::Oracle | DbKind::SqlServer => 38,
        DbKind::MySql => 65,
        _ => return Ok(()),
    };
    for column in 0..set.columns.len() {
        let name = &set.columns[column].type_name;
        let Some(args) = name
            .strip_prefix("DECIMAL(")
            .and_then(|s| s.strip_suffix(')'))
        else {
            continue;
        };
        let Some((precision, scale)) = args.split_once(',') else {
            continue;
        };
        let (Ok(precision), Ok(scale)) = (
            precision.trim().parse::<usize>(),
            scale.trim().parse::<usize>(),
        ) else {
            continue;
        };
        if precision <= limit {
            continue;
        }
        if scale > limit {
            return Err(format!(
                "{}: scale {scale} exceeds {} precision {limit}",
                set.columns[column].name,
                kind.name()
            ));
        }
        let mut offset = 0;
        while offset < set.row_count {
            for row in set.page(offset, 512)? {
                if row[column] == Value::Null {
                    continue;
                }
                let value = row[column].display();
                let unsigned = value.trim_start_matches(['+', '-']);
                let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
                if !whole
                    .bytes()
                    .chain(fraction.bytes())
                    .all(|c| c.is_ascii_digit())
                    || whole.trim_start_matches('0').len() + scale > limit
                    || fraction.len() > scale
                {
                    return Err(format!(
                        "{}: value does not fit {} DECIMAL({limit},{scale})",
                        set.columns[column].name,
                        kind.name()
                    ));
                }
            }
            offset += 512;
        }
        set.columns[column].type_name = format!("DECIMAL({limit},{scale})");
    }
    Ok(())
}

// Oracle's thin driver does not reliably classify SQL with leading comments.
// Leave the statement body (including optimizer hints after SELECT) untouched.
fn oracle_statement(mut sql: &str) -> &str {
    loop {
        sql = sql.trim_start();
        if let Some(rest) = sql.strip_prefix("--") {
            sql = rest.find('\n').map(|i| &rest[i + 1..]).unwrap_or("");
        } else if let Some(rest) = sql.strip_prefix("/*") {
            let mut depth = 1;
            let mut i = 0;
            while i < rest.len() && depth > 0 {
                if rest.as_bytes()[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if rest.as_bytes()[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth != 0 {
                return "";
            }
            sql = &rest[i..];
        } else {
            return sql;
        }
    }
}
fn execute_batch(
    db: &mut Database,
    sql: &str,
    flag: &std::sync::atomic::AtomicBool,
) -> Result<Vec<ResultSet>> {
    if db.kind() != DbKind::Oracle {
        return db.execute(sql, flag);
    }
    let mut sets = vec![];
    for statement in yy_db_core::sql::statements(sql, DbKind::Oracle)? {
        let statement = oracle_statement(&statement);
        if !statement.is_empty() {
            sets.extend(db.execute(statement, flag)?);
        }
    }
    Ok(sets)
}

pub enum Request {
    Connect(Profile, Option<String>),
    Disconnect,
    Execute(String),
    Explain(String, bool),
    Catalog,
    Columns(String, String),
    Page {
        set: usize,
        start: u64,
    },
    Export {
        set: usize,
        path: PathBuf,
        format: Format,
    },
    Import {
        path: PathBuf,
        format: Format,
        table: String,
        create: bool,
    },
    Copy {
        set: usize,
        start: u64,
        end: u64,
        left: usize,
        right: usize,
    },
    Schema,
    LoadPlan(PathBuf),
    SavePlan(PathBuf, Plan),
    PlanSvg(PathBuf, Plan),
    ErSvg(PathBuf, Model),
    LoadEr(PathBuf),
    SaveEr(PathBuf, yy_db_core::schema::ErDocument),
    ErMarkdown(PathBuf, Model),
}
/// Format a bounded preview without materializing a whole TEXT/BLOB cell.
pub fn display_prefix(value: &Value, limit: usize) -> String {
    match value {
        Value::Text(s) | Value::Temporal(s) | Value::Integer(s) | Value::Decimal(s) => {
            if s.len() <= limit {
                return s.clone();
            }
            let mut end = limit.min(s.len());
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            s[..end].to_owned() + "…"
        }
        Value::Binary(bytes) => {
            use std::fmt::Write;
            let mut text = String::from("0x");
            for b in bytes.iter().take(limit.saturating_sub(2) / 2) {
                let _ = write!(text, "{b:02X}");
            }
            if text.len().saturating_sub(2) / 2 < bytes.len() {
                text.push('…');
            }
            text
        }
        _ => value.display(),
    }
}
pub struct Page {
    pub set: usize,
    pub start: u64,
    pub rows: Vec<Vec<Value>>,
}
pub struct Summary {
    pub columns: Vec<Column>,
    pub count: u64,
}
pub enum Reply {
    Connected(String, Cancellation, Vec<Object>),
    Disconnected,
    Results(Vec<Summary>, Page),
    Plan(Plan),
    Catalog(Vec<Object>),
    Columns(Vec<yy_db_core::catalog::Column>),
    Page(Page),
    Schema(yy_db_core::schema::ErDocument),
    Notice(String),
    Copied(String),
}
struct Session {
    db: Database,
    sets: Vec<ResultSet>,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
pub struct Worker {
    tx: Option<Sender<Request>>,
    pub rx: Receiver<Result<Reply>>,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let (out, results) = mpsc::channel();
        let join = std::thread::spawn(move || {
            let mut sessions: HashMap<String, Session> = HashMap::new();
            let mut active = String::new();
            while let Ok(request) = rx.recv() {
                let result = (|| -> Result<Reply> {
                    match request {
                        Request::Connect(profile, password) => {
                            let name = profile.name.clone();
                            if !sessions.contains_key(&name) {
                                let mut db = Database::open(&profile.options(password)?)?;
                                let objects = db.catalog()?;
                                let cancel = db.cancellation();
                                sessions.insert(
                                    name.clone(),
                                    Session {
                                        db,
                                        sets: vec![],
                                        flag: cancel.flag(),
                                    },
                                );
                                active = name.clone();
                                Ok(Reply::Connected(name, cancel, objects))
                            } else {
                                let s = sessions.get_mut(&name).unwrap();
                                let objects = s.db.catalog()?;
                                let cancel = s.db.cancellation();
                                s.flag = cancel.flag();
                                active = name.clone();
                                Ok(Reply::Connected(name, cancel, objects))
                            }
                        }
                        Request::Disconnect => {
                            sessions.remove(&active);
                            active.clear();
                            Ok(Reply::Disconnected)
                        }
                        Request::LoadPlan(path) => {
                            Ok(Reply::Plan(yy_db_core::plan_file::load(&path)?))
                        }
                        Request::PlanSvg(path, plan) => {
                            let doc = plan.drawing();
                            let text = yy_draw::render::Renderer::new(None)
                                .svg(&doc, &yy_db_core::plan::drawing_export(&doc))?;
                            write_new(&path, text.as_bytes())?;
                            Ok(Reply::Notice("Plan SVG exported".into()))
                        }
                        Request::ErSvg(path, model) => {
                            let doc = model.drawing();
                            let text = yy_draw::render::Renderer::new(None)
                                .svg(&doc, &yy_db_core::plan::drawing_export(&doc))?;
                            write_new(&path, text.as_bytes())?;
                            Ok(Reply::Notice("ER SVG exported".into()))
                        }
                        Request::SavePlan(path, plan) => {
                            if path.exists() {
                                return Err("Choose a new plan file path".into());
                            }
                            write_new(
                                &path,
                                &serde_json::to_vec_pretty(&plan).map_err(|e| e.to_string())?,
                            )?;
                            Ok(Reply::Notice("Plan saved".into()))
                        }
                        Request::LoadEr(path) => {
                            Ok(Reply::Schema(yy_db_core::schema::ErDocument::load(&path)?))
                        }
                        Request::SaveEr(path, model) => {
                            if path.exists() {
                                return Err("Choose a new ER YAML path".into());
                            }
                            model.save(&path)?;
                            Ok(Reply::Notice("ER YAML saved".into()))
                        }
                        Request::ErMarkdown(path, model) => {
                            if path.exists() {
                                return Err("Choose a new Markdown path".into());
                            }
                            model.export_markdown(&path)?;
                            Ok(Reply::Notice("ER Markdown exported".into()))
                        }
                        request => {
                            let s = sessions
                                .get_mut(&active)
                                .ok_or("Connect to a database first")?;
                            let flag = s.flag.clone();
                            match request {
                                Request::Execute(sql) => {
                                    s.sets = execute_batch(&mut s.db, &sql, &flag)?;
                                    let summaries = s
                                        .sets
                                        .iter()
                                        .map(|r| Summary {
                                            columns: r.columns.clone(),
                                            count: r.row_count,
                                        })
                                        .collect();
                                    let set = s.sets.len().saturating_sub(1);
                                    let rows = s
                                        .sets
                                        .get(set)
                                        .map(|s| s.page(0, 128))
                                        .transpose()?
                                        .unwrap_or_default();
                                    Ok(Reply::Results(
                                        summaries,
                                        Page {
                                            set,
                                            start: 0,
                                            rows,
                                        },
                                    ))
                                }
                                Request::Explain(sql, actual) => {
                                    let sql = if s.db.kind() == DbKind::Oracle {
                                        yy_db_core::sql::statements(&sql, DbKind::Oracle)?
                                            .iter()
                                            .map(|s| oracle_statement(s))
                                            .filter(|s| !s.is_empty())
                                            .collect::<Vec<_>>()
                                            .join(";\n")
                                    } else {
                                        sql
                                    };
                                    Ok(Reply::Plan(s.db.explain(&sql, actual, &flag)?))
                                }
                                Request::Copy {
                                    set,
                                    start,
                                    end,
                                    left,
                                    right,
                                } => {
                                    let set = s.sets.get(set).ok_or("No result set")?;
                                    if start > end
                                        || end >= set.row_count
                                        || left > right
                                        || right >= set.columns.len()
                                    {
                                        return Err("Selection outside result set".into());
                                    }
                                    let mut text = String::new();
                                    let mut offset = start;
                                    while offset <= end {
                                        if flag.load(std::sync::atomic::Ordering::Relaxed) {
                                            return Err("Copy cancelled".into());
                                        }
                                        for row in set
                                            .page(offset, ((end - offset + 1).min(128)) as usize)?
                                        {
                                            if !text.is_empty() {
                                                text.push('\n');
                                            }
                                            for (i, value) in
                                                row.iter().enumerate().take(right + 1).skip(left)
                                            {
                                                if i > left {
                                                    text.push('\t');
                                                }
                                                let value = value.display();
                                                if value.contains(['\t', '\n', '\r', '"']) {
                                                    text.push('"');
                                                    text.push_str(&value.replace('"', "\"\""));
                                                    text.push('"');
                                                } else {
                                                    text.push_str(&value);
                                                }
                                                if text.len() > 1024 * 1024 {
                                                    return Err("Copy selection exceeds 1 MiB; use result export".into());
                                                }
                                            }
                                            offset += 1;
                                        }
                                    }
                                    Ok(Reply::Copied(text))
                                }
                                Request::Catalog => Ok(Reply::Catalog(s.db.catalog()?)),
                                Request::Columns(schema, table) => {
                                    Ok(Reply::Columns(s.db.object_columns(&schema, &table)?))
                                }
                                Request::Page { set, start } => Ok(Reply::Page(Page {
                                    set,
                                    start,
                                    rows: s
                                        .sets
                                        .get(set)
                                        .ok_or("Result set missing")?
                                        .page(start, 128)?,
                                })),
                                Request::Export { set, path, format } => {
                                    if path.exists() {
                                        return Err("Choose a new export path".into());
                                    }
                                    format.write(
                                        s.sets.get(set).ok_or("Result set missing")?,
                                        &path,
                                    )?;
                                    Ok(Reply::Notice("Result exported".into()))
                                }
                                Request::Import {
                                    path,
                                    format,
                                    table,
                                    create,
                                } => {
                                    let mut set = format.read(&path)?;
                                    import_decimal_limits(&mut set, s.db.kind())?;
                                    let count = s.db.import_into(&set, &table, create)?;
                                    Ok(Reply::Notice(format!("Imported {count} rows")))
                                }
                                Request::Schema => Ok(Reply::Schema(
                                    yy_db_core::schema::ErDocument::new(s.db.er_model()?, None),
                                )),
                                _ => unreachable!(),
                            }
                        }
                    }
                })();
                if out.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            tx: Some(tx),
            rx: results,
            join: Some(join),
        }
    }
    pub fn send(&self, request: Request) -> Result<()> {
        self.tx
            .as_ref()
            .ok_or("Database worker closed")?
            .send(request)
            .map_err(|_| "Database worker closed".into())
    }
}
impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist_noclobber(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn reply(worker: &Worker) -> Result<Reply> {
        worker
            .rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|e| e.to_string())?
    }
    #[test]
    fn decimal_import_checks_destination_limits_without_rounding() -> Result<()> {
        let make = |text: &str| -> Result<ResultSet> {
            let mut set = ResultSet::new(vec![Column {
                name: "amount".into(),
                type_name: "DECIMAL(38,4)".into(),
            }])?;
            set.push(&[Value::Decimal(text.into())])?;
            set.push(&[Value::Null])?;
            set.finish()?;
            Ok(set)
        };
        let mut set = make("-1234567890123456.1234")?;
        import_decimal_limits(&mut set, DbKind::Db2)?;
        assert_eq!(set.columns[0].type_name, "DECIMAL(31,4)");
        assert_eq!(set.page(0, 1)?[0][0].display(), "-1234567890123456.1234");
        let mut too_large = make("1234567890123456789012345678.1234")?;
        assert!(import_decimal_limits(&mut too_large, DbKind::Db2).is_err());
        assert_eq!(too_large.columns[0].type_name, "DECIMAL(38,4)");
        let mut fractional = make("1.12345")?;
        assert!(import_decimal_limits(&mut fractional, DbKind::Db2).is_err());
        Ok(())
    }
    #[test]
    fn parquet_metadata_and_oracle_comments_preserve_semantics() {
        assert_eq!(parquet_sql_type("Decimal128(38, 4)"), "DECIMAL(38, 4)");
        assert_eq!(parquet_sql_type("Decimal256(76, 0)"), "DECIMAL(76, 0)");
        assert_eq!(parquet_sql_type("Binary"), "Binary");
        assert_eq!(
            oracle_statement(
                "-- 日本語\r\n /* a /* nested */ b */ SELECT /*+ INDEX(t) */ ';' FROM t"
            ),
            "SELECT /*+ INDEX(t) */ ';' FROM t"
        );
        assert_eq!(oracle_statement("-- comment only"), "");
    }
    #[test]
    fn persistent_sessions_exact_pages_transactions_plans_and_cancellation() -> Result<()> {
        let worker = Worker::new();
        for kind in [DbKind::Sqlite, DbKind::DuckDb] {
            worker.send(Request::Connect(Profile::memory(kind), None))?;
            assert!(matches!(reply(&worker)?, Reply::Connected(..)));
            worker.send(Request::Execute("CREATE TABLE t(id BIGINT,name VARCHAR); INSERT INTO t VALUES (9223372036854775807,'日本語'); SELECT * FROM t".into()))?;
            let Reply::Results(sets, page) = reply(&worker)? else {
                panic!("No query result")
            };
            assert_eq!(sets.last().unwrap().count, 1);
            assert_eq!(page.rows[0][0].display(), "9223372036854775807");
            assert_eq!(page.rows[0][1].display(), "日本語");
            worker.send(Request::Explain("SELECT * FROM t".into(), false))?;
            let Reply::Plan(plan) = reply(&worker)? else {
                panic!("No plan")
            };
            crate::database_plan::Canvas::new(&plan)?;
            worker.send(Request::Execute(
                "BEGIN; INSERT INTO t VALUES (2,'discard'); ROLLBACK; SELECT count(*) FROM t"
                    .into(),
            ))?;
            let Reply::Results(_, page) = reply(&worker)? else {
                panic!("No transaction result")
            };
            assert_eq!(page.rows[0][0].display(), "1");
        }
        worker.send(Request::Connect(Profile::memory(DbKind::Sqlite), None))?;
        let Reply::Connected(_, cancel, objects) = reply(&worker)? else {
            panic!("No session")
        };
        assert!(objects.iter().any(|o| o.name == "t"));
        worker.send(Request::Execute("SELECT * FROM t".into()))?;
        let Reply::Results(_, page) = reply(&worker)? else {
            panic!("No retained result")
        };
        assert_eq!(page.rows[0][1].display(), "日本語");
        worker.send(Request::Execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT sum(x) FROM n".into()))?;
        cancel.cancel();
        assert!(reply(&worker).is_err());
        cancel
            .flag()
            .store(false, std::sync::atomic::Ordering::Relaxed);
        worker.send(Request::Execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<400) SELECT x, 'value' FROM n".into()))?;
        let Reply::Results(_, page) = reply(&worker)? else {
            panic!("No results")
        };
        assert_eq!(page.rows.len(), 128);
        worker.send(Request::Page { set: 0, start: 300 })?;
        let Reply::Page(page) = reply(&worker)? else {
            panic!("No page")
        };
        assert_eq!(page.rows.len(), 100);
        assert_eq!(page.rows[0][0].display(), "301");
        worker.send(Request::Copy {
            set: 0,
            start: 126,
            end: 130,
            left: 0,
            right: 1,
        })?;
        let Reply::Copied(text) = reply(&worker)? else {
            panic!("No copy")
        };
        assert_eq!(
            text,
            "127\tvalue\n128\tvalue\n129\tvalue\n130\tvalue\n131\tvalue"
        );
        Ok(())
    }
    #[test]
    fn exports_and_schema_documents_round_trip_and_refuse_existing_paths() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let w = Worker::new();
        w.send(Request::Connect(Profile::memory(DbKind::Sqlite), None))?;
        reply(&w)?;
        w.send(Request::Execute("CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER PRIMARY KEY,parent_id INTEGER REFERENCES parent(id)); INSERT INTO parent VALUES(1); SELECT 9223372036854775807 AS id, 'quoted, text' AS name".into()))?;
        reply(&w)?;
        for (ext, format) in [
            ("csv", Format::Delimited(Default::default())),
            (
                "tsv",
                Format::Delimited(yy_db_core::transfer::Delimited {
                    delimiter: '\t',
                    ..Default::default()
                }),
            ),
            (
                "pipe",
                Format::Delimited(yy_db_core::transfer::Delimited {
                    delimiter: '|',
                    ..Default::default()
                }),
            ),
            ("parquet", Format::Parquet),
        ] {
            let path = dir.path().join(format!("result.{ext}"));
            w.send(Request::Export {
                set: 3,
                path: path.clone(),
                format: format.clone(),
            })?;
            reply(&w)?;
            let result = format.read(&path)?;
            assert_eq!(result.page(0, 1)?[0][0].display(), "9223372036854775807");
            w.send(Request::Export {
                set: 3,
                path,
                format,
            })?;
            assert!(reply(&w).is_err());
        }
        let layout = yy_cobol::parse("01 RECORD.\n 05 ID PIC 9(19).\n 05 NAME PIC X(12).\n")
            .map_err(|e| e.to_string())?;
        let fixed = Format::Fixed {
            layout,
            codec: yy_cobol::Codec {
                charset: yy_cobol::Charset::Ms932,
                little_endian: false,
            },
            separator: vec![],
        };
        let fixed_path = dir.path().join("result.dat");
        w.send(Request::Export {
            set: 3,
            path: fixed_path.clone(),
            format: fixed.clone(),
        })?;
        reply(&w)?;
        assert_eq!(
            fixed.read(&fixed_path)?.page(0, 1)?[0][0].display(),
            "9223372036854775807"
        );
        let importer = Worker::new();
        importer.send(Request::Connect(Profile::memory(DbKind::Sqlite), None))?;
        reply(&importer)?;
        for (ext, format) in [
            ("csv", Format::Delimited(Default::default())),
            ("parquet", Format::Parquet),
            ("dat", fixed),
        ] {
            importer.send(Request::Import {
                path: dir.path().join(format!("result.{ext}")),
                format,
                table: format!("import_{ext}"),
                create: true,
            })?;
            reply(&importer)?;
            importer.send(Request::Execute(format!(
                "SELECT count(*) FROM import_{ext}"
            )))?;
            let Reply::Results(_, page) = reply(&importer)? else {
                panic!("No import count")
            };
            assert_eq!(page.rows[0][0].display(), "1");
        }
        w.send(Request::Schema)?;
        let Reply::Schema(mut doc) = reply(&w)? else {
            panic!("No schema")
        };
        assert_eq!(doc.model.entities.len(), 2);
        assert_eq!(doc.model.relations.len(), 1);
        doc.view = Some(yy_db_core::schema::ErViewState {
            zoom: 1.5,
            origin: serde_json::from_str(r#"{"x":12.0,"y":34.0}"#).map_err(|e| e.to_string())?,
            selected: Some(doc.model.entities[0].id.clone()),
        });
        let path = dir.path().join("schema.yaml");
        w.send(Request::SaveEr(path.clone(), doc.clone()))?;
        reply(&w)?;
        w.send(Request::LoadEr(path))?;
        let Reply::Schema(restored) = reply(&w)? else {
            panic!("No restored schema")
        };
        assert_eq!(doc.to_yaml()?, restored.to_yaml()?);
        Ok(())
    }
    #[test]
    fn profiles_round_trip_without_persisting_passwords() -> Result<()> {
        let d = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = d.path().join("profiles.json");
        assert_eq!(load_profiles(&path)?.len(), 2);
        save_profiles(&path, &[Profile::memory(DbKind::Sqlite)])?;
        assert_eq!(load_profiles(&path)?[0].kind, DbKind::Sqlite);
        let config = yy_db_core::native::Config {
            host: "localhost".into(),
            port: 5432,
            database: "demo".into(),
            username: "user".into(),
            password: "secret".into(),
            tls: "require".into(),
            ca_file: None,
            trust_server_certificate: false,
        };
        let p = Profile {
            name: "test".into(),
            kind: DbKind::PostgreSql,
            target: config.encode()?,
            read_only: false,
            password_env: String::new(),
        };
        assert!(save_profiles(&path, &[p]).is_err());
        assert!(
            !std::fs::read_to_string(path)
                .map_err(|e| e.to_string())?
                .contains("secret")
        );
        Ok(())
    }
    #[test]
    fn bounded_display_keeps_utf8_and_original_values() {
        let value = Value::Text("日本語".repeat(10000));
        let prefix = display_prefix(&value, 7);
        assert_eq!(prefix, "日本…");
        assert_eq!(value.display().len(), 90000);
        let binary = Value::Binary(vec![0xab; 10000]);
        let prefix = display_prefix(&binary, 8);
        assert!(prefix.starts_with("0xAB"));
        assert!(prefix.len() < 20);
        assert_eq!(display_prefix(&Value::Text("short".into()), 8), "short");
    }
}
