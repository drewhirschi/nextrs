//! RSX server component (docs/rsx-server-components.md): a Rust page that
//! does its data access on the server, renders HTML with `rsx!`, and embeds
//! the React `TodoStats` island (components/TodoStats.tsx) through the
//! generated typed bindings. No macro on the fn — `pub async fn page` is the
//! convention, and extractor params work exactly like `route.rs` handlers.
//!
//! Styling: the same hand-written `public/style.css` the React pages use —
//! an RSX page is ordinary HTML, so whatever styling pipeline the app has
//! (a plain stylesheet here; Tailwind output works identically) applies by
//! linking it in the document head.

use axum::Extension;
use nextrs::rsx;
use nextrs::rsx::Rsx;
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
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>"server-stats · react-todos"</title>
                <link rel="stylesheet" href="/style.css" />
                <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
            </head>
            <body>
                <main>
                    <nav class="topnav">
                        <a href="/" class="wordmark"><span>"next"<b>"rs"</b></span></a>
                        <span class="nav-tag">"server-stats"</span>
                        <span class="muted">" · rendered in Rust with rsx!"</span>
                        <a class="muted" href="/">"todos"</a>
                    </nav>

                    <div class="row">
                        <h1>"Todos, from the server"</h1>
                        <span class="badge badge-open">"rsx"</span>
                    </div>
                    <p class="muted">
                        "This document is an RSX server component: the list below is HTML "
                        "rendered in Rust, and the stats box is a React island hydrated "
                        "with typed props extracted from its TypeScript interface."
                    </p>

                    <TodoStats
                        title={Some("Live todo stats".to_string())}
                        initial_filter={TodoStatsInitialFilter::Open}
                        counts={counts}
                    />

                    <ul class="list">
                        { all.iter().map(|todo| rsx! {
                            <li class={todo.done.then_some("done")}>
                                <span class="title">{&todo.title}</span>
                                { if todo.done {
                                    rsx! { <span class="badge badge-done">"done"</span> }
                                } else {
                                    rsx! { <span class="badge badge-open">"open"</span> }
                                } }
                            </li>
                        }).collect::<Rsx>() }
                    </ul>

                    <p class="note muted">
                        "View source: the list arrived as HTML — no JavaScript rendered it. "
                        "Only the stats island shipped a bundle."
                    </p>
                </main>
            </body>
        </html>
    }
}
