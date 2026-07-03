import { A as require_react, M as __toESM, n as useQueryClient, r as require_jsx_runtime } from "./chunks/QueryClientProvider.js";
import { a as useGetTodos, n as useAddTodo, r as useDeleteTodo, t as getGetTodosQueryKey } from "./chunks/todos.js";

//#region ../app/page.tsx
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function Todos() {
	const queryClient = useQueryClient();
	const [title, setTitle] = (0, import_react.useState)("");
	const invalidate = () => queryClient.invalidateQueries({ queryKey: getGetTodosQueryKey() });
	const { data: todos, refetch, isFetching } = useGetTodos({ status: "open" });
	const addTodo = useAddTodo({ mutation: { onSuccess: () => {
		invalidate();
		setTitle("");
	} } });
	const deleteTodo = useDeleteTodo({ mutation: { onSuccess: invalidate } });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "row",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", { children: "Todos" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				className: "ghost",
				onClick: () => refetch(),
				disabled: isFetching,
				children: isFetching ? "Refreshing…" : "Refresh"
			})]
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
			className: "list",
			children: todos?.data.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
				href: `/todos/${t.id}`,
				children: t.title
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				className: "ghost",
				"aria-label": `Delete ${t.title}`,
				onClick: () => deleteTodo.mutate({ id: t.id }),
				children: "✕"
			})] }, t.id))
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
			className: "add",
			onSubmit: (e) => {
				e.preventDefault();
				if (title.trim()) addTodo.mutate({ data: { title: title.trim() } });
			},
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
				placeholder: "Something to do…",
				value: title,
				onChange: (e) => setTitle(e.target.value)
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				className: "primary",
				type: "submit",
				disabled: addTodo.isPending,
				children: "Add"
			})]
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
			className: "muted note",
			children: [
				"This page is a ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "page.tsx" }),
				" rendered client-side by React. Its data comes from ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "route.rs" }),
				" through generated typed hooks, and the list on first paint was seeded into the React Query cache by",
				" ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "prefetch.rs" }),
				" — no fetch on load."
			]
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
			className: "muted",
			children: [
				"Heads up: todos are stored in process memory with no database, so they reset on cold starts and aren't shared across serverless instances. Storage lives in one file (",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "core/todos.rs" }),
				") — swapping in a real DB wouldn't touch the page or the API."
			]
		})
	] });
}

//#endregion
export { Todos as default };