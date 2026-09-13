//! End-to-end tests for the `rsx!` macro against the `nextrs::rsx` runtime.

use nextrs::rsx;
use nextrs::rsx::Rsx;

#[test]
fn elements_text_and_holes() {
    let name = "Drew <script>";
    let count = 3u32;
    let out = rsx! {
        <main class="p-8">
            <h1>"Todos"</h1>
            <p data-count={count}>{name}</p>
        </main>
    };
    assert_eq!(
        out.as_html(),
        "<main class=\"p-8\"><h1>Todos</h1><p data-count=\"3\">Drew &lt;script&gt;</p></main>"
    );
}

#[test]
fn iteration_and_conditionals() {
    struct Todo {
        id: u32,
        title: String,
        done: bool,
    }
    let todos = vec![
        Todo { id: 1, title: "a & b".into(), done: true },
        Todo { id: 2, title: "c".into(), done: false },
    ];
    let out = rsx! {
        <ul>
            { todos.iter().map(|t| rsx! {
                <li id={format!("todo-{}", t.id)} data-done={t.done}>
                    {&t.title}
                    { if t.done { rsx! { <em>"(done)"</em> } } else { Rsx::new() } }
                </li>
            }).collect::<Rsx>() }
        </ul>
    };
    assert_eq!(
        out.as_html(),
        "<ul>\
         <li id=\"todo-1\" data-done>a &amp; b<em>(done)</em></li>\
         <li id=\"todo-2\">c</li>\
         </ul>"
    );
}

#[test]
fn void_elements_and_boolean_attrs() {
    let out = rsx! {
        <div>
            <input type="text" disabled />
            <br/>
        </div>
    };
    assert_eq!(
        out.as_html(),
        "<div><input type=\"text\" disabled><br></div>"
    );
}

#[test]
fn doctype_and_fragment() {
    let out = rsx! {
        <>
            <!DOCTYPE html>
            <html lang="en"><body></body></html>
        </>
    };
    assert_eq!(
        out.as_html(),
        "<!doctype html><html lang=\"en\"><body></body></html>"
    );
}

// A "component": plain fn + Props struct, the exact shape generated island
// bindings have.
#[allow(non_snake_case)]
mod client {
    use nextrs::rsx::Rsx;

    pub struct TodoFilterProps {
        pub initial_filter: String,
        pub count: u32,
    }

    #[allow(non_snake_case)]
    pub fn TodoFilter(props: TodoFilterProps) -> Rsx {
        #[derive(serde::Serialize)]
        struct Wire<'a> {
            #[serde(rename = "initialFilter")]
            initial_filter: &'a str,
            count: u32,
        }
        Rsx::client_ref(
            "TodoFilter-test",
            &Wire { initial_filter: &props.initial_filter, count: props.count },
        )
    }

    #[allow(non_snake_case)]
    pub fn Divider() -> Rsx {
        Rsx::from_raw("<hr>")
    }
}

#[test]
fn component_invocation_and_island_placeholder() {
    use client::{Divider, TodoFilter, TodoFilterProps};
    let _ = (TodoFilter, Divider, |p: TodoFilterProps| p); // silence unused-import pedantry

    let out = rsx! {
        <section>
            <client::TodoFilter initial_filter="all" count={2u32} />
            <client::Divider />
        </section>
    };
    assert_eq!(
        out.as_html(),
        "<section>\
         <div data-nx-island=\"TodoFilter-test\" data-nx-props=\"{&quot;initialFilter&quot;:&quot;all&quot;,&quot;count&quot;:2}\"></div>\
         <hr>\
         </section>"
    );
}
