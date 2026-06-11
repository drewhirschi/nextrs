"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { fetchTodos, todosKey } from "@/lib/queries";
import type { Todo } from "@/lib/store";

export function TodosClient({ initial }: { initial: Todo[] }) {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState("");

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["todos"] });

  // Seeded from the server-passed `initial` (read from the store in page.tsx) —
  // defined on first render, no mount fetch. The client-side analog of
  // react-todos's props.rs cache seeding.
  const {
    data: todos,
    refetch,
    isFetching,
  } = useQuery({
    queryKey: todosKey("open"),
    queryFn: () => fetchTodos("open"),
    initialData: initial,
  });

  const addTodo = useMutation({
    mutationFn: async (t: string) =>
      (
        await fetch("/api/todos", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ title: t }),
        })
      ).json(),
    onSuccess: () => {
      invalidate();
      setTitle("");
    },
  });

  const deleteTodo = useMutation({
    mutationFn: async (id: number) =>
      fetch(`/api/todos/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
  });

  return (
    <section>
      <div className="row">
        <h1>Todos</h1>
        <button
          className="ghost"
          onClick={() => refetch()}
          disabled={isFetching}
        >
          {isFetching ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      <ul className="list">
        {todos?.map((t) => (
          <li key={t.id}>
            <span>{t.title}</span>
            <button
              className="ghost"
              aria-label={`Delete ${t.title}`}
              onClick={() => deleteTodo.mutate(t.id)}
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
          if (title.trim()) addTodo.mutate(title.trim());
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
        This is the Next.js baseline: an App Router RSC page seeds the React
        Query cache server-side (no fetch on load), with Route Handlers for the
        API — architecturally identical to the nextrs version.
      </p>
    </section>
  );
}
