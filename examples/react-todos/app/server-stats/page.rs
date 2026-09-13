//! RSX server component (docs/rsx-server-components.md): a Rust page that
//! does its data access on the server, renders HTML with `rsx!`, and embeds
//! the React `TodoStats` island (components/TodoStats.tsx) through the
//! generated typed bindings. No macro on the fn — `pub async fn page` is the
//! convention, and extractor params work exactly like `route.rs` handlers.

use axum::Extension;
use nextrs::rsx::Rsx;
use nextrs::rsx;
use react_todos::client::{TodoStats, TodoStatsCounts, TodoStatsInitialFilter, TodoStatsProps};
use react_todos::core::todos::TodosCtx;

pub async fn page(Extension(todos): Extension<TodosCtx>) -> Rsx {
    // Server-only work — the demo's stand-in for a DB query.
    let all = todos.list(false).await;
    let done = all.iter().filter(|t| t.done).count();
    let counts = TodoStatsCounts {
        all: all.len() as f64,
        open: (all.len() - done) as f64,
        done: done as f64,
    };

    rsx! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <title>"Server Stats — RSX"</title>
            </head>
            <body>
                <main class="rsx-demo">
                    <h1>"Todos, rendered from Rust"</h1>
                    <p>
                        "This page is an RSX server component. The list below is server HTML; "
                        "the stats box is a React island hydrated with typed props."
                    </p>

                    <TodoStats
                        title={Some("Live todo stats".to_string())}
                        initial_filter={TodoStatsInitialFilter::Open}
                        counts={counts}
                    />

                    <ul>
                        { all.iter().map(|todo| rsx! {
                            <li data-done={todo.done}>{&todo.title}</li>
                        }).collect::<Rsx>() }
                    </ul>

                    <p><a href="/">"Back to the React app"</a></p>
                </main>
            </body>
        </html>
    }
}
