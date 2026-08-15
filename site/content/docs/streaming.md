+++
title = "Loading and Prefetch"
description = "Keep React navigation responsive while route code and server data are prepared"
section = "Guides"
order = 3
+++

nextrs uses React loading components and server-warmed React Query data to keep
navigation responsive. The supported frontend conventions are `page.tsx`,
`layout.tsx`, and `loading.tsx`.

## Loading UI

Add `loading.tsx` beside a page:

```tsx
export default function LoadingTodos() {
  return <p>Loading todos…</p>;
}
```

The app shell can show this component while the route bundle and data become
available. Keep it small and independent of the data it is waiting for.

## Prefetch server data

Add `prefetch.rs` beside the same `page.tsx` to warm its React Query cache. On
a hard load, nextrs puts those entries into the page shell before React mounts.
On link intent and soft navigation, the app shell preloads the target route and
calls the same prefetch path automatically.

The page itself continues using an ordinary generated hook:

```tsx
import { useGetApiTodos } from "@mysite/client/react-query";

export default function TodosPage() {
  const { data, isPending } = useGetApiTodos();

  if (isPending) return <p>Loading todos…</p>;
  return <ul>{data?.data.map((todo) => <li key={todo.id}>{todo.title}</li>)}</ul>;
}
```

Delete `prefetch.rs` and the component still works; its hook simply fetches on
mount. Prefetch is an optimization, not a second frontend data model.

See [React Pages & Server Prefetch](/docs/react-server-props) for the complete
server-seeding flow.
