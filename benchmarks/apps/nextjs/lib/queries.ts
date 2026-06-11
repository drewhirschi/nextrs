import type { Todo } from "./store";

// Shared query key so the server prefetch (page.tsx) and the client hook
// (todos-client.tsx) hit the same cache entry — the equivalent of react-todos
// seeding under the hook's canonical key.
export const todosKey = (status: string) => ["todos", { status }] as const;

// Client-side fetcher used by useQuery after hydration / on refetch.
export async function fetchTodos(status: string): Promise<Todo[]> {
  const res = await fetch(`/api/todos?status=${status}`, { cache: "no-store" });
  return res.json();
}
