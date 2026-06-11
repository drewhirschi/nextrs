// In-memory todo store — the Next.js mirror of react-todos's core/todos.rs.
// Same shape, same seed data, no database, so the benchmark isolates
// framework/runtime overhead rather than I/O.

export interface Todo {
  id: number;
  title: string;
  done: boolean;
}

// Module-level state persists for the life of the (warm) server instance, the
// same as the Rust app's OnceLock<Mutex<Vec<Todo>>>.
const todos: Todo[] = [
  { id: 1, title: "Write a page.tsx", done: true },
  { id: 2, title: "Seed the React Query cache from the server", done: false },
  { id: 3, title: "Ship it", done: false },
];

export function list(openOnly: boolean): Todo[] {
  return openOnly ? todos.filter((t) => !t.done) : todos.slice();
}

export function add(title: string): Todo {
  const id = todos.reduce((m, t) => Math.max(m, t.id), 0) + 1;
  const todo: Todo = { id, title, done: false };
  todos.push(todo);
  return todo;
}

export function remove(id: number): boolean {
  const i = todos.findIndex((t) => t.id === id);
  if (i === -1) return false;
  todos.splice(i, 1);
  return true;
}
