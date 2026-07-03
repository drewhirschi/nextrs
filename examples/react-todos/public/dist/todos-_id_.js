import { r as require_jsx_runtime } from "./chunks/QueryClientProvider.js";
import { i as useGetApiTodosById } from "./chunks/todos.js";

//#region ../app/todos/[id]/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function TodoDetail({ params }) {
	const { data, isFetching } = useGetApiTodosById(Number(params.id));
	const todo = data?.data;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("h1", { children: ["Todo #", params.id] }),
		todo ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", { children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", { children: todo.title }),
			" ",
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "muted",
				children: todo.done ? "done" : "open"
			})
		] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "muted",
			children: isFetching ? "Loading…" : "No such todo."
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
			className: "muted note",
			children: [
				"The ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "id" }),
				" arrives as a ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "params" }),
				" prop — live from the router on soft navigation, streamed by the server on a hard load. The todo itself was seeded by ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "prefetch.rs" }),
				" via the typed Path-param companion for ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: "GET /api/todos/{id}" }),
				"."
			]
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
			href: "/",
			children: "← back to the list"
		}) })
	] });
}

//#endregion
export { TodoDetail as default };