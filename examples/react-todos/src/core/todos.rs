//! In-memory todo service for the demo. Serde-free domain types — the wire
//! DTOs live in the route.rs adapter that exposes this over HTTP.
//!
//! State lives in [`TodosCtx`], installed as an axum `Extension` layer in
//! `main.rs` / `api/index.rs` and extracted by handlers — the shape real apps
//! use for a DB pool. Seeded GETs taking `Extension<TodosCtx>` still get
//! their `#[nextrs::api]` companions: the companion pulls the context from
//! the request extensions the prefetch call sites already pass.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

/// Shared todo store — the demo's stand-in for a DB handle. Cheap to clone
/// (`Arc` inside), `Clone + Send + Sync + 'static` as axum's `Extension`
/// requires.
#[derive(Clone)]
pub struct TodosCtx {
    store: Arc<Mutex<Vec<Todo>>>,
}

impl Default for TodosCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl TodosCtx {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(vec![
                Todo {
                    id: 1,
                    title: "Write a page.tsx".to_string(),
                    done: true,
                },
                Todo {
                    id: 2,
                    title: "Seed the React Query cache from Rust".to_string(),
                    done: false,
                },
                Todo {
                    id: 3,
                    title: "Ship nextrs".to_string(),
                    done: false,
                },
            ])),
        }
    }

    pub async fn list(&self, open_only: bool) -> Vec<Todo> {
        let todos = self.store.lock().unwrap();
        todos
            .iter()
            .filter(|t| !open_only || !t.done)
            .cloned()
            .collect()
    }

    pub async fn get(&self, id: u64) -> Option<Todo> {
        let todos = self.store.lock().unwrap();
        todos.iter().find(|t| t.id == id).cloned()
    }

    /// The ids adjacent to `id` in list order, for prev/next navigation.
    pub async fn neighbors(&self, id: u64) -> (Option<u64>, Option<u64>) {
        let todos = self.store.lock().unwrap();
        let Some(pos) = todos.iter().position(|t| t.id == id) else {
            return (None, None);
        };
        let prev = pos.checked_sub(1).map(|p| todos[p].id);
        let next = todos.get(pos + 1).map(|t| t.id);
        (prev, next)
    }

    /// Mark a todo done/undone. Returns the updated todo, `None` if unknown.
    pub async fn set_done(&self, id: u64, done: bool) -> Option<Todo> {
        let mut todos = self.store.lock().unwrap();
        let todo = todos.iter_mut().find(|t| t.id == id)?;
        todo.done = done;
        Some(todo.clone())
    }

    pub async fn add(&self, title: String) -> Todo {
        let mut todos = self.store.lock().unwrap();
        let id = todos.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let todo = Todo {
            id,
            title,
            done: false,
        };
        todos.push(todo.clone());
        todo
    }

    /// Remove a todo by id. Returns `true` if one was removed.
    pub async fn remove(&self, id: u64) -> bool {
        let mut todos = self.store.lock().unwrap();
        let before = todos.len();
        todos.retain(|t| t.id != id);
        todos.len() != before
    }
}
