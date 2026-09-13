//! Runtime support for the `rsx!` macro: the [`Rsx`] HTML value, escaping,
//! and the client-island placeholder protocol.
//!
//! A `page.rs` server component returns [`Rsx`] (see `docs/rsx-server-components.md`).
//! The `rsx!` macro builds one by appending escaped text, attributes, and
//! child fragments to an internal string. React `.tsx` islands render through
//! [`Rsx::client_ref`], which emits a `data-nx-island` placeholder that the
//! client mount runtime resolves against the build manifest.

use axum::response::{Html, IntoResponse, Response};

/// A rendered HTML fragment. Produced by the `rsx!` macro; returned by
/// RSX server components (`pub async fn page(...) -> Rsx`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rsx {
    html: String,
}

impl Rsx {
    /// An empty fragment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wrap an already-safe HTML string. The caller vouches for escaping —
    /// this is the `rsx!` macro's constructor and the escape hatch for
    /// embedding trusted markup.
    pub fn from_raw(html: impl Into<String>) -> Self {
        Self { html: html.into() }
    }

    /// The rendered HTML.
    pub fn into_html(self) -> String {
        self.html
    }

    /// The rendered HTML, borrowed.
    pub fn as_html(&self) -> &str {
        &self.html
    }

    /// Render a client-island placeholder: a `div` carrying the island id and
    /// the JSON-serialized props. The client runtime scans for
    /// `data-nx-island`, resolves the chunk via the manifest, and
    /// `createRoot`-mounts the React component with these props.
    ///
    /// Generated island bindings (`crate::client::*`) call this; applications
    /// normally never do directly.
    pub fn client_ref<T: serde::Serialize>(island_id: &str, props: &T) -> Rsx {
        let json = serde_json::to_string(props)
            .unwrap_or_else(|e| panic!("island `{island_id}`: props failed to serialize: {e}"));
        let mut out = String::with_capacity(json.len() + island_id.len() + 64);
        out.push_str("<div data-nx-island=\"");
        escape_attr(&mut out, island_id);
        out.push_str("\" data-nx-props=\"");
        escape_attr(&mut out, &json);
        out.push_str("\"></div>");
        Rsx { html: out }
    }
}

impl IntoResponse for Rsx {
    fn into_response(self) -> Response {
        Html(self.html).into_response()
    }
}

/// Escape text content (`<`, `>`, `&`).
pub fn escape_text(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

/// Escape a double-quoted attribute value (`&`, `"`, `<`).
pub fn escape_attr(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(ch),
        }
    }
}

/// Anything an `rsx!` expression hole (`{expr}`) can render. Text-like values
/// are escaped; [`Rsx`] values are already-safe markup; iterators render each
/// item in order (so `{todos.iter().map(|t| rsx! { ... })}` works directly).
pub trait Render {
    fn render_to(self, out: &mut String);
}

impl Render for Rsx {
    fn render_to(self, out: &mut String) {
        out.push_str(&self.html);
    }
}

impl Render for &Rsx {
    fn render_to(self, out: &mut String) {
        out.push_str(&self.html);
    }
}

impl Render for &str {
    fn render_to(self, out: &mut String) {
        escape_text(out, self);
    }
}

impl Render for String {
    fn render_to(self, out: &mut String) {
        escape_text(out, &self);
    }
}

impl Render for &String {
    fn render_to(self, out: &mut String) {
        escape_text(out, self);
    }
}

impl Render for char {
    fn render_to(self, out: &mut String) {
        escape_text(out, self.encode_utf8(&mut [0u8; 4]));
    }
}

impl Render for bool {
    fn render_to(self, out: &mut String) {
        out.push_str(if self { "true" } else { "false" });
    }
}

macro_rules! render_via_display {
    ($($t:ty),*) => {$(
        impl Render for $t {
            fn render_to(self, out: &mut String) {
                // Numeric Display output never contains HTML-special chars.
                use std::fmt::Write;
                let _ = write!(out, "{self}");
            }
        }
        impl Render for &$t {
            fn render_to(self, out: &mut String) {
                use std::fmt::Write;
                let _ = write!(out, "{self}");
            }
        }
    )*};
}
render_via_display!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64);

impl<T: Render> Render for Option<T> {
    fn render_to(self, out: &mut String) {
        if let Some(v) = self {
            v.render_to(out);
        }
    }
}

impl<T> Render for Vec<T>
where
    T: Render,
{
    fn render_to(self, out: &mut String) {
        for item in self {
            item.render_to(out);
        }
    }
}

/// `iter().map(...)` chains collect straight into a fragment:
/// `{ todos.iter().map(|t| rsx! { ... }).collect::<Rsx>() }`.
impl<T: Render> FromIterator<T> for Rsx {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut out = String::new();
        for item in iter {
            item.render_to(&mut out);
        }
        Rsx { html: out }
    }
}

impl Rsx {
    /// Render every item of an iterator in order — the loop form for
    /// expression holes: `{ Rsx::each(todos.iter().map(|t| rsx! { ... })) }`.
    /// (A direct `impl Render for I: Iterator` is ruled out by coherence:
    /// it conflicts with the concrete impls for foreign types like `&str`.)
    pub fn each<I>(items: I) -> Rsx
    where
        I: IntoIterator,
        I::Item: Render,
    {
        items.into_iter().collect()
    }
}

/// Anything usable as an `rsx!` attribute value. Strings escape; numbers
/// print; `Option` omits the attribute entirely when `None`; `bool` renders
/// as a boolean attribute (present when true, absent when false).
pub trait AttrValue {
    /// Write ` name="value"` (with leading space) or nothing.
    fn render_attr_to(self, name: &str, out: &mut String);
}

fn push_attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_attr(out, value);
    out.push('"');
}

impl AttrValue for &str {
    fn render_attr_to(self, name: &str, out: &mut String) {
        push_attr(out, name, self);
    }
}

impl AttrValue for String {
    fn render_attr_to(self, name: &str, out: &mut String) {
        push_attr(out, name, &self);
    }
}

impl AttrValue for &String {
    fn render_attr_to(self, name: &str, out: &mut String) {
        push_attr(out, name, self);
    }
}

impl AttrValue for bool {
    fn render_attr_to(self, name: &str, out: &mut String) {
        if self {
            out.push(' ');
            out.push_str(name);
        }
    }
}

macro_rules! attr_via_display {
    ($($t:ty),*) => {$(
        impl AttrValue for $t {
            fn render_attr_to(self, name: &str, out: &mut String) {
                push_attr(out, name, &self.to_string());
            }
        }
    )*};
}
attr_via_display!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64);

impl<T: AttrValue> AttrValue for Option<T> {
    fn render_attr_to(self, name: &str, out: &mut String) {
        if let Some(v) = self {
            v.render_attr_to(name, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text_and_attrs() {
        let mut t = String::new();
        escape_text(&mut t, r#"<b>&"quotes" stay</b>"#);
        assert_eq!(t, "&lt;b&gt;&amp;\"quotes\" stay&lt;/b&gt;");

        let mut a = String::new();
        escape_attr(&mut a, r#"say "hi" & <bye>"#);
        assert_eq!(a, "say &quot;hi&quot; &amp; &lt;bye>");
    }

    #[test]
    fn render_impls() {
        let mut out = String::new();
        "a<b".render_to(&mut out);
        42i32.render_to(&mut out);
        Some("!").render_to(&mut out);
        Option::<&str>::None.render_to(&mut out);
        vec![Rsx::from_raw("<hr>"), Rsx::from_raw("<br>")].render_to(&mut out);
        Rsx::each(["x", "y"].iter().map(|s| Rsx::from_raw(*s))).render_to(&mut out);
        ["1<2", "3"].iter().map(|s| *s).collect::<Rsx>().render_to(&mut out);
        assert_eq!(out, "a&lt;b42!<hr><br>xy1&lt;23");
    }

    #[test]
    fn attr_impls() {
        let mut out = String::new();
        "v<1".render_attr_to("title", &mut out);
        true.render_attr_to("checked", &mut out);
        false.render_attr_to("disabled", &mut out);
        Option::<&str>::None.render_attr_to("id", &mut out);
        Some(7u8).render_attr_to("tabindex", &mut out);
        assert_eq!(out, r#" title="v&lt;1" checked tabindex="7""#);
    }

    #[test]
    fn client_ref_placeholder() {
        #[derive(serde::Serialize)]
        struct P {
            n: u32,
            s: &'static str,
        }
        let rsx = Rsx::client_ref("Counter-abc123", &P { n: 1, s: "a\"b" });
        assert_eq!(
            rsx.as_html(),
            r#"<div data-nx-island="Counter-abc123" data-nx-props="{&quot;n&quot;:1,&quot;s&quot;:&quot;a\&quot;b&quot;}"></div>"#
        );
    }
}
