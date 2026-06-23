# Cargo Dev Double Compile

## Problem

The generated `cargo dev` flow first builds/runs `nextrs-dev`, then that binary
runs `cargo run --bin <app>`. On a cold build this looks like dependencies
compile twice, especially for expensive crates such as TLS, database, or native
library dependencies.

## Proposed Direction

Short term:

- Document the two-phase build in the scaffold docs and local dev workflow.
- Make CLI output explicit: first phase is the watcher, second phase is the app.

Medium term:

- Explore a dev runner strategy that avoids nested Cargo when practical.
- Consider shipping `nextrs-dev` as a lightweight installed tool instead of a
  binary inside every app package.
- Consider an `xtask` or external watcher path for framework examples.

## Implementation Notes

- Nested Cargo is simple and portable, but it is confusing and expensive-looking
  for new users.
- An external installed tool improves app build graphs, but adds installation
  and versioning questions.
- Any alternative should preserve the current zero-setup scaffold ergonomics.

## Validation

- Documented behavior appears in generated README or next-step output.
- Measure cold and warm dev startup before and after any runner redesign.
- Confirm workspace and standalone app workflows both still work.
