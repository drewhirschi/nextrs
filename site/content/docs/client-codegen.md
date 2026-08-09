+++
title = "Client Code Generation, Step by Step"
description = "Turn a small Rust route into typed fetch functions, React Query hooks, and a plain JavaScript client"
section = "Guides"
order = 3
+++

nextrs can turn a Rust API route into client code. The generated client knows
the URL, request body, query parameters, success response, and documented error
responses. Start with one endpoint and add the more advanced pieces only when
you need them.

## 1. Write a small Rust endpoint

Create `app/api/greeting/route.rs`:

```rust
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Greeting {
    pub message: String,
}

#[nextrs::api(
    get,
    operation_id = "getGreeting",
    responses((status = 200, description = "A greeting", body = Greeting)),
)]
pub async fn get() -> Json<Greeting> {
    Json(Greeting {
        message: "Hello from Rust".into(),
    })
}
```

The file location supplies the URL: `app/api/greeting/route.rs` becomes
`GET /api/greeting`. The annotation supplies the name and response contract
for code generation.

This remains an ordinary Axum handler. `#[nextrs::api]` adds it to the OpenAPI
document; it does not introduce a separate RPC runtime.

## 2. Generate the client

Install the nextrs Cargo command once, then generate from the app root:

```bash
cargo install cargo-nextrs
cargo nextrs client generate
```

The Cargo command installs the client generator dependencies when they are
missing. Orval and TypeScript still do the OpenAPI-to-JavaScript work, but they
are implementation details behind one project-level command.

The command follows this path:

```text
route.rs
   ↓ Rust build and #[nextrs::api]
OpenAPI document
   ↓ Orval
generated TypeScript fetch function + React Query hook
```

Conceptually, the generated surface looks like this:

```ts
declare function getGreeting(): Promise<{
  status: 200;
  data: { message: string };
  headers: Headers;
}>;

declare function useGetGreeting(): UseQueryResult<
  Awaited<ReturnType<typeof getGreeting>>
>;
```

Those declarations are illustrative; nextrs and Orval generate the real
implementation and types. Do not edit `client/src/generated/` by hand.

## 3. Make a direct call

The smallest client usage is a normal async function call:

```ts
import { getGreeting } from "@mysite/client";

const response = await getGreeting();
console.log(response.data.message);
```

This does not use React or React Query. It works well in event handlers,
scripts, tests, and other UI frameworks. Compared with raw `fetch`, there is no
handwritten URL, response interface, JSON parsing, or type assertion.

## 4. Use the hook when a component needs it

The same endpoint also produces a React Query hook:

```tsx
import { useGetGreeting } from "@mysite/client";

export default function GreetingPage() {
  const greeting = useGetGreeting();

  if (greeting.isPending) return <p>Loading…</p>;
  return <p>{greeting.data?.data.message}</p>;
}
```

The direct function and hook are two clients for the same Rust contract. Use
the hook when caching, loading state, refetching, or invalidation is useful;
otherwise the direct function is enough.

## 5. Watch a Rust change reach the client

Rename the response field:

```rust
pub struct Greeting {
    pub text: String,
}
```

Regenerate:

```bash
cargo nextrs client generate
```

The old TypeScript expression now fails at the correct line:

```ts
response.data.message;
//            ^^^^^^^ Property 'message' does not exist
```

That failure is the value of code generation: Rust owns the contract, and
client call sites cannot silently keep using its previous shape.

## 6. Add a typed request body

Request bodies flow in the other direction. Add a POST handler:

```rust
use serde::Deserialize;

#[derive(Deserialize, ToSchema)]
pub struct CreateGreeting {
    pub name: String,
}

#[nextrs::api(
    post,
    operation_id = "createGreeting",
    responses((status = 200, description = "A greeting", body = Greeting)),
)]
pub async fn post(Json(body): Json<CreateGreeting>) -> Json<Greeting> {
    Json(Greeting {
        text: format!("Hello, {}", body.name),
    })
}
```

After regeneration, the body is checked at the call site:

```ts
import { createGreeting } from "@mysite/client";

await createGreeting({ name: "Ada" }); // valid
await createGreeting({ name: 42 });    // type error
```

## 7. Publish plain JavaScript to another project

For a Chrome extension or separate JavaScript project, configure a dedicated
output directory in `client/nextrs.client.json`:

```json
{
  "output": "../../extension/generated/nextrs-client",
  "baseUrl": "https://challenge.example.com"
}
```

Then run the same command from the app root:

```bash
cargo nextrs client generate
```

When `client/nextrs.client.json` exists, the command regenerates the internal
client and publishes the external client in the same pass.

The destination receives:

```text
generated/nextrs-client/
├── client.js       browser-native fetch client; no React dependency
├── client.d.ts     editor and type-checker declarations
├── package.json    marks the directory as an ES module package
└── .nextrs-generated-client
```

A plain JavaScript file can import it directly:

```js
// @ts-check
import { getGreeting } from "./generated/nextrs-client/client.js";

const response = await getGreeting();
console.log(response.data.message);
```

JavaScript executes `client.js`; VS Code and TypeScript read `client.d.ts`.
The consuming project does not need to compile TypeScript.

The publisher only replaces a non-empty destination bearing the nextrs marker,
so it will not clean an unrelated directory accidentally. It also rebuilds and
dumps the Rust contract before publishing, ensuring that the external client
does not come from a stale OpenAPI file.

## The rule to remember

After changing an annotated Rust API contract, regenerate its clients:

```bash
cargo nextrs client generate
```

The external command includes the full Rust-contract refresh, so it can be used
by itself when only an external consumer needs the client.

For path and query parameters, documented errors, operation naming, and
troubleshooting, continue to [Typesafe Client Generation](/docs/typesafe-client).
