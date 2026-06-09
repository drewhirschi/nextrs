import { defineConfig } from "orval";

// Generates a typesafe TanStack (React) Query client from the OpenAPI document
// that `dump-openapi` writes (which is the same spec the app serves at
// /openapi.json). Run `npm run gen` to refresh both the spec and the client.
export default defineConfig({
  api: {
    input: "./openapi.json",
    output: {
      // One file per OpenAPI tag (we tag handlers with e.g. `ping`).
      mode: "tags-split",
      target: "./src/generated",
      schemas: "./src/generated/model",
      client: "react-query",
      // Use the platform `fetch` so the client needs no HTTP-library dep.
      httpClient: "fetch",
      // Same-origin: the app serves both the pages and the API.
      baseUrl: "/",
      clean: true,
      prettier: false,
    },
  },
});
