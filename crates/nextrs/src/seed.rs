//! Server-side React Query cache seeding (the `props.rs` convention).
//!
//! A `props.rs` next to a `page.tsx` returns a [`QuerySeed`]; the generated
//! shell handler serializes it into the streamed HTML as a JSON `<script>`
//! tag, and the page's entry wrapper loads it into the React Query cache
//! before mount. Entries are keyed exactly like the generated client keys its
//! queries (`[url]` / `[url, params]`), so mutations and `invalidateQueries`
//! reach seeded data the same as fetched data.
//!
//! Seed entries come from the typed companions `#[nextrs::api]` emits for
//! eligible GET handlers (re-exported by `nextrs::build::emit_seeds` under
//! derived names):
//!
//! ```ignore
//! // app/todos/props.rs
//! include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));
//!
//! pub async fn props(req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
//!     nextrs::QuerySeed::new()
//!         .seed(get_api_todos(
//!             api_todos::TodosFilter { status: Some("open".into()) },
//!             req.extensions(),
//!         ))
//!         .await
//! }
//! ```

use std::future::Future;

/// One warmed cache entry: the canonical query key and the bare response body
/// (the client wraps it in whatever envelope its fetch layer caches).
pub struct SeedEntry {
    pub key: serde_json::Value,
    pub data: serde_json::Value,
}

/// The value a `props.rs` returns: a list of cache entries to stream into the
/// page.
#[derive(Default)]
pub struct QuerySeed {
    entries: Vec<SeedEntry>,
}

impl QuerySeed {
    pub fn new() -> Self {
        Self::default()
    }

    /// Await a seed companion and add its entry. Chains:
    /// `QuerySeed::new().seed(a).await.seed(b).await`.
    pub async fn seed(mut self, entry: impl Future<Output = SeedEntry>) -> Self {
        self.entries.push(entry.await);
        self
    }

    /// The entries as a JSON array (`[{key, data}, ...]`) — the wire shape of
    /// both the `__nx_seeds__` tag and the soft-nav prefetch endpoint.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "key": e.key,
                        "data": e.data,
                    })
                })
                .collect(),
        )
    }

    /// The JSON script tag the shell handler streams before the mount div.
    pub fn to_script_tag(&self) -> String {
        // Escape `<` so payload content can never close the script tag (or
        // open a comment) and break out into markup. `<` is plain JSON,
        // invisible to JSON.parse.
        let safe = self.to_json().to_string().replace('<', "\\u003c");
        format!(
            r#"<script type="application/json" id="__nx_seeds__">{}</script>"#,
            safe
        )
    }
}

/// Build the canonical query key for a GET endpoint — the same shape the
/// generated client uses: `[url]` with no params, `[url, params]` with. The
/// client hashes keys order-insensitively, so params only need to match by
/// content. (Params structs must skip serializing `None` fields: the client
/// drops absent keys, while `null` would be kept and never match.)
pub fn seed_key(url: &str, params: Option<serde_json::Value>) -> serde_json::Value {
    match params {
        Some(p) => serde_json::json!([url, p]),
        None => serde_json::json!([url]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_key_shapes_match_generated_client() {
        assert_eq!(
            seed_key("/api/todos", None).to_string(),
            r#"["/api/todos"]"#
        );
        assert_eq!(
            seed_key("/api/todos", Some(serde_json::json!({"status": "open"}))).to_string(),
            r#"["/api/todos",{"status":"open"}]"#
        );
    }

    #[tokio::test]
    async fn script_tag_escapes_angle_brackets() {
        let seed = QuerySeed::new()
            .seed(async {
                SeedEntry {
                    key: seed_key("/api/x", None),
                    data: serde_json::json!({"html": "</script><script>alert(1)</script>"}),
                }
            })
            .await;
        let tag = seed.to_script_tag();
        assert!(tag.starts_with(r#"<script type="application/json" id="__nx_seeds__">"#));
        // The payload may not contain a literal `<` anywhere.
        let inner = &tag[tag.find('>').unwrap() + 1..tag.rfind("</script>").unwrap()];
        assert!(!inner.contains('<'), "unescaped < in payload: {}", inner);
        // And it still parses back to the same content.
        let parsed: serde_json::Value = serde_json::from_str(inner).unwrap();
        assert_eq!(
            parsed[0]["data"]["html"],
            serde_json::json!("</script><script>alert(1)</script>")
        );
    }

    #[tokio::test]
    async fn entries_accumulate_in_order() {
        let seed = QuerySeed::new()
            .seed(async {
                SeedEntry {
                    key: seed_key("/a", None),
                    data: serde_json::json!(1),
                }
            })
            .await
            .seed(async {
                SeedEntry {
                    key: seed_key("/b", None),
                    data: serde_json::json!(2),
                }
            })
            .await;
        let tag = seed.to_script_tag();
        assert!(tag.find("/a").unwrap() < tag.find("/b").unwrap());
    }
}
