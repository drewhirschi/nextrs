//! Matched route params for React pages (the `[seg]` directory convention).
//!
//! The server matched the route, so it hands the params over instead of making
//! the client re-parse `window.location`. For a dynamic tsx route the
//! generated shell handler extracts the matched params and streams them as a
//! JSON script tag (`__nx_params__`) ahead of the mount div — the same
//! mechanism `props.rs` seeds use. The bundle entry wrapper reads the tag and
//! passes them to the page Next.js-style:
//!
//! ```ignore
//! // app/source/[id]/page.tsx
//! export default function Page({ params }: { params: { id: string } }) { ... }
//! ```
//!
//! Routes with dynamic segments also get params passed to their `props.rs`:
//! `pub async fn props(req, params: nextrs::Params) -> QuerySeed`. Paramless
//! routes keep the one-argument `props(req)` form.

use axum::extract::{FromRequestParts, RawPathParams, Request};
use std::collections::BTreeMap;

/// The matched `[seg]` params of a routed request. Values are the raw URL
/// segments (strings); typed parsing is the caller's concern
/// (`params.get("id").and_then(|v| v.parse().ok())`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params(BTreeMap<String, String>);

impl Params {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The JSON script tag the shell handler streams before the mount div.
    /// `<` is escaped exactly like the seeds tag, so a param value can never
    /// close the script tag and break out into markup.
    pub fn to_script_tag(&self) -> String {
        let json = serde_json::to_value(&self.0).expect("string map serializes");
        let safe = json.to_string().replace('<', "\\u003c");
        format!(
            r#"<script type="application/json" id="__nx_params__">{}</script>"#,
            safe
        )
    }
}

impl FromIterator<(String, String)> for Params {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Params(iter.into_iter().collect())
    }
}

/// Pull the matched path params off a routed request, returning the request
/// for further use. Axum records the match in the request's extensions, so
/// this only works on requests that came through the router — elsewhere it
/// yields empty [`Params`].
pub async fn extract_params(req: Request) -> (Params, Request) {
    let (mut parts, body) = req.into_parts();
    let params = match RawPathParams::from_request_parts(&mut parts, &()).await {
        Ok(raw) => raw
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        Err(_) => Params::default(),
    };
    (params, Request::from_parts(parts, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn script_tag_carries_the_map() {
        let tag = params(&[("id", "17")]).to_script_tag();
        assert_eq!(
            tag,
            r#"<script type="application/json" id="__nx_params__">{"id":"17"}</script>"#
        );
    }

    #[test]
    fn script_tag_escapes_angle_brackets() {
        let tag = params(&[("id", "</script><script>alert(1)</script>")]).to_script_tag();
        let inner = &tag[tag.find('>').unwrap() + 1..tag.rfind("</script>").unwrap()];
        assert!(!inner.contains('<'), "unescaped < in payload: {}", inner);
        let parsed: serde_json::Value = serde_json::from_str(inner).unwrap();
        assert_eq!(
            parsed["id"],
            serde_json::json!("</script><script>alert(1)</script>")
        );
    }

    #[test]
    fn get_and_iter() {
        let p = params(&[("a", "1"), ("b", "2")]);
        assert_eq!(p.get("a"), Some("1"));
        assert_eq!(p.get("missing"), None);
        assert_eq!(p.iter().count(), 2);
        assert!(!p.is_empty());
        assert!(Params::default().is_empty());
    }

    #[tokio::test]
    async fn extract_params_from_routed_request() {
        use axum::routing::get;

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Params::default()));
        let captured_clone = std::sync::Arc::clone(&captured);
        let app = axum::Router::new().route(
            "/source/{id}",
            get(move |req: Request| {
                let captured = std::sync::Arc::clone(&captured_clone);
                async move {
                    let (params, _req) = extract_params(req).await;
                    *captured.lock().unwrap() = params;
                    "ok"
                }
            }),
        );

        use tower::ServiceExt;
        let _ = app
            .oneshot(
                http::Request::builder()
                    .uri("/source/17")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(captured.lock().unwrap().get("id"), Some("17"));
    }

    #[tokio::test]
    async fn extract_params_off_router_is_empty() {
        let req = http::Request::builder()
            .uri("/whatever")
            .body(axum::body::Body::empty())
            .unwrap();
        let (params, _req) = extract_params(req).await;
        assert!(params.is_empty());
    }
}
