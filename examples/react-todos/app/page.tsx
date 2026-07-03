import { useQueryClient } from "@tanstack/react-query";
import {
  useGetTodosFromUrl,
  useAddTodo,
  useUpdateTodo,
  getGetTodosQueryKey,
  getGetApiTodosByIdQueryKey,
} from "@react-todos/client";
import { useState } from "react";

export default function Todos() {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState("");

  // Any mutation refreshes every /api/todos variant — including the
  // server-seeded entry — because they all share the canonical query key.
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: getGetTodosQueryKey() });

  // URL-bound: the filter lives in the page URL (?status=open), not in
  // useState — so a shared link shows the same view, back/forward walks
  // previous filters (from cache, instantly), and a hard load of any
  // filtered URL is seeded by prefetch.rs from the same query string.
  // Warmed from the stream on first render: no spinner, no mount fetch.
  const {
    data: todos,
    refetch,
    isFetching,
    params,
    setParams,
  } = useGetTodosFromUrl();

  const addTodo = useAddTodo({
    mutation: {
      onSuccess: () => {
        invalidate();
        setTitle("");
      },
    },
  });

  // The toggled todo also lives in the detail page's cache entry (a different
  // URL key, not a prefix of the list's) — invalidate it too, or its page
  // shows a stale badge after soft-navigating there.
  const updateTodo = useUpdateTodo({
    mutation: {
      onSuccess: (_data, variables) => {
        invalidate();
        queryClient.invalidateQueries({
          queryKey: getGetApiTodosByIdQueryKey(variables.id),
        });
      },
    },
  });

  return (
    <section>
      <div className="row">
        <h1>Todos</h1>
        {/* setParams soft-navigates: the URL becomes ?status=open, this hook
            re-keys off it, and the previous filter stays warm in the cache. */}
        <select
          aria-label="Filter todos"
          value={params.status ?? ""}
          onChange={(e) => setParams({ status: e.target.value || undefined })}
        >
          <option value="">All</option>
          <option value="open">Open</option>
        </select>
        <button className="ghost" onClick={() => refetch()} disabled={isFetching}>
          {isFetching ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      <ul className="list">
        {todos?.data.map((t) => (
          <li key={t.id} className={t.done ? "done" : ""}>
            <button
              className={`check${t.done ? " checked" : ""}`}
              aria-label={t.done ? `Reopen ${t.title}` : `Complete ${t.title}`}
              onClick={() => updateTodo.mutate({ id: t.id, data: { done: !t.done } })}
            >
              {t.done ? "✓" : ""}
            </button>
            {/* Plain anchor — the app shell intercepts it and soft-navigates
                to the [id] route (no document load; layout stays mounted). */}
            <a className="title" href={`/todos/${t.id}`}>
              {t.title}
            </a>
            <span className={`badge ${t.done ? "badge-done" : "badge-open"}`}>
              {t.done ? "Done" : "Open"}
            </span>
          </li>
        ))}
      </ul>

      <form
        className="add"
        onSubmit={(e) => {
          e.preventDefault();
          if (title.trim()) addTodo.mutate({ data: { title: title.trim() } });
        }}
      >
        <input
          placeholder="Something to do…"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <button className="primary" type="submit" disabled={addTodo.isPending}>
          Add
        </button>
      </form>

      <p className="muted note">
        This page is a <code>page.tsx</code> rendered client-side by React. Its
        data comes from <code>route.rs</code> through generated typed hooks, and
        the list on first paint was seeded into the React Query cache by{" "}
        <code>prefetch.rs</code> — no fetch on load.
      </p>
      <p className="muted">
        Heads up: todos are stored in process memory with no database, so they
        reset on cold starts and aren&apos;t shared across serverless instances.
        Storage lives in one file (<code>core/todos.rs</code>) — swapping in a
        real DB wouldn&apos;t touch the page or the API.
      </p>
    </section>
  );
}
