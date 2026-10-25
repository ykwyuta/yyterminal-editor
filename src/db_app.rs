use crate::{App, Editor, K, M, MouseAction, Prompt, View};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};
use std::{collections::BTreeMap, path::PathBuf, sync::atomic::Ordering};
use yy_db_core::{DbKind, Value, plan::Plan, schema::Model, session::Cancellation};
use yyterminal_editor::{
    database::{self as db, Format, Profile, Reply, Request, Worker},
    database_plan::Canvas,
};
#[derive(Clone, Copy, PartialEq)]
pub enum Pane {
    Query,
    Results,
    Plan,
    Er,
    Messages,
}
#[derive(Clone, Copy)]
pub enum Input {
    Name,
    Kind,
    Path,
    Host,
    Port,
    Database,
    Username,
    Tls,
    PasswordEnv,
    ReadOnly,
    Password,
    Select,
    OpenPlan,
    SavePlan,
    PlanSvg,
    ErSvg,
    ImportPath,
    ImportTable,
    ExportPath,
    Delimiter,
    FixedLayout,
    Width,
    Height,
    OpenEr,
    SaveEr,
    ErMarkdown,
    Diagnostic,
    DeleteProfile,
    EntityName,
    ColumnsTable,
    RelationSuggestion,
}
impl Input {
    pub fn title(self) -> &'static str {
        match self {
            Self::Name => "Connection name",
            Self::Kind => {
                "Database: SQLite / DuckDB / PostgreSQL / SQL Server / MySQL / Oracle / Db2"
            }
            Self::Path => "Database file path (or :memory:)",
            Self::Host => "Database server hostname",
            Self::Port => "Database port",
            Self::Database => "Database / service name",
            Self::Username => "Database username",
            Self::Tls => "TLS: require / prefer / disable",
            Self::ReadOnly => "Read-only connection: yes / no",
            Self::PasswordEnv => "Password environment variable name (blank = ask on connect)",
            Self::Password => "Password (masked; kept only for this connection)",
            Self::Select => "Connection name to select",
            Self::OpenPlan => "Open plan: .sqlplan / .yyplan / native JSON / XML / text",
            Self::PlanSvg => "Export plan to a NEW SVG path",
            Self::ErSvg => "Export ER to a NEW SVG path",
            Self::SavePlan => "Save plan to a NEW .yyplan path",
            Self::ImportPath => "Import file path (CSV / TSV / Parquet / fixed-length)",
            Self::ImportTable => "Import: table name;create or append",
            Self::ExportPath => "Export results to a NEW path",
            Self::Delimiter => "Delimiter: , / \\t / | / one character",
            Self::FixedLayout => "Fixed-length layout: .yyl path",
            Self::Width => "Result column width: 4..80 cells",
            Self::Height => "Result row height: 1..8 lines",
            Self::OpenEr => "Open ER YAML (.yyer / .yaml)",
            Self::SaveEr => "Save ER to a NEW YAML path",
            Self::ErMarkdown => "Export ER design to a NEW Markdown path",
            Self::Diagnostic => "Performance template number (see Messages)",
            Self::ColumnsTable => "ER table name to edit columns (schema.table)",
            Self::EntityName => "New ER table: schema.table (or table)",
            Self::RelationSuggestion => "Apply recommended logical relation: number",
            Self::DeleteProfile => "Delete connection profile: type its name",
        }
    }
}
pub struct State {
    pub worker: Worker,
    pub profiles: Vec<Profile>,
    pub profile_path: PathBuf,
    pub profile: usize,
    pub active: String,
    pub cancellation: Option<Cancellation>,
    pub busy: bool,
    pub pane: Pane,
    pub result_focus: bool,
    pub objects: Vec<yy_db_core::catalog::Object>,
    pub expanded: std::collections::BTreeSet<String>,
    pub tree_offset: usize,
    tree_cache: Option<Vec<(String, Option<Hit>)>>,
    er_cache: yyterminal_editor::render_cache::TextViewport,
    properties_cache: yyterminal_editor::render_cache::TextViewport,
    plan_revision: u64,
    auto_widths: BTreeMap<(usize, usize), u16>,
    pub summaries: Vec<db::Summary>,
    pub page: Option<db::Page>,
    pub result: usize,
    pub row: u64,
    pub col: usize,
    pub left: usize,
    pub top: u64,
    pub anchor: Option<(u64, usize)>,
    pub widths: BTreeMap<usize, u16>,
    pub height: u16,
    pub visible_rows: u64,
    pub plan_edge: bool,
    pub plan: Option<Plan>,
    pub canvas: Option<Canvas>,
    pub node: usize,
    pub plan_left: usize,
    pub plan_top: usize,
    pub property_top: usize,
    pub model: Option<Model>,
    pub er_generation: u64,
    pub type_menu: bool,
    pub er_view: Option<yy_db_core::schema::ErViewState>,
    pub messages: String,
    pub draft: Profile,
    pub draft_index: Option<usize>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub tls: String,
    pub format: Format,
    pub transfer_path: PathBuf,
    pub object_columns: BTreeMap<usize, Vec<yy_db_core::catalog::Column>>,
    pub pending_object: Option<usize>,
    pub suggestions: Vec<yy_db_core::schema::RelationSuggestion>,
    pub tree_area: Rect,
    pub data_area: Rect,
}
impl State {
    fn new(path: PathBuf) -> Self {
        let (profiles, messages) = match db::load_profiles(&path) {
            Ok(p) => (p, String::new()),
            Err(e) => (vec![], e),
        };
        Self {
            worker: Worker::new(),
            profiles,
            profile_path: path,
            profile: 0,
            active: String::new(),
            cancellation: None,
            busy: false,
            pane: Pane::Query,
            result_focus: false,
            objects: vec![],
            expanded: Default::default(),
            tree_offset: 0,
            tree_cache: None,
            er_cache: Default::default(),
            properties_cache: Default::default(),
            plan_revision: 0,
            auto_widths: Default::default(),
            summaries: vec![],
            page: None,
            result: 0,
            row: 0,
            col: 0,
            left: 0,
            top: 0,
            anchor: None,
            widths: Default::default(),
            height: 1,
            visible_rows: 12,
            plan_edge: false,
            plan: None,
            canvas: None,
            node: 0,
            plan_left: 0,
            plan_top: 0,
            property_top: 0,
            model: None,
            er_view: None,
            er_generation: 0,
            type_menu: false,
            messages,
            draft: Profile::memory(DbKind::Sqlite),
            draft_index: None,
            host: String::new(),
            port: 0,
            database: String::new(),
            username: String::new(),
            tls: "require".into(),
            format: Format::Delimited(Default::default()),
            transfer_path: PathBuf::new(),
            object_columns: Default::default(),
            pending_object: None,
            suggestions: vec![],
            tree_area: Rect::default(),
            data_area: Rect::default(),
        }
    }
    pub(crate) fn send(&mut self, request: Request) {
        if self.busy {
            self.messages = "A database operation is already running".into();
            return;
        }
        if let Some(c) = &self.cancellation {
            c.flag().store(false, Ordering::Relaxed);
        }
        match self.worker.send(request) {
            Ok(()) => self.busy = true,
            Err(e) => self.messages = e,
        }
    }
    fn count(&self) -> u64 {
        self.summaries.get(self.result).map_or(0, |s| s.count)
    }
    fn columns(&self) -> usize {
        self.summaries
            .get(self.result)
            .map_or(0, |s| s.columns.len())
    }
    fn page(&mut self) {
        if self.busy {
            return;
        }
        if self.count() > 0
            && !self.page.as_ref().is_some_and(|p| {
                p.set == self.result
                    && self.top >= p.start
                    && self.top + 64 < p.start + p.rows.len() as u64
                    || p.set == self.result
                        && p.start + p.rows.len() as u64 == self.count()
                        && self.top >= p.start
            })
        {
            self.send(Request::Page {
                set: self.result,
                start: self.top,
            });
        }
    }
    fn focus_node(&mut self) {
        if let Some(c) = self.canvas.as_ref().and_then(|c| c.cards.get(self.node)) {
            self.plan_left = c.x.saturating_sub(2);
            self.plan_top = c.y.saturating_sub(2);
        }
        self.property_top = 0;
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(c) = &self.cancellation {
            c.cancel();
        }
    }
}
impl App {
    pub fn ensure_db(&mut self) {
        if self.db.is_none() {
            let path = self
                .settings_path
                .as_ref()
                .and_then(|p| p.parent())
                .unwrap_or(&self.root)
                .join("databases.json");
            self.db = Some(State::new(path));
        }
    }
    pub fn db_prompt(&mut self, input: Input) {
        self.ask(Prompt::Database(input));
        let Some(d) = self.db.as_ref() else { return };
        self.input = match input {
            Input::Name => d.draft.name.clone(),
            Input::Kind => d.draft.kind.name().into(),
            Input::Path => d.draft.target.clone(),
            Input::Database => d.database.clone(),
            Input::Username => d.username.clone(),
            Input::PasswordEnv => d.draft.password_env.clone(),
            Input::Host => {
                if d.host.is_empty() {
                    "localhost".into()
                } else {
                    d.host.clone()
                }
            }
            Input::Port if d.port > 0 => d.port.to_string(),
            Input::Port => match d.draft.kind {
                DbKind::PostgreSql => 5432,
                DbKind::MySql => 3306,
                DbKind::SqlServer => 1433,
                DbKind::Oracle => 1521,
                DbKind::Db2 => 50000,
                _ => 0,
            }
            .to_string(),
            Input::Tls => d.tls.clone(),
            Input::ReadOnly => if d.draft.read_only { "yes" } else { "no" }.into(),
            Input::Select => d
                .profiles
                .get(d.profile)
                .map_or(String::new(), |p| p.name.clone()),
            _ => String::new(),
        };
    }
    pub fn db_poll(&mut self) -> bool {
        let Some(d) = &mut self.db else { return false };
        let mut changed = false;
        while let Ok(reply) = d.worker.rx.try_recv() {
            changed = true;
            d.busy = false;
            d.tree_cache = None;
            match reply {
                Err(e) => {
                    d.messages = e.clone();
                    d.property_top = 0;
                    self.status = e;
                    d.pane = Pane::Messages;
                }
                Ok(Reply::Connected(name, c, objects)) => {
                    d.active = name;
                    d.cancellation = Some(c);
                    d.objects = objects;
                    d.object_columns.clear();
                    d.summaries.clear();
                    d.page = None;
                    self.status = format!("Connected: {}", d.active);
                }
                Ok(Reply::Disconnected) => {
                    d.active.clear();
                    d.cancellation = None;
                    d.objects.clear();
                    d.summaries.clear();
                    d.page = None;
                    self.status = "Database disconnected".into();
                }
                Ok(Reply::Results(summaries, page)) => {
                    d.result = page.set;
                    d.summaries = summaries;
                    d.page = Some(page);
                    d.row = 0;
                    d.col = 0;
                    d.top = 0;
                    d.left = 0;
                    d.widths.clear();
                    d.auto_widths.clear();
                    d.anchor = None;
                    d.pane = Pane::Results;
                    self.status = format!(
                        "SQL complete: {} result sets, {} rows",
                        d.summaries.len(),
                        d.count()
                    );
                }
                Ok(Reply::Copied(text)) => {
                    self.clipboard = text;
                    self.status = "Result selection copied".into();
                }
                Ok(Reply::Page(page)) => d.page = Some(page),
                Ok(Reply::Plan(plan)) => match Canvas::new(&plan) {
                    Ok(canvas) => {
                        d.plan_revision = d.plan_revision.wrapping_add(1);
                        d.canvas = Some(canvas);
                        d.plan = Some(plan);
                        d.node = 0;
                        d.plan_left = 0;
                        d.plan_top = 0;
                        d.property_top = 0;
                        d.pane = Pane::Plan;
                        self.status =
                            "Execution plan ready | source → result: right to left".into();
                    }
                    Err(e) => {
                        d.messages = e;
                        d.pane = Pane::Messages;
                    }
                },
                Ok(Reply::Catalog(objects)) => {
                    d.objects = objects;
                    d.object_columns.clear();
                    self.status = "Database objects refreshed".into();
                }
                Ok(Reply::Columns(columns)) => {
                    if let Some(i) = d.pending_object.take() {
                        d.object_columns.insert(i, columns);
                    }
                }
                Ok(Reply::Schema(model)) => {
                    d.er_generation += 1;
                    d.er_view = model.view;
                    d.model = Some(model.model);
                    d.pane = Pane::Er;
                    self.status = "ER model ready".into();
                }
                Ok(Reply::Notice(s)) => {
                    self.status = s.clone();
                    d.messages = s;
                }
            }
        }
        changed
    }
    fn db_sql(&self) -> Result<String, String> {
        let e = &self.tabs[self.tab];
        if e.terminal.is_some() || e.raw || e.spreadsheet {
            return Err("Select a text or SQL tab before executing SQL".into());
        }
        if let Ok(Some(text)) = e.doc.selected_text(32 * 1024 * 1024) {
            return Ok(text);
        }
        let snap = e.doc.snapshot();
        if snap.len() > 32 * 1024 * 1024 {
            return Err("Select SQL text (maximum 32 MiB)".into());
        }
        Ok(String::from_utf8_lossy(&snap.read(0..snap.len())).into_owned())
    }
    pub fn db_command(&mut self, name: &str) -> bool {
        if !name.starts_with("DB ") {
            return false;
        }
        self.ensure_db();
        self.db.as_mut().unwrap().tree_cache = None;
        if name == "DB New SQL" || name == "DB Manager" {
            if name == "DB New SQL" || !self.tabs[self.tab].database {
                let mut e = Editor::empty();
                e.database = true;
                e.syntax_language = Some("sql".into());
                self.tabs.push(e);
                self.tab = self.tabs.len() - 1;
            }
            self.view = View::Database;
            self.explorer = false;
            self.db.as_mut().unwrap().pane = Pane::Query;
            return true;
        }
        if name == "DB Query" {
            self.db.as_mut().unwrap().result_focus = false;
            self.tabs[self.tab].database = true;
            self.db.as_mut().unwrap().pane = Pane::Query;
            self.view = View::Database;
            return true;
        }
        if !matches!(name, "DB Column types" | "DB Apply ER columns") {
            self.view = View::Database;
        }
        self.explorer = false;
        let result = (|| -> Result<(), String> {
            match name {
                "DB Edit connection" => {
                    let d = self.db.as_mut().unwrap();
                    let p = d
                        .profiles
                        .get(d.profile)
                        .ok_or("Select a connection first")?
                        .clone();
                    if d.busy {
                        return Err("Wait for the database operation to finish".into());
                    }
                    if d.active == p.name {
                        d.send(Request::Disconnect);
                    }
                    if let Some(c) = yy_db_core::native::Config::decode(&p.target)? {
                        d.host = c.host;
                        d.port = c.port;
                        d.database = c.database;
                        d.username = c.username;
                        d.tls = c.tls;
                    }
                    d.draft = p;
                    d.draft_index = Some(d.profile);
                    self.db_prompt(Input::Name);
                }
                "DB New connection" => {
                    let d = self.db.as_mut().unwrap();
                    d.draft = Profile::memory(DbKind::Sqlite);
                    d.draft.name.clear();
                    d.draft_index = None;
                    d.host.clear();
                    d.port = 0;
                    d.database.clear();
                    d.username.clear();
                    d.tls = "require".into();
                    self.db_prompt(Input::Name);
                }
                "DB Select connection" => self.db_prompt(Input::Select),
                "DB Delete connection" => self.db_prompt(Input::DeleteProfile),
                "DB Connect" => {
                    let d = self.db.as_mut().unwrap();
                    let p = d
                        .profiles
                        .get(d.profile)
                        .ok_or("Create or select a connection first")?
                        .clone();
                    if !matches!(p.kind, DbKind::Sqlite | DbKind::DuckDb)
                        && p.password_env.is_empty()
                        && !d.active.eq(&p.name)
                    {
                        self.db_prompt(Input::Password);
                    } else {
                        d.send(Request::Connect(p, None));
                    }
                }
                "DB Disconnect" => self.db.as_mut().unwrap().send(Request::Disconnect),
                "DB Execute" | "DB Estimated plan" | "DB Actual plan" => {
                    self.tabs[self.tab].database = true;
                    let sql = self.db_sql()?;
                    self.db.as_mut().unwrap().send(if name == "DB Execute" {
                        Request::Execute(sql)
                    } else {
                        Request::Explain(sql, name == "DB Actual plan")
                    });
                }
                "DB Cancel" => {
                    if let Some(c) = &self.db.as_ref().unwrap().cancellation {
                        c.cancel();
                        self.status = "Database cancellation requested".into();
                    }
                }
                "DB Commit" | "DB Rollback" => self.db.as_mut().unwrap().send(Request::Execute(
                    if name == "DB Commit" {
                        "COMMIT"
                    } else {
                        "ROLLBACK"
                    }
                    .into(),
                )),
                "DB Refresh objects" => self.db.as_mut().unwrap().send(Request::Catalog),
                "DB Results" => self.db.as_mut().unwrap().pane = Pane::Results,
                "DB Plan" => self.db.as_mut().unwrap().pane = Pane::Plan,
                "DB Messages" => self.db.as_mut().unwrap().pane = Pane::Messages,
                "DB Open plan" => self.db_prompt(Input::OpenPlan),
                "DB Save plan" => self.db_prompt(Input::SavePlan),
                "DB Plan SVG" => self.db_prompt(Input::PlanSvg),
                "DB ER SVG" => self.db_prompt(Input::ErSvg),
                "DB Plan raw" => {
                    let plan = self.db.as_ref().unwrap().plan.as_ref().ok_or("No plan")?;
                    let raw = plan.raw.clone();
                    self.db_new_text(&raw, false);
                }

                "DB Next operator" | "DB Previous operator" | "DB Next warning" => {
                    let d = self.db.as_mut().unwrap();
                    let p = d
                        .plan
                        .as_ref()
                        .ok_or("Open or obtain an execution plan first")?;
                    if !p.nodes.is_empty() {
                        d.node = if name == "DB Previous operator" {
                            (d.node + p.nodes.len() - 1) % p.nodes.len()
                        } else if name == "DB Next warning" {
                            (1..=p.nodes.len())
                                .map(|n| (d.node + n) % p.nodes.len())
                                .find(|&n| yy_db_core::plan_visual::warning(&p.nodes[n]).is_some())
                                .unwrap_or(d.node)
                        } else {
                            (d.node + 1) % p.nodes.len()
                        };
                        d.focus_node();
                    }
                }
                "DB Next result" => {
                    let d = self.db.as_mut().unwrap();
                    if !d.summaries.is_empty() {
                        d.result = (d.result + 1) % d.summaries.len();
                        d.row = 0;
                        d.top = 0;
                        d.col = 0;
                        d.left = 0;
                        d.anchor = None;
                        d.page = None;
                        d.page();
                    }
                }
                "DB Column width" => self.db_prompt(Input::Width),
                "DB Row height" => self.db_prompt(Input::Height),
                "DB Copy cell" => {
                    let d = self.db.as_mut().unwrap();
                    let (r, c) = d.anchor.unwrap_or((d.row, d.col));
                    d.send(Request::Copy {
                        set: d.result,
                        start: r.min(d.row),
                        end: r.max(d.row),
                        left: c.min(d.col),
                        right: c.max(d.col),
                    });
                }
                "DB Export CSV"
                | "DB Export TSV"
                | "DB Export delimited"
                | "DB Export Parquet"
                | "DB Export fixed"
                | "DB Import CSV"
                | "DB Import TSV"
                | "DB Import delimited"
                | "DB Import Parquet"
                | "DB Import fixed" => {
                    self.db.as_mut().unwrap().transfer_path =
                        PathBuf::from(if name.starts_with("DB Import") {
                            ":import"
                        } else {
                            ":export"
                        });
                    if name.ends_with("fixed") {
                        self.db_prompt(Input::FixedLayout);
                    } else if name.ends_with("delimited") {
                        self.db_prompt(Input::Delimiter);
                    } else {
                        self.db.as_mut().unwrap().format = if name.ends_with("Parquet") {
                            Format::Parquet
                        } else {
                            Format::Delimited(yy_db_core::transfer::Delimited {
                                delimiter: if name.ends_with("TSV") { '\t' } else { ',' },
                                ..Default::default()
                            })
                        };
                        self.db_transfer_prompt();
                    }
                }
                "DB ER from database" => self.db.as_mut().unwrap().send(Request::Schema),
                "DB Edit ER columns" => self.db_prompt(Input::ColumnsTable),
                "DB Apply ER columns" => self.db_apply_columns()?,
                "DB Column types" => {
                    if self.tabs[self.tab].er_edit.is_none() {
                        return Err("Open an ER column grid first".into());
                    }
                    self.close_menu();
                    self.db.as_mut().unwrap().type_menu = true;
                    self.palette = Some(0);
                }
                "DB ER" => self.db.as_mut().unwrap().pane = Pane::Er,
                "DB New ER" => {
                    let d = self.db.as_mut().unwrap();
                    d.er_generation += 1;
                    d.er_view = None;
                    d.model = Some(Model {
                        version: 4,
                        kind: d.profiles.get(d.profile).map_or(DbKind::Sqlite, |p| p.kind),
                        entities: vec![],
                        relations: vec![],
                        names: Default::default(),
                        level: Default::default(),
                    });
                    d.pane = Pane::Er;
                }
                "DB Open ER" => self.db_prompt(Input::OpenEr),
                "DB Save ER" => self.db_prompt(Input::SaveEr),
                "DB ER Markdown" => self.db_prompt(Input::ErMarkdown),
                "DB Important fields" => {
                    let d = self.db.as_mut().unwrap();
                    let m = d.model.as_mut().ok_or("No ER model")?;
                    m.level = if m.level == yy_db_core::schema::Level::Important {
                        yy_db_core::schema::Level::Types
                    } else {
                        yy_db_core::schema::Level::Important
                    };
                }
                "DB Add ER table" => self.db_prompt(Input::EntityName),
                "DB Apply relation" => self.db_prompt(Input::RelationSuggestion),
                "DB Recommend relations" => {
                    let d = self.db.as_mut().unwrap();
                    let m = d.model.as_ref().ok_or("No ER model")?;
                    d.suggestions = m.recommend_logical_relations(100);
                    d.messages = d
                        .suggestions
                        .iter()
                        .enumerate()
                        .map(|(i, r)| {
                            format!(
                                "{}. {} ({}) -> {} ({})\n   {}",
                                i + 1,
                                r.relation.child.display(),
                                r.relation.columns.join(","),
                                r.relation.parent.display(),
                                r.relation.referenced_columns.join(","),
                                r.reason
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    d.pane = Pane::Messages;
                }
                "DB ER DDL" => {
                    let m = self
                        .db
                        .as_ref()
                        .unwrap()
                        .model
                        .as_ref()
                        .ok_or("No ER model")?;
                    let ddl = m.ddl()?;
                    self.db_new_text(&ddl.sql, true);
                    self.status = "DDL template: review before executing".into();
                }
                "DB Edit ER YAML" => {
                    let m = self
                        .db
                        .as_ref()
                        .unwrap()
                        .model
                        .as_ref()
                        .ok_or("No ER model")?;
                    let yaml = yy_db_core::schema::ErDocument::new(
                        m.clone(),
                        self.db.as_ref().unwrap().er_view.clone(),
                    )
                    .to_yaml()?;
                    self.db_new_text(&yaml, false);
                    self.status = "Edit ER YAML, Save as, then Database > Open ER".into();
                }
                "DB Performance SQL" => {
                    let d = self.db.as_mut().unwrap();
                    let kind = d.profiles.get(d.profile).map_or(DbKind::Sqlite, |p| p.kind);
                    d.messages = yy_db_core::diagnostics::templates(kind)?
                        .iter()
                        .enumerate()
                        .map(|(i, t)| format!("{}. {}\n   {}", i + 1, t.title, t.requirements))
                        .collect::<Vec<_>>()
                        .join("\n");
                    d.pane = Pane::Messages;
                    self.db_prompt(Input::Diagnostic);
                }
                _ => return Err("Unknown database command".into()),
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.status = e.clone();
            self.db.as_mut().unwrap().messages = e;
        }
        true
    }
    fn db_new_text(&mut self, text: &str, sql: bool) {
        let mut e = Editor::empty();
        e.database = sql;
        e.syntax_language = Some(if sql { "sql" } else { "yaml" }.into());
        e.doc.insert_text(text, false);
        self.tabs.push(e);
        self.tab = self.tabs.len() - 1;
        self.view = if sql { View::Database } else { View::Editor };
        if sql {
            self.db.as_mut().unwrap().pane = Pane::Query;
        }
    }
    fn db_transfer_prompt(&mut self) {
        let import = self.db.as_ref().unwrap().transfer_path == std::path::Path::new(":import");
        self.db_prompt(if import {
            Input::ImportPath
        } else {
            Input::ExportPath
        });
    }
    pub fn db_submit(&mut self, kind: Input, input: String) {
        let result = (|| -> Result<(), String> {
            let d = self.db.as_mut().ok_or("No database view")?;
            d.tree_cache = None;
            match kind {
                Input::ColumnsTable => {
                    let model = d.model.as_ref().ok_or("No ER model")?;
                    let i = model
                        .entities
                        .iter()
                        .position(|e| e.id.display() == input || e.id.name == input)
                        .ok_or("Table not found")?;
                    self.db_edit_columns(i)?;
                }
                Input::RelationSuggestion => {
                    let n = input
                        .parse::<usize>()
                        .map_err(|_| "Enter a suggestion number")?;
                    let relation = d
                        .suggestions
                        .get(n.saturating_sub(1))
                        .filter(|_| n > 0)
                        .ok_or("Suggestion number out of range")?
                        .relation
                        .clone();
                    d.model
                        .as_mut()
                        .ok_or("No ER model")?
                        .replace_relation(None, relation)?;
                    d.er_generation += 1;
                    self.status = "Logical relation added (not a database constraint)".into();
                    d.pane = Pane::Er;
                }
                Input::EntityName => {
                    let model = d.model.as_mut().ok_or("Create or load an ER model first")?;
                    let (schema, name) = input.rsplit_once('.').unwrap_or(("", input.as_str()));
                    yy_db_core::sql::identifier(name)?;
                    if model
                        .entities
                        .iter()
                        .any(|e| e.id.schema == schema && e.id.name == name)
                    {
                        return Err("Table already exists".into());
                    }
                    let e = yy_db_core::schema::Entity {
                        size: None,
                        id: yy_db_core::schema::TableId {
                            schema: schema.into(),
                            name: name.into(),
                        },
                        logical_name: String::new(),
                        view: false,
                        fields: vec![yy_db_core::schema::Field {
                            important: false,
                            name: "id".into(),
                            logical_name: String::new(),
                            data_type: "INTEGER".into(),
                            nullable: false,
                            default_sql: None,
                        }],
                        keys: vec![yy_db_core::schema::Key {
                            name: format!("pk_{name}"),
                            primary: true,
                            columns: vec!["id".into()],
                        }],
                        position: serde_json::from_str(r#"{"x":0,"y":0}"#)
                            .map_err(|e| e.to_string())?,
                        visible: true,
                        indexes: vec![],
                        catalog_definition: false,
                    };
                    let mut next = model.clone();
                    next.entities.push(e);
                    next.validate()?;
                    let index = next.entities.len().saturating_sub(1);
                    if let Some(entity) = next.entities.last_mut() {
                        entity.position = yy_draw::Point::new(
                            (index % 3) as f64 * 320.,
                            (index / 3) as f64 * 240.,
                        );
                    }
                    *model = next;
                    d.er_generation += 1;
                    d.pane = Pane::Er;
                    self.status = "ER table added; Edit ER YAML to define columns and keys".into();
                }
                Input::Name => {
                    if input.trim().is_empty()
                        || d.profiles
                            .iter()
                            .enumerate()
                            .any(|(i, p)| p.name == input && Some(i) != d.draft_index)
                    {
                        return Err("Enter a unique connection name".into());
                    }
                    d.draft.name = input;
                    self.db_prompt(Input::Kind);
                }
                Input::Kind => {
                    d.draft.kind = parse_kind(&input)?;
                    self.db_prompt(
                        if matches!(
                            self.db.as_ref().unwrap().draft.kind,
                            DbKind::Sqlite | DbKind::DuckDb
                        ) {
                            Input::Path
                        } else {
                            Input::Host
                        },
                    );
                }
                Input::Path => {
                    d.draft.target = if input.is_empty() || input == ":memory:" {
                        ":memory:".into()
                    } else {
                        self.root.join(input).to_string_lossy().into_owned()
                    };
                    self.db_prompt(Input::ReadOnly);
                }
                Input::ReadOnly => {
                    if !["yes", "no"].contains(&input.as_str()) {
                        return Err("Enter yes or no".into());
                    }
                    d.draft.read_only = input == "yes";
                    self.db_save_draft()?;
                }
                Input::Host => {
                    d.host = input;
                    self.db_prompt(Input::Port);
                }
                Input::Port => {
                    d.port = input.parse().map_err(|_| "Enter a TCP port")?;
                    self.db_prompt(Input::Database);
                }
                Input::Database => {
                    d.database = input;
                    self.db_prompt(Input::Username);
                }
                Input::Username => {
                    d.username = input;
                    self.db_prompt(Input::Tls);
                }
                Input::Tls => {
                    if !["require", "prefer", "disable"].contains(&input.as_str()) {
                        return Err("TLS: require / prefer / disable".into());
                    }
                    d.tls = input;
                    self.db_prompt(Input::PasswordEnv);
                }
                Input::PasswordEnv => {
                    d.draft.password_env = input;
                    let c = yy_db_core::native::Config {
                        host: d.host.clone(),
                        port: d.port,
                        database: d.database.clone(),
                        username: d.username.clone(),
                        password: String::new(),
                        tls: d.tls.clone(),
                        ca_file: None,
                        trust_server_certificate: false,
                    };
                    c.validate()?;
                    d.draft.target = c.encode()?;
                    self.db_prompt(Input::ReadOnly);
                }
                Input::Password => {
                    let p = d
                        .profiles
                        .get(d.profile)
                        .ok_or("No selected connection")?
                        .clone();
                    d.send(Request::Connect(p, Some(input)));
                }
                Input::Select => {
                    d.profile = d
                        .profiles
                        .iter()
                        .position(|p| p.name == input)
                        .ok_or("Connection name not found")?;
                    self.status = format!("Selected connection: {}", d.profiles[d.profile].name);
                }
                Input::DeleteProfile => {
                    let i = d
                        .profiles
                        .iter()
                        .position(|p| p.name == input)
                        .ok_or("Connection name not found")?;
                    if d.active == input {
                        d.send(Request::Disconnect);
                    }
                    let mut profiles = d.profiles.clone();
                    profiles.remove(i);
                    db::save_profiles(&d.profile_path, &profiles)?;
                    d.profiles = profiles;
                    d.profile = d.profile.min(d.profiles.len().saturating_sub(1));
                }
                Input::PlanSvg => d.send(Request::PlanSvg(
                    self.root.join(input),
                    d.plan.clone().ok_or("No plan")?,
                )),
                Input::ErSvg => d.send(Request::ErSvg(
                    self.root.join(input),
                    d.model.clone().ok_or("No ER model")?,
                )),
                Input::OpenPlan => d.send(Request::LoadPlan(self.root.join(input))),
                Input::SavePlan => d.send(Request::SavePlan(
                    self.root.join(input),
                    d.plan.clone().ok_or("No plan")?,
                )),
                Input::OpenEr => d.send(Request::LoadEr(self.root.join(input))),
                Input::SaveEr => d.send(Request::SaveEr(
                    self.root.join(input),
                    yy_db_core::schema::ErDocument::new(
                        d.model.clone().ok_or("No ER model")?,
                        d.er_view.clone(),
                    ),
                )),
                Input::ErMarkdown => d.send(Request::ErMarkdown(
                    self.root.join(input),
                    d.model.clone().ok_or("No ER model")?,
                )),
                Input::ExportPath => d.send(Request::Export {
                    set: d.result,
                    path: self.root.join(input),
                    format: d.format.clone(),
                }),
                Input::ImportPath => {
                    d.transfer_path = self.root.join(input);
                    self.db_prompt(Input::ImportTable);
                }
                Input::ImportTable => {
                    let (table, mode) = input
                        .rsplit_once(';')
                        .ok_or("Use table;create or table;append")?;
                    if !["create", "append"].contains(&mode) {
                        return Err("Use create or append".into());
                    }
                    d.send(Request::Import {
                        path: d.transfer_path.clone(),
                        format: d.format.clone(),
                        table: table.into(),
                        create: mode == "create",
                    });
                }
                Input::Delimiter => {
                    let delimiter = if input == "\\t" {
                        '\t'
                    } else {
                        let mut chars = input.chars();
                        let c = chars.next().ok_or("Enter a delimiter")?;
                        if chars.next().is_some() {
                            return Err("One delimiter character required".into());
                        }
                        c
                    };
                    d.format = Format::Delimited(yy_db_core::transfer::Delimited {
                        delimiter,
                        ..Default::default()
                    });
                    self.db_transfer_prompt();
                }
                Input::FixedLayout => {
                    let spec =
                        crate::layouts::load(&self.root.join(input)).map_err(|e| e.to_string())?;
                    if spec.is_multi() {
                        return Err("Database result export requires a single record layout".into());
                    }
                    d.format = Format::Fixed {
                        layout: spec.layout.as_ref().clone(),
                        codec: spec.codec,
                        separator: spec.separator.bytes().to_vec(),
                    };
                    self.db_transfer_prompt();
                }
                Input::Width => {
                    let width = input.parse::<u16>().map_err(|_| "Width: 4..80")?;
                    if !(4..=80).contains(&width) {
                        return Err("Width: 4..80".into());
                    }
                    d.widths.insert(d.col, width);
                }
                Input::Height => {
                    let height = input.parse::<u16>().map_err(|_| "Height: 1..8")?;
                    if !(1..=8).contains(&height) {
                        return Err("Height: 1..8".into());
                    }
                    d.height = height;
                }
                Input::Diagnostic => {
                    let kind = d.profiles.get(d.profile).map_or(DbKind::Sqlite, |p| p.kind);
                    let n = input
                        .parse::<usize>()
                        .map_err(|_| "Enter a template number")?;
                    let templates = yy_db_core::diagnostics::templates(kind)?;
                    let t = templates
                        .get(n.saturating_sub(1))
                        .filter(|_| n > 0)
                        .ok_or("Template number out of range")?;
                    let sql = t.editor_text(kind, &d.active);
                    self.db_new_text(&sql, true);
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.status = e;
        }
    }
    fn db_save_draft(&mut self) -> Result<(), String> {
        let d = self.db.as_mut().unwrap();
        let mut profiles = d.profiles.clone();
        if let Some(i) = d.draft_index {
            profiles[i] = d.draft.clone();
        } else {
            profiles.push(d.draft.clone());
        }
        db::save_profiles(&d.profile_path, &profiles)?;
        d.profiles = profiles;
        d.profile = d.draft_index.unwrap_or(d.profiles.len() - 1);
        self.status = format!("Connection saved: {}", d.draft.name);
        Ok(())
    }
    pub fn db_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        if self.view != View::Database {
            return false;
        }
        if matches!(key.code, K::F(1) | K::F(10))
            || key.modifiers.contains(M::CONTROL)
                && matches!(key.code, K::Char('q' | 'w' | 's' | 't'))
        {
            return false;
        }
        if key.code == K::F(5) {
            self.db_command("DB Execute");
            return true;
        }
        if key.code == K::F(2) {
            self.db.as_mut().unwrap().pane = Pane::Query;
            return true;
        }
        if key.modifiers.contains(M::CONTROL)
            && key.code == K::Char('c')
            && (self.db.as_ref().unwrap().pane == Pane::Results
                || self.db.as_ref().unwrap().result_focus)
        {
            self.db_command("DB Copy cell");
            return true;
        }
        let d = self.db.as_mut().unwrap();
        let pane = if d.pane == Pane::Query && d.result_focus {
            Pane::Results
        } else {
            d.pane
        };
        if pane == Pane::Query {
            return false;
        }
        match pane {
            Pane::Results => {
                let count = d.count();
                let cols = d.columns();
                if !key.modifiers.contains(M::SHIFT) {
                    d.anchor = None;
                } else if d.anchor.is_none() {
                    d.anchor = Some((d.row, d.col));
                }
                match key.code {
                    K::Down => d.row = (d.row + 1).min(count.saturating_sub(1)),
                    K::Up => d.row = d.row.saturating_sub(1),
                    K::Right => d.col = (d.col + 1).min(cols.saturating_sub(1)),
                    K::Left => d.col = d.col.saturating_sub(1),
                    K::PageDown => d.row = (d.row + 20).min(count.saturating_sub(1)),
                    K::PageUp => d.row = d.row.saturating_sub(20),
                    K::Home => d.row = 0,
                    K::End => d.row = count.saturating_sub(1),
                    _ => {}
                }
                if d.row < d.top || d.row >= d.top + d.visible_rows.max(1) {
                    d.top = d.row;
                }
                if d.col < d.left || d.col >= d.left + 3 {
                    d.left = d.col;
                }
                d.page();
            }
            Pane::Plan => match key.code {
                K::Left => d.plan_left = d.plan_left.saturating_sub(4),
                K::Right => d.plan_left = d.plan_left.saturating_add(4),
                K::Up => d.plan_top = d.plan_top.saturating_sub(1),
                K::Down => d.plan_top = d.plan_top.saturating_add(1),
                K::PageDown => d.property_top = d.property_top.saturating_add(10),
                K::PageUp => d.property_top = d.property_top.saturating_sub(10),
                K::Tab => {
                    self.db_command("DB Next operator");
                }
                _ => {}
            },
            Pane::Er | Pane::Messages => match key.code {
                K::Down => d.property_top = d.property_top.saturating_add(1),
                K::Up => d.property_top = d.property_top.saturating_sub(1),
                K::PageDown => d.property_top = d.property_top.saturating_add(10),
                K::PageUp => d.property_top = d.property_top.saturating_sub(10),
                _ => {}
            },
            _ => {}
        }
        true
    }
}
fn parse_kind(s: &str) -> Result<DbKind, String> {
    DbKind::ALL
        .into_iter()
        .find(|k| {
            k.name().eq_ignore_ascii_case(s.trim())
                || matches!(k, DbKind::Db2) && s.eq_ignore_ascii_case("db2")
        })
        .ok_or("Choose one of the seven database kinds".into())
}

#[derive(Clone, Copy)]
pub enum Hit {
    Profile(usize),
    Schema(usize),
    Object(usize),
    Select(usize),
    Node(usize),
    Edge(usize),
    Cell(u64, usize),
    Result(usize),
}
impl App {
    pub fn db_mouse(
        &mut self,
        event: crossterm::event::MouseEvent,
        action: Option<MouseAction>,
        repeats: usize,
    ) -> bool {
        use crossterm::event::{MouseButton as B, MouseEventKind as E};
        let steps = 3usize.saturating_mul(repeats);
        if self.view != View::Database {
            return false;
        }
        if event.kind == E::Down(B::Right)
            && self.content_area.contains((event.column, event.row).into())
        {
            if let Some(i) = database_menu_index() {
                self.show_menu(i);
            }
            return true;
        }
        if matches!(event.kind, E::ScrollLeft | E::ScrollRight) {
            let d = self.db.as_mut().unwrap();
            if d.pane == Pane::Results {
                d.left = if event.kind == E::ScrollLeft {
                    d.left.saturating_sub(repeats)
                } else {
                    (d.left + repeats).min(d.columns().saturating_sub(1))
                };
                return true;
            }
        }
        if matches!(event.kind, E::ScrollUp | E::ScrollDown) {
            let d = self.db.as_mut().unwrap();
            let up = event.kind == E::ScrollUp;
            if d.tree_area.contains((event.column, event.row).into()) {
                d.tree_offset = if up {
                    d.tree_offset.saturating_sub(steps)
                } else {
                    d.tree_offset.saturating_add(steps)
                };
                return true;
            }
            if d.pane != Pane::Query {
                let code = if up { K::Up } else { K::Down };
                for _ in 0..steps {
                    self.db_key(crossterm::event::KeyEvent::new(code, M::NONE));
                }
                return true;
            }
        }
        if event.kind != E::Down(B::Left) {
            return false;
        }
        if self.editor_area.contains((event.column, event.row).into()) {
            self.db.as_mut().unwrap().result_focus = false;
            return false;
        }
        let Some(MouseAction::Database(hit)) = action else {
            return false;
        };
        let d = self.db.as_mut().unwrap();
        d.tree_cache = None;
        match hit {
            Hit::Profile(i) => {
                d.profile = i;
                self.db_command("DB Connect");
            }
            Hit::Schema(i) => {
                let schema = d.objects[i].schema.clone();
                if !d.expanded.remove(&schema) {
                    d.expanded.insert(schema);
                }
            }
            Hit::Object(i) => {
                if d.object_columns.remove(&i).is_none() && !d.busy {
                    d.pending_object = Some(i);
                    let o = &d.objects[i];
                    d.send(Request::Columns(o.schema.clone(), o.name.clone()));
                }
            }
            Hit::Select(i) => {
                let o = &d.objects[i];
                let kind = d.profiles.get(d.profile).map_or(DbKind::Sqlite, |p| p.kind);
                let qualified = if o.schema.is_empty() {
                    yy_db_core::sql::identifier_for(kind, &o.name)
                } else {
                    yy_db_core::sql::identifier_for(kind, &o.schema).and_then(|s| {
                        Ok(format!(
                            "{s}.{}",
                            yy_db_core::sql::identifier_for(kind, &o.name)?
                        ))
                    })
                };
                match qualified {
                    Ok(name) => {
                        self.db_new_text(&format!("SELECT * FROM {name};\n"), true);
                        self.status = "SELECT template: review and execute".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            Hit::Edge(i) => {
                d.node = i;
                d.plan_edge = true;
                d.property_top = 0;
            }
            Hit::Node(i) => {
                d.plan_edge = false;
                d.node = i;
                d.property_top = 0;
            }
            Hit::Cell(row, col) => {
                d.result_focus = true;
                if event.modifiers.contains(M::SHIFT) {
                    d.anchor.get_or_insert((d.row, d.col));
                } else {
                    d.anchor = None;
                }
                d.row = row;
                d.col = col;
            }
            Hit::Result(i) => {
                d.result = i;
                d.row = 0;
                d.top = 0;
                d.col = 0;
                d.page = None;
                d.page();
            }
        }
        true
    }
}
fn database_menu_index() -> Option<usize> {
    crate::MENUS.iter().position(|m| m.name == "Database")
}
fn border(title: impl Into<String>) -> Block<'static> {
    Block::bordered()
        .title(title.into())
        .border_style(Style::default().fg(Color::DarkGray))
}
fn button(frame: &mut Frame, app: &mut App, area: Rect, text: &str, command: &'static str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Cyan)),
        area,
    );
    app.mouse_actions.push((area, MouseAction::Run(command)));
}
pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    app.ensure_db();
    app.preview_area = Rect::default();
    app.preview_focus = app.db.as_ref().unwrap().result_focus;
    app.editor_area = Rect::default();
    let [tools, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let mut x = tools.x;
    let mut y = tools.y;
    for (text, cmd) in [
        ("[Connect]", "DB Connect"),
        ("[SQL]", "DB Query"),
        ("[Run]", "DB Execute"),
        ("[Stop]", "DB Cancel"),
        ("[Estimated]", "DB Estimated plan"),
        ("[Actual: executes SQL]", "DB Actual plan"),
        ("[Results]", "DB Results"),
        ("[Plan]", "DB Plan"),
        ("[ER]", "DB ER"),
        ("[Messages]", "DB Messages"),
    ] {
        let w = (text.len() + 1).min(tools.width as usize) as u16;
        if x + w > tools.right() {
            x = tools.x;
            y += 1;
        }
        if y < tools.bottom() {
            button(frame, app, Rect::new(x, y, w, 1), text, cmd);
        }
        x += w;
    }
    let [tree, content] = if body.width >= 64 {
        Layout::horizontal([Constraint::Length(26), Constraint::Min(0)]).areas(body)
    } else {
        [Rect::default(), body]
    };
    draw_tree(frame, app, tree);
    app.db.as_mut().unwrap().data_area = content;
    match app.db.as_ref().unwrap().pane {
        Pane::Query => {
            let [q, result] = if content.height >= 10 {
                Layout::vertical([Constraint::Percentage(45), Constraint::Min(0)]).areas(content)
            } else {
                [content, Rect::default()]
            };
            let b = border("SQL editor · F5 execute");
            let inner = b.inner(q);
            frame.render_widget(b, q);
            if inner.width > 0 && inner.height > 0 {
                crate::ui::draw_editor(frame, app, inner);
            }
            draw_results(frame, app, result);
        }
        Pane::Results => draw_results(frame, app, content),
        Pane::Plan => draw_plan(frame, app, content),
        Pane::Er => draw_er(frame, app, content),
        Pane::Messages => {
            let d = app.db.as_ref().unwrap();
            frame.render_widget(
                Paragraph::new(yyterminal_editor::mermaid::safe(&d.messages))
                    .wrap(Wrap { trim: false })
                    .scroll((d.property_top.min(u16::MAX as usize) as u16, 0))
                    .block(border("Database messages")),
                content,
            );
        }
    }
    if app.db.as_ref().unwrap().busy && tools.height > 0 {
        frame.render_widget(
            Paragraph::new("[DB working]").style(Style::default().fg(Color::Yellow)),
            Rect::new(
                tools.right().saturating_sub(12).max(tools.x),
                tools.y,
                12.min(tools.width),
                1,
            ),
        );
    }
}
fn build_tree_rows(d: &State) -> Vec<(String, Option<Hit>)> {
    let mut rows: Vec<(String, Option<Hit>)> = vec![("[+] New connection".into(), None)];
    for (i, p) in d.profiles.iter().enumerate() {
        rows.push((
            format!("{} {}", if d.active == p.name { "●" } else { "○" }, p.name),
            Some(Hit::Profile(i)),
        ));
    }
    let mut schema = String::new();
    let mut category = String::new();
    for (i, o) in d.objects.iter().enumerate() {
        if o.schema != schema || category.is_empty() {
            schema = o.schema.clone();
            category.clear();
            rows.push((
                format!(
                    "{} {}",
                    if d.expanded.contains(&schema) {
                        "[-]"
                    } else {
                        "[+]"
                    },
                    if schema.is_empty() {
                        "(default)"
                    } else {
                        &schema
                    }
                ),
                Some(Hit::Schema(i)),
            ));
        }
        if !d.expanded.contains(&schema) {
            continue;
        }
        if o.kind != category {
            category = o.kind.clone();
            rows.push((format!("  {category}"), None));
        }
        rows.push((
            format!(
                "    {} {}",
                if d.object_columns.contains_key(&i) {
                    "[-]"
                } else {
                    "[+]"
                },
                o.name
            ),
            Some(Hit::Select(i)),
        ));
        if let Some(cols) = d.object_columns.get(&i) {
            for c in cols {
                rows.push((format!("      {}: {}", c.name, c.data_type), None));
            }
        }
    }
    rows
}
fn draw_tree(frame: &mut Frame, app: &mut App, area: Rect) {
    let b = border("Object Explorer");
    let inner = b.inner(area);
    frame.render_widget(b, area);
    app.db.as_mut().unwrap().tree_area = inner;
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let d = app.db.as_mut().unwrap();
    if d.tree_cache.is_none() {
        d.tree_cache = Some(build_tree_rows(d));
    }
    let rows = d.tree_cache.as_ref().unwrap();
    let offset = d
        .tree_offset
        .min(rows.len().saturating_sub(inner.height as usize));
    for (y, (text, hit)) in rows
        .iter()
        .skip(offset)
        .take(inner.height as usize)
        .cloned()
        .enumerate()
    {
        let rect = Rect::new(inner.x, inner.y + y as u16, inner.width, 1);
        frame.render_widget(
            Paragraph::new(yyterminal_editor::mermaid::safe(&text))
                .style(Style::default().fg(Color::Gray)),
            rect,
        );
        if let Some(hit) = hit {
            app.mouse_actions.push((rect, MouseAction::Database(hit)));
            if let Hit::Select(i) = hit {
                app.mouse_actions.push((
                    Rect::new(rect.x + 4, rect.y, 3.min(rect.width.saturating_sub(4)), 1),
                    MouseAction::Database(Hit::Object(i)),
                ));
            }
        } else if text == "[+] New connection" {
            app.mouse_actions
                .push((rect, MouseAction::Run("DB New connection")));
        }
    }
}
fn draw_results(frame: &mut Frame, app: &mut App, area: Rect) {
    let b = border("Results · arrows / Shift+arrows select · Ctrl+C copy");
    let inner = b.inner(area);
    frame.render_widget(b, area);
    if inner.height < 3 || inner.width < 4 {
        return;
    }
    let d = app.db.as_mut().unwrap();
    let mut x = inner.x;
    for (i, s) in d.summaries.iter().enumerate() {
        let text = format!("[{}: {} rows]", i + 1, s.count);
        let w = (text.len() as u16).min(inner.right().saturating_sub(x));
        if w == 0 {
            break;
        }
        let r = Rect::new(x, inner.y, w, 1);
        frame.render_widget(
            Paragraph::new(text).style(if i == d.result {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Cyan)
            }),
            r,
        );
        app.mouse_actions
            .push((r, MouseAction::Database(Hit::Result(i))));
        x += w + 1;
    }
    let Some(summary) = d.summaries.get(d.result) else {
        return;
    };
    let mut x = inner.x + 6;
    let mut columns = vec![];
    for (c, column) in summary.columns.iter().enumerate().skip(d.left) {
        if x >= inner.right() {
            break;
        }
        let natural = if let Some(w) = d.widths.get(&c) {
            *w
        } else if let Some(w) = d.auto_widths.get(&(d.result, c)) {
            *w
        } else {
            let measured = d
                .page
                .as_ref()
                .filter(|p| p.set == d.result)
                .map_or(0, |p| {
                    p.rows
                        .iter()
                        .take(32)
                        .filter_map(|r| r.get(c))
                        .map(|v| {
                            unicode_width::UnicodeWidthStr::width(
                                db::display_prefix(v, 128).as_str(),
                            )
                        })
                        .max()
                        .unwrap_or(0)
                });
            let w = unicode_width::UnicodeWidthStr::width(column.name.as_str())
                .max(measured)
                .clamp(8, 28) as u16;
            if d.page.as_ref().is_some_and(|p| p.set == d.result) {
                d.auto_widths.insert((d.result, c), w);
            }
            w
        };
        let width = natural.min(inner.right() - x);
        if width < 2 {
            break;
        }
        columns.push((c, x, width));
        let title = format!("{} {}", yy_sheet::col_name(c as u32), column.name);
        frame.render_widget(
            Paragraph::new(title).style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(40, 60, 80))
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(x, inner.y + 1, width, 1),
        );
        x += width + 1;
    }
    frame.render_widget(
        Paragraph::new(" #").style(Style::default().fg(Color::Gray)),
        Rect::new(inner.x, inner.y + 1, 6.min(inner.width), 1),
    );
    d.visible_rows = u64::from(inner.height.saturating_sub(3) / d.height.max(1));
    let mut y = inner.y + 3;
    let mut row = d.top;
    while y < inner.bottom() && row < summary.count {
        let height = d.height.min(inner.bottom() - y);
        frame.render_widget(
            Paragraph::new(format!("{}", row + 1))
                .alignment(Alignment::Right)
                .style(Style::default().fg(Color::Gray)),
            Rect::new(inner.x, y, 5.min(inner.width), 1),
        );
        let values = d
            .page
            .as_ref()
            .filter(|p| p.set == d.result && row >= p.start)
            .and_then(|p| p.rows.get((row - p.start) as usize));
        for &(c, x, width) in &columns {
            let cell = values.and_then(|v| v.get(c));
            let text = cell.map_or(String::new(), |v| {
                if *v == Value::Null {
                    "NULL".into()
                } else {
                    yyterminal_editor::mermaid::safe(&db::display_prefix(
                        v,
                        usize::from(width) * usize::from(height) * 4 + 16,
                    ))
                }
            });
            let selected = d.anchor.map_or(row == d.row && c == d.col, |(r, col)| {
                row >= r.min(d.row)
                    && row <= r.max(d.row)
                    && c >= col.min(d.col)
                    && c <= col.max(d.col)
            });
            let mut style = Style::default()
                .bg(if row.is_multiple_of(2) {
                    Color::Rgb(22, 27, 34)
                } else {
                    Color::Rgb(30, 37, 46)
                })
                .fg(if matches!(cell, Some(Value::Null)) {
                    Color::DarkGray
                } else {
                    Color::White
                });
            if selected {
                style = style
                    .bg(Color::Rgb(35, 85, 130))
                    .add_modifier(Modifier::BOLD);
            }
            let rect = Rect::new(x, y, width, height);
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .alignment(
                        if matches!(
                            cell,
                            Some(Value::Integer(_) | Value::Decimal(_) | Value::Float(_))
                        ) {
                            Alignment::Right
                        } else {
                            Alignment::Left
                        },
                    )
                    .style(style),
                rect,
            );
            app.mouse_actions
                .push((rect, MouseAction::Database(Hit::Cell(row, c))));
            if x + width < inner.right() {
                frame.render_widget(
                    Paragraph::new("│\n".repeat(height as usize))
                        .style(Style::default().fg(Color::DarkGray)),
                    Rect::new(x + width, y, 1, height),
                );
            }
        }
        y += height;
        row += 1;
    }
    frame.render_widget(
        Paragraph::new("─".repeat(inner.width as usize))
            .style(Style::default().fg(Color::DarkGray)),
        Rect::new(inner.x, inner.y + 2, inner.width, 1),
    );
    d.page();
}
fn draw_plan(frame: &mut Frame, app: &mut App, area: Rect) {
    let [graph, props] = if area.width >= 90 {
        Layout::horizontal([Constraint::Percentage(67), Constraint::Min(0)]).areas(area)
    } else {
        Layout::vertical([Constraint::Percentage(65), Constraint::Min(0)]).areas(area)
    };
    let b = border("Plan · right → left · Tab next operator · arrows pan");
    let inner = b.inner(graph);
    frame.render_widget(b, graph);
    let d = app.db.as_mut().unwrap();
    if let (Some(plan), Some(canvas)) = (&d.plan, &d.canvas) {
        for (rect, node) in canvas.draw(frame, inner, plan, d.node, d.plan_left, d.plan_top) {
            app.mouse_actions.push((
                rect,
                MouseAction::Database(match node {
                    yy_db_core::plan_visual::Hit::Node(i) => Hit::Node(i),
                    yy_db_core::plan_visual::Hit::Edge(i) => Hit::Edge(i),
                }),
            ));
        }
        let b = border("Operator properties · PgUp/PgDn");
        let inner = b.inner(props);
        frame.render_widget(b, props);
        d.properties_cache.prepare(
            [d.plan_revision, d.node as u64, u64::from(d.plan_edge)],
            inner.width,
            || {
                yy_db_core::plan_visual::details(plan, d.node, d.plan_edge)
                    .into_iter()
                    .map(|(key, value)| {
                        Line::from(vec![
                            Span::styled(format!("{key}: "), Style::default().fg(Color::Cyan)),
                            Span::raw(yyterminal_editor::mermaid::safe(&value)),
                        ])
                    })
                    .collect()
            },
        );
        d.properties_cache.paint(frame, inner, d.property_top);
    } else {
        frame.render_widget(
            Paragraph::new("Database > Estimated plan / Actual plan / Open plan"),
            inner,
        );
    }
}
fn er_lines(m: &Model) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(
        format!(
            "{} · {} tables · {} relations",
            m.kind.name(),
            m.entities.len(),
            m.relations.len()
        ),
        Style::default().fg(Color::Cyan),
    )];
    for e in m.entities.iter().filter(|e| e.visible) {
        let fields = e
            .fields
            .iter()
            .filter(|f| m.level != yy_db_core::schema::Level::Important || m.important_field(e, f))
            .collect::<Vec<_>>();
        let width = 36;
        lines.push(Line::raw(format!("┌{}┐", "─".repeat(width))));
        lines.push(Line::styled(
            format!("│ {}", e.id.display()),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw(format!("├{}┤", "─".repeat(width))));
        for f in fields {
            let pk = e
                .keys
                .iter()
                .any(|k| k.primary && k.columns.contains(&f.name));
            let fk = m
                .relations
                .iter()
                .any(|r| r.child == e.id && r.columns.contains(&f.name));
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "│ {} ",
                        if pk {
                            "PK"
                        } else if fk {
                            "FK"
                        } else if f.important {
                            "★ "
                        } else {
                            "  "
                        }
                    ),
                    Style::default().fg(if pk { Color::Yellow } else { Color::Cyan }),
                ),
                Span::raw(format!(
                    "{}  {}{}",
                    f.name,
                    f.data_type,
                    if f.nullable { "" } else { " NOT NULL" }
                )),
            ]));
        }
        lines.push(Line::raw(format!("└{}┘", "─".repeat(width))));
    }
    for r in &m.relations {
        lines.push(Line::styled(
            format!(
                "{} ({}) {} {} ({})  [{}]",
                r.child.display(),
                r.columns.join(","),
                if r.logical {
                    "···▶"
                } else {
                    "───▶"
                },
                r.parent.display(),
                r.referenced_columns.join(","),
                r.name
            ),
            Style::default().fg(if r.logical {
                Color::Magenta
            } else {
                Color::Green
            }),
        ));
    }
    lines
}
fn draw_er(frame: &mut Frame, app: &mut App, area: Rect) {
    let d = app.db.as_mut().unwrap();
    let b = border("ER · PK/FK/★ · edit YAML / generate DDL / export Markdown");
    let inner = b.inner(area);
    frame.render_widget(b, area);
    let Some(m) = &d.model else {
        frame.render_widget(
            Paragraph::new("Database > ER from database / New ER / Open ER"),
            inner,
        );
        return;
    };
    d.er_cache
        .prepare([d.er_generation, m.level as u64, 0], inner.width, || {
            er_lines(m)
        });
    d.er_cache.paint(frame, inner, d.property_top);
}

impl App {
    pub fn db_type_choices(&self) -> &'static [&'static str] {
        let kind = self
            .db
            .as_ref()
            .and_then(|d| d.model.as_ref())
            .map_or(DbKind::Sqlite, |m| m.kind);
        match kind {
            DbKind::Sqlite => &["INTEGER", "TEXT", "REAL", "BLOB", "NUMERIC"],
            DbKind::DuckDb => &[
                "INTEGER",
                "BIGINT",
                "HUGEINT",
                "VARCHAR",
                "BOOLEAN",
                "DECIMAL(18,2)",
                "DATE",
                "TIMESTAMP",
                "DOUBLE",
                "BLOB",
                "UUID",
                "JSON",
            ],
            DbKind::PostgreSql => &[
                "integer",
                "bigint",
                "text",
                "varchar(255)",
                "boolean",
                "numeric(18,2)",
                "date",
                "timestamp",
                "timestamptz",
                "uuid",
                "jsonb",
                "bytea",
            ],
            DbKind::SqlServer => &[
                "int",
                "bigint",
                "nvarchar(255)",
                "nvarchar(max)",
                "bit",
                "decimal(18,2)",
                "date",
                "datetime2",
                "uniqueidentifier",
                "varbinary(max)",
            ],
            DbKind::MySql => &[
                "INT",
                "BIGINT",
                "VARCHAR(255)",
                "TEXT",
                "BOOLEAN",
                "DECIMAL(18,2)",
                "DATE",
                "DATETIME",
                "TIMESTAMP",
                "JSON",
                "BLOB",
            ],
            DbKind::Oracle => &[
                "NUMBER",
                "NUMBER(18,2)",
                "VARCHAR2(255)",
                "NVARCHAR2(255)",
                "DATE",
                "TIMESTAMP",
                "CLOB",
                "BLOB",
                "RAW(16)",
                "BINARY_DOUBLE",
            ],
            DbKind::Db2 => &[
                "INTEGER",
                "BIGINT",
                "VARCHAR(255)",
                "CLOB",
                "DECIMAL(18,2)",
                "DATE",
                "TIME",
                "TIMESTAMP",
                "BLOB",
                "DOUBLE",
            ],
        }
    }
    pub fn db_pick_type(&mut self, name: &str) {
        self.close_menu();
        if let Some(book) = &mut self.tabs[self.tab].book {
            book.col = 3;
            self.book_op(crate::workbook::Op::Set(name.into()));
        }
    }
    fn db_edit_columns(&mut self, index: usize) -> Result<(), String> {
        let d = self.db.as_ref().unwrap();
        let m = d.model.as_ref().ok_or("No ER model")?;
        let table = &m.entities[index];
        let mut book = crate::workbook::Book::empty().map_err(|e| e.to_string())?;
        let mut rows = vec![vec![
            "Original index".to_owned(),
            "Physical name".into(),
            "Logical name".into(),
            "Type".into(),
            "Nullable".into(),
            "Important".into(),
            "Default SQL".into(),
        ]];
        for (i, f) in table.fields.iter().enumerate() {
            rows.push(vec![
                i.to_string(),
                f.name.clone(),
                f.logical_name.clone(),
                f.data_type.clone(),
                f.nullable.to_string(),
                f.important.to_string(),
                f.default_sql.clone().unwrap_or_default(),
            ]);
        }
        for (r, row) in rows.into_iter().enumerate() {
            for (c, text) in row.into_iter().enumerate() {
                book.doc.book.sheets[0]
                    .set(
                        &book.doc.ctx,
                        r as u64,
                        c as u32,
                        yy_sheet::Value::Text(text.into()),
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        let mut e = Editor::empty();
        e.spreadsheet = true;
        e.er_edit = Some((d.er_generation, index));
        e.book = Some(book);
        self.tabs.push(e);
        self.tab = self.tabs.len() - 1;
        self.view = View::Sheet;
        self.status =
            "ER column grid: copy/paste, choose column types, then Apply ER columns".into();
        Ok(())
    }
    fn db_apply_columns(&mut self) -> Result<(), String> {
        let e = &self.tabs[self.tab];
        let (generation, index) = e.er_edit.ok_or("Select the ER column grid tab")?;
        let book = e.book.as_ref().ok_or("Wait for the sheet operation")?;
        if book.current().view.rows.is_some() {
            return Err("Clear sorting and filtering before applying column definitions".into());
        }
        if book.current().extent().0 > 4097 {
            return Err("ER tables support at most 4096 columns".into());
        }
        let get = |r, c| {
            book.value(r, c)
                .map(|v| v.general_text())
                .map_err(|e| e.to_string())
        };
        if get(0, 0)? != "Original index" || get(0, 3)? != "Type" {
            return Err("Keep the definition header row and column order".into());
        }
        let boolean = |s: String| -> Result<bool, String> {
            match s.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" => Ok(true),
                "false" | "no" | "0" | "" => Ok(false),
                _ => Err("Nullable/Important must be true or false".into()),
            }
        };
        let mut rows = vec![];
        for r in 1..book.current().extent().0 {
            let name = get(r, 1)?;
            if name.is_empty() {
                continue;
            }
            let id = get(r, 0)?;
            let id = if id.is_empty() {
                None
            } else {
                Some(
                    id.parse::<usize>()
                        .map_err(|_| "Original index must be a number or blank for a new column")?,
                )
            };
            let default = get(r, 6)?;
            rows.push((
                id,
                yy_db_core::schema::Field {
                    name,
                    logical_name: get(r, 2)?,
                    data_type: get(r, 3)?,
                    nullable: boolean(get(r, 4)?)?,
                    important: boolean(get(r, 5)?)?,
                    default_sql: (!default.is_empty()).then_some(default),
                },
            ));
        }
        let d = self.db.as_mut().unwrap();
        if d.er_generation != generation {
            return Err(
                "ER model changed since this grid was opened; reopen the column grid".into(),
            );
        }
        d.model
            .as_mut()
            .ok_or("No ER model")?
            .replace_fields(index, &rows)?;
        d.er_generation += 1;
        d.pane = Pane::Er;
        self.view = View::Database;
        self.status = "ER column definitions applied".into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn idle(app: &mut App) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while app.db.as_ref().is_some_and(|d| d.busy) {
            app.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "Database did not complete: {}",
                app.status
            );
            std::thread::yield_now();
        }
    }
    #[test]
    #[ignore = "manual generation of user documentation from captured live DB plans"]
    fn capture_live_plan_documentation() -> anyhow::Result<()> {
        let source =
            PathBuf::from(std::env::var_os("YYTE_PLAN_EXAMPLES_INPUT").ok_or_else(|| {
                anyhow::anyhow!(
                    "Set YYTE_PLAN_EXAMPLES_INPUT to successful database-live report directory"
                )
            })?);
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/plan-examples");
        std::fs::create_dir_all(&output)?;
        for service in ["postgres", "mysql", "sqlserver", "oracle", "db2"] {
            let filename = if service == "db2" {
                "false.yyplan"
            } else {
                "true.yyplan"
            };
            let plan = yy_db_core::plan_file::load(&source.join(service).join(filename))
                .map_err(anyhow::Error::msg)?;
            let temporary = tempfile::tempdir()?;
            let mut app = App::new(temporary.path().to_owned());
            app.ensure_db();
            let d = app.db.as_mut().unwrap();
            d.node = if service == "sqlserver" { 1 } else { 0 };
            d.canvas = Some(Canvas::new(&plan).map_err(anyhow::Error::msg)?);
            d.plan = Some(plan);
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(180, 36))?;
            terminal.draw(|f| draw_plan(f, &mut app, f.area()))?;
            fn color(c: Color, default: &str) -> String {
                match c {
                    Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
                    Color::Black => "#161b22".into(),
                    Color::White => "#e6edf3".into(),
                    Color::Red => "#ff7b72".into(),
                    Color::Green => "#7ee787".into(),
                    Color::Yellow => "#f2cc60".into(),
                    Color::Blue => "#58a6ff".into(),
                    Color::Cyan => "#79c0ff".into(),
                    Color::Gray => "#b1bac4".into(),
                    Color::DarkGray => "#6e7681".into(),
                    _ => default.into(),
                }
            }
            let mut rows = vec![];
            let mut text = String::new();
            for y in 0..36 {
                let mut row = vec![];
                let mut x = 0;
                while x < 180 {
                    let c = &terminal.backend().buffer()[(x, y)];
                    let width = unicode_width::UnicodeWidthStr::width(c.symbol()).max(1) as u16;
                    row.push(serde_json::json!({"x":x,"text":c.symbol(),"fg":color(c.fg,"#e6edf3"),"bg":color(c.bg,"#0d1117"),"bold":c.modifier.contains(Modifier::BOLD)}));
                    text.push_str(c.symbol());
                    x += width;
                }
                text.push('\n');
                rows.push(row);
            }
            assert!(text.contains("Query 1") && text.contains("Operator properties"));
            std::fs::write(output.join(format!("{service}.txt")), text)?;
            std::fs::write(
                output.join(format!("{service}.json")),
                serde_json::to_vec(&rows)?,
            )?;
        }
        Ok(())
    }
    #[test]
    fn sql_tabs_grid_clipboard_and_plan_properties_work_at_small_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_owned());
        app.command("DB Manager");
        assert!(app.tabs[app.tab].database);
        app.command("DB Connect");
        idle(&mut app);
        assert!(app.status.starts_with("Connected:"));
        app.tabs[app.tab].doc.insert_text("SELECT 9223372036854775807 AS exact_integer, '東京' AS city UNION ALL SELECT 42,'大阪';",false);
        app.command("DB Execute");
        idle(&mut app);
        assert_eq!(app.db.as_ref().unwrap().count(), 2);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 32)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(screen.contains("9223372036854775807"));
        assert!(screen.contains("exact_integer"));
        assert!(screen.contains('東') && screen.contains('京'));
        app.db.as_mut().unwrap().anchor = Some((0, 0));
        app.db.as_mut().unwrap().row = 1;
        app.db.as_mut().unwrap().col = 1;
        app.command("DB Copy cell");
        idle(&mut app);
        assert_eq!(app.clipboard, "9223372036854775807\t東京\n42\t大阪");
        app.command("DB Estimated plan");
        idle(&mut app);
        assert_eq!(
            app.db.as_ref().unwrap().plan.as_ref().unwrap().kind,
            DbKind::Sqlite
        );
        for (w, h) in [(60, 18), (40, 12), (8, 4), (1, 1)] {
            let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            for pane in [
                Pane::Query,
                Pane::Results,
                Pane::Plan,
                Pane::Er,
                Pane::Messages,
            ] {
                app.db.as_mut().unwrap().pane = pane;
                t.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            }
        }
        let sqltab = app.tab;
        app.command("New file");
        app.select_tab(sqltab);
        assert!(app.view == View::Database);
    }
    #[test]
    fn er_column_spreadsheet_types_and_atomic_apply_preserve_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_owned());
        app.command("DB Manager");
        app.command("DB New ER");
        app.db_submit(Input::EntityName, "customers".into());
        app.db_submit(Input::ColumnsTable, "customers".into());
        assert!(app.view == View::Sheet);
        {
            let book = app.tabs[app.tab].book.as_mut().unwrap();
            book.doc.book.sheets[0]
                .set(
                    &book.doc.ctx,
                    1,
                    1,
                    yy_sheet::Value::Text("customer_id".into()),
                )
                .unwrap();
            book.doc.book.sheets[0]
                .set(&book.doc.ctx, 1, 5, yy_sheet::Value::Text("true".into()))
                .unwrap();
            book.row = 1;
        }
        app.command("DB Column types");
        assert!(app.db.as_ref().unwrap().type_menu);
        assert_eq!(app.menu_commands()[0], "INTEGER");
        app.run_menu(0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.busy {
            app.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        app.command("DB Apply ER columns");
        let model = app.db.as_ref().unwrap().model.as_ref().unwrap();
        assert_eq!(
            model.entities[0].fields[0].name, "customer_id",
            "{}",
            app.status
        );
        assert_eq!(model.entities[0].keys[0].columns, ["customer_id"]);
        assert!(model.entities[0].fields[0].important);
    }
    #[test]
    fn connection_wizard_saves_memory_target_and_masked_native_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_owned());
        app.command("DB Manager");
        app.db_submit(Input::Name, "demo".into());
        app.db_submit(Input::Kind, "SQLite".into());
        app.db_submit(Input::Path, ":memory:".into());
        app.db_submit(Input::ReadOnly, "no".into());
        let d = app.db.as_ref().unwrap();
        assert_eq!(d.profiles.last().unwrap().target, ":memory:");
        assert_eq!(db::load_profiles(&d.profile_path).unwrap().len(), 3);
        app.db_submit(Input::Name, "remote".into());
        app.db_submit(Input::Kind, "PostgreSQL".into());
        app.db_submit(Input::Host, "localhost".into());
        app.db_submit(Input::Port, "5432".into());
        app.db_submit(Input::Database, "demo".into());
        app.db_submit(Input::Username, "test".into());
        app.db_submit(Input::Tls, "require".into());
        app.db_submit(Input::PasswordEnv, "DB_PASSWORD".into());
        app.db_submit(Input::ReadOnly, "yes".into());
        let d = app.db.as_ref().unwrap();
        let p = d.profiles.last().unwrap();
        assert!(p.read_only);
        assert_eq!(p.password_env, "DB_PASSWORD");
        assert!(
            yy_db_core::native::Config::decode(&p.target)
                .unwrap()
                .unwrap()
                .password
                .is_empty()
        );
    }
}
