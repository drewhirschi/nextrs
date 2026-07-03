import { useQueryClient } from "@tanstack/react-query";
import {
  useGetTodos,
  useAddTodo,
  useDeleteTodo,
  getGetTodosQueryKey,
} from "@react-todos/client";
import { useState } from "react";

export default function Todos() {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState("");

  // Any mutation refreshes every /api/todos variant — including the
  // server-seeded entry — because they all share the canonical query key.
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: getGetTodosQueryKey() });

  // Warmed from the stream by prefetch.rs: defined on first render, no
  // spinner, no mount fetch. Delete prefetch.rs and this just fetches on
  // mount instead — the component can't tell.
  const { data: todos, refetch, isFetching } = useGetTodos({ status: "open" });

  const addTodo = useAddTodo({
    mutation: {
      onSuccess: () => {
        invalidate();
        setTitle("");
      },
    },
  });

  const deleteTodo = useDeleteTodo({ mutation: { onSuccess: invalidate } });

  return (
    <section>
      <div className="row">
        <h1>Todos</h1>
        <button className="ghost" onClick={() => refetch()} disabled={isFetching}>
          {isFetching ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      <ul className="list">
        {todos?.data.map((t) => (
          <li key={t.id}>
            {/* Plain anchor — the app shell intercepts it and soft-navigates
                to the [id] route (no document load; layout stays mounted). */}
            <a href={`/todos/${t.id}`}>{t.title}</a>
            <button
              className="ghost"
              aria-label={`Delete ${t.title}`}
              onClick={() => deleteTodo.mutate({ id: t.id })}
            >
              ✕
            </button>
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
