import { defineConfig } from "orval";

// Both surfaces come from the same hidden OpenAPI document. The root package
// entry stays framework-agnostic; React and TanStack Query live behind the
// explicit `/react-query` entry point.
export default defineConfig({
  fetch: {
    input: "../openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated/fetch",
      schemas: "./src/generated/fetch/model",
      client: "fetch",
      httpClient: "fetch",
      baseUrl: "/",
      clean: true,
      prettier: false,
    },
  },
  reactQuery: {
    input: "../openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated/react-query",
      schemas: "./src/generated/react-query/model",
      client: "react-query",
      httpClient: "fetch",
      baseUrl: "/",
      clean: true,
      prettier: false,
    },
  },
});
