import { defineConfig } from "orval";

// Generates a typed TanStack (React) Query client from the OpenAPI document
// `dump-openapi` writes (the same spec the app serves at /openapi.json).
// Run `npm run gen` to refresh both.
export default defineConfig({
  basic: {
    input: "./openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated/basic",
      schemas: "./src/generated/basic/model",
      client: "fetch",
      httpClient: "fetch",
      // Same-origin: one binary serves both the page and the API.
      baseUrl: "/",
      clean: true,
      prettier: false,
    },
  },
  reactQuery: {
    input: "./openapi.json",
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
