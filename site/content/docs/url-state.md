+++
title = "List State Belongs in the URL"
description = "The default pattern for filters and pagination: URL search params, passed to the server, via the generated FromUrl hooks"
section = "Guides"
order = 6
+++

**This is the house style for nextrs apps — humans and coding agents alike
should treat it as the default, not an option.** Any list that can be
filtered, sorted, searched, or paginated keeps that state in URL search
params, and those params are passed through to the server. Do not reach for
`useState` for list state.

Why it's the default:

- **Links are the whole point.** `?status=open&page=3` can be shared,
  bookmarked, opened in a new tab, and restored on refresh; back/forward
  walks previous views out of warm cache. `useState` loses all of that.
- **The server sees the real query.** nextrs seeds data server-side per
  request; when the filter lives in the URL, the first render of any
  filtered URL is the *filtered* result — no flash of unfiltered content,
  no client fetch on load.
- **One source of truth.** The URL, the query key, and the API call all
  derive from the same params, so they cannot drift apart.

## The pattern

The wiring is generated end to end. Declare the params as a `Query<T>`
extractor on the API route — that makes them part of the OpenAPI contract and
the typed client:

```rust
#[derive(Serialize, Deserialize, IntoParams)]
pub struct ItemsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

#[nextrs::api]
pub async fn get(Query(query): Query<ItemsQuery>) -> Json<ItemsPage> { ... }
```

(`skip_serializing_if` matters: seeded query keys drop absent fields, so
serializing `None` as `null` would make the server-built key never match the
client hook's.)

On the client, the codegen emits a **`use...FromUrl` variant of every GET
hook with params**. Params are read from the page URL; `setParams`
soft-navigates, which re-keys the query and keeps the previous view warm in
cache:

```tsx
const { data, params, setParams } = useGetApiItemsFromUrl();

<input
  value={params.q ?? ""}
  onChange={(e) => setParams({ q: e.target.value || undefined, page: undefined })}
/>
<button onClick={() => setParams({ page: (params.page ?? 1) + 1 })}>Next</button>
```

`undefined` in a patch deletes the key from the URL; a filter change should
reset `page`. Pass `{ history: "replace" }` for typeahead-style updates that
shouldn't spam history.

Close the loop with a `prefetch.rs` beside the page that parses the **same**
query string and seeds through the handler's generated companion:

```rust
include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));

pub async fn prefetch(req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    let query = nextrs::search_params::<api_items::ItemsQuery, _>(&req)
        .unwrap_or(api_items::ItemsQuery { q: None, page: None });
    nextrs::QuerySeed::new()
        .seed(get_api_items(query, req.extensions()))
        .await
}
```

Now every `?q=&page=` URL renders server-seeded with the right slice.

Scaffolded apps ship a worked example at `app/items/` (a filtered, paginated
list); `examples/react-todos` does the same for its `?status=` filter.

## When to reach for nuqs

For URL state that is **not** tied to a generated endpoint hook — an active
tab, a selected view mode, panel state you want shareable —
[nuqs](https://nuqs.dev) is the recommended package: `useState` ergonomics,
type-safe parsers, reads and writes the URL. Don't layer it on top of a
`FromUrl` hook for the same params, though — two writers to one URL.

## Rules of thumb

- Filtering, sorting, searching, pagination, active tab on a list page → URL
  search params, always.
- Ephemeral UI (open modal, hover, half-typed input before submit) → local
  state is fine.
- Params are part of the API contract: declare them in the `Query<T>` struct
  so they flow into OpenAPI and the typed client — never hand-build query
  strings.
- Debounce text search locally if you must, but commit the value to the URL.
