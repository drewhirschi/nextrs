import { defineConfig } from "orval";

// Generates a typed TanStack (React) Query client from the OpenAPI document
// `dump-openapi` writes (the same spec the app serves at /openapi.json).
// Run `npm run gen` to refresh both.
export default defineConfig({
  api: {
    input: "./openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated",
      schemas: "./src/generated/model",
      client: "react-query",
      httpClient: "fetch",
      // Same-origin: one binary serves both the page and the API.
      baseUrl: "/",
      clean: true,
      prettier: false,
    },
  },
});
