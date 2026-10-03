//! Todo service for the demo. Serde-free domain types — the wire DTOs live in
//! the route.rs adapter that exposes this over HTTP.
//!
//! State lives in [`TodosCtx`], installed as an axum `Extension` layer in
//! `main.rs` / `api/index.rs` and extracted by handlers — the shape real apps
//! use for a DB pool. Seeded GETs taking `Extension<TodosCtx>` still get
//! their `#[nextrs::api]` companions: the companion pulls the context from
//! the request extensions the prefetch call sites already pass.
//!
//! Two backends, chosen at startup:
//!
//! - **Turso/libsql** when `NEXTRS_DB_URL` (or `TURSO_DATABASE_URL`) is set —
//!   production. Serverless instances don't share memory, so a deployed app
//!   needs one shared store: two tabs (or a job and a request) must see the
//!   same list. The same database holds the framework's job and log tables.
//! - **Memory** otherwise — local dev and tests, zero setup.
//!
//! Like any real data layer, every call can fail ([`TodosError`]).

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

/// Why a todo operation failed.
#[derive(Debug)]
pub enum TodosError {
    Db(String),
    /// Demo hook: adding a todo titled `boom` fails on purpose, so the request
    /// log has an error to show (see docs/react-todos-demo-plan.md, step 1).
    SimulatedFailure,
}

impl std::fmt::Display for TodosError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodosError::Db(e) => write!(f, "database: {e}"),
            TodosError::SimulatedFailure => f.write_str("simulated failure (demo: the title 'boom')"),
        }
    }
}

impl std::error::Error for TodosError {}

fn db_err(e: impl std::fmt::Display) -> TodosError {
    TodosError::Db(e.to_string())
}

const SEED: [(&str, bool); 3] = [
    ("Write a page.tsx", true),
    ("Seed the React Query cache from Rust", false),
    ("Ship nextrs", false),
];

/// Shared todo store. Cheap to clone (`Arc` inside), `Clone + Send + Sync +
/// 'static` as axum's `Extension` requires.
#[derive(Clone)]
pub struct TodosCtx {
    backend: Arc<Backend>,
}

enum Backend {
    Memory(Mutex<Vec<Todo>>),
    Libsql {
        url: String,
        token: String,
        conn: tokio::sync::OnceCell<libsql::Connection>,
    },
}

impl Default for TodosCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl TodosCtx {
    /// Turso when `NEXTRS_DB_URL` / `TURSO_DATABASE_URL` is set, else memory.
    pub fn new() -> Self {
        let var = |names: &[&str]| {
            names
                .iter()
                .find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()))
        };
        match var(&["NEXTRS_DB_URL", "TURSO_DATABASE_URL"]) {
            Some(url) => Self::libsql(url, var(&["NEXTRS_DB_TOKEN", "TURSO_AUTH_TOKEN"]).unwrap_or_default()),
            None => Self::memory(),
        }
    }

    /// In-process store seeded with the demo todos.
    pub fn memory() -> Self {
        let seed = SEED
            .iter()
            .enumerate()
            .map(|(i, (title, done))| Todo {
                id: i as u64 + 1,
                title: title.to_string(),
                done: *done,
            })
            .collect();
        Self {
            backend: Arc::new(Backend::Memory(Mutex::new(seed))),
        }
    }

    /// A Turso/libsql store (connects lazily on first use).
    pub fn libsql(url: String, token: String) -> Self {
        Self {
            backend: Arc::new(Backend::Libsql {
                url,
                token,
                conn: tokio::sync::OnceCell::new(),
            }),
        }
    }

    async fn conn(url: &str, token: &str, cell: &tokio::sync::OnceCell<libsql::Connection>) -> Result<libsql::Connection, TodosError> {
        cell.get_or_try_init(|| async {
            let db = libsql::Builder::new_remote(url.to_string(), token.to_string())
                .build()
                .await
                .map_err(db_err)?;
            let conn = db.connect().map_err(db_err)?;
            conn.execute(
                "CREATE TABLE IF NOT EXISTS todos (
                   id    INTEGER PRIMARY KEY AUTOINCREMENT,
                   title TEXT NOT NULL,
                   done  INTEGER NOT NULL DEFAULT 0
                 )",
                (),
            )
            .await
            .map_err(db_err)?;
            // First boot: the same three demo todos as the memory store.
            let mut rows = conn.query("SELECT COUNT(*) FROM todos", ()).await.map_err(db_err)?;
            let empty = match rows.next().await.map_err(db_err)? {
                Some(row) => row.get::<i64>(0).map_err(db_err)? == 0,
                None => true,
            };
            if empty {
                // Fixed ids + OR IGNORE: two cold instances racing on an
                // empty table seed the three demo todos once, not twice.
                for (i, (title, done)) in SEED.iter().enumerate() {
                    conn.execute(
                        "INSERT OR IGNORE INTO todos (id, title, done) VALUES (?1, ?2, ?3)",
                        libsql::params![i as i64 + 1, *title, *done as i64],
                    )
                    .await
                    .map_err(db_err)?;
                }
            }
            Ok(conn)
        })
        .await
        .cloned()
    }

    async fn query(&self, sql: &str, params: impl libsql::params::IntoParams) -> Result<Vec<Todo>, TodosError> {
        let Backend::Libsql { url, token, conn } = &*self.backend else {
            unreachable!("query is only called on the libsql backend");
        };
        let conn = Self::conn(url, token, conn).await?;
        let mut rows = conn.query(sql, params).await.map_err(db_err)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(db_err)? {
            out.push(Todo {
                id: row.get::<i64>(0).map_err(db_err)? as u64,
                title: row.get::<String>(1).map_err(db_err)?,
                done: row.get::<i64>(2).map_err(db_err)? != 0,
            });
        }
        Ok(out)
    }

    pub async fn list(&self, open_only: bool) -> Result<Vec<Todo>, TodosError> {
        match &*self.backend {
            Backend::Memory(store) => Ok(store
                .lock()
                .unwrap()
                .iter()
                .filter(|t| !open_only || !t.done)
                .cloned()
                .collect()),
            Backend::Libsql { .. } => {
                let sql = if open_only {
                    "SELECT id, title, done FROM todos WHERE done = 0 ORDER BY id"
                } else {
                    "SELECT id, title, done FROM todos ORDER BY id"
                };
                self.query(sql, ()).await
            }
        }
    }

    pub async fn get(&self, id: u64) -> Result<Option<Todo>, TodosError> {
        match &*self.backend {
            Backend::Memory(store) => Ok(store.lock().unwrap().iter().find(|t| t.id == id).cloned()),
            Backend::Libsql { .. } => Ok(self
                .query("SELECT id, title, done FROM todos WHERE id = ?1", libsql::params![id as i64])
                .await?
                .pop()),
        }
    }

    /// The ids adjacent to `id` in list order, for prev/next navigation.
    pub async fn neighbors(&self, id: u64) -> Result<(Option<u64>, Option<u64>), TodosError> {
        let todos = self.list(false).await?;
        let Some(pos) = todos.iter().position(|t| t.id == id) else {
            return Ok((None, None));
        };
        let prev = pos.checked_sub(1).map(|p| todos[p].id);
        let next = todos.get(pos + 1).map(|t| t.id);
        Ok((prev, next))
    }

    /// Mark a todo done/undone. Returns the updated todo, `None` if unknown.
    pub async fn set_done(&self, id: u64, done: bool) -> Result<Option<Todo>, TodosError> {
        match &*self.backend {
            Backend::Memory(store) => {
                let mut todos = store.lock().unwrap();
                Ok(todos.iter_mut().find(|t| t.id == id).map(|todo| {
                    todo.done = done;
                    todo.clone()
                }))
            }
            Backend::Libsql { .. } => Ok(self
                .query(
                    "UPDATE todos SET done = ?2 WHERE id = ?1 RETURNING id, title, done",
                    libsql::params![id as i64, done as i64],
                )
                .await?
                .pop()),
        }
    }

    pub async fn add(&self, title: String) -> Result<Todo, TodosError> {
        if title == "boom" {
            return Err(TodosError::SimulatedFailure);
        }
        match &*self.backend {
            Backend::Memory(store) => {
                let mut todos = store.lock().unwrap();
                let id = todos.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                let todo = Todo { id, title, done: false };
                todos.push(todo.clone());
                Ok(todo)
            }
            Backend::Libsql { .. } => self
                .query(
                    "INSERT INTO todos (title, done) VALUES (?1, 0) RETURNING id, title, done",
                    libsql::params![title],
                )
                .await?
                .pop()
                .ok_or_else(|| TodosError::Db("insert returned no row".into())),
        }
    }

    /// Remove a todo by id. Returns `true` if one was removed.
    pub async fn remove(&self, id: u64) -> Result<bool, TodosError> {
        match &*self.backend {
            Backend::Memory(store) => {
                let mut todos = store.lock().unwrap();
                let before = todos.len();
                todos.retain(|t| t.id != id);
                Ok(todos.len() != before)
            }
            Backend::Libsql { .. } => Ok(!self
                .query("DELETE FROM todos WHERE id = ?1 RETURNING id, title, done", libsql::params![id as i64])
                .await?
                .is_empty()),
        }
    }
}
