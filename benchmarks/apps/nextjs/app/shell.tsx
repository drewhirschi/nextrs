"use client";

import dynamic from "next/dynamic";
import type { Todo } from "@/lib/store";

// ssr: false → Next ships a placeholder + the serialized `initial` data and
// renders the todos client-side. The server does NOT render the list to HTML,
// mirroring nextrs's `<div id="__nx_root__">` + bundle shell. (ssr:false is
// only allowed inside a Client Component, hence this thin wrapper.)
const TodosClient = dynamic(
  () => import("./todos-client").then((m) => m.TodosClient),
  { ssr: false },
);

export function Shell({ initial }: { initial: Todo[] }) {
  return <TodosClient initial={initial} />;
}
