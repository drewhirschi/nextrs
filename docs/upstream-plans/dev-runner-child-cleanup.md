# Dev Runner Child Cleanup

## Problem

`cargo dev` runs the scaffolded `nextrs-dev` binary, which then runs the app
with `cargo run --bin <app>`. When interrupted, the child app server can remain
alive and keep port 3000 bound. The next dev run then fails with
`Address already in use`.

## Proposed Direction

The dev runner should install shutdown handling and reliably terminate the child
process group:

- On Ctrl-C, terminate the child process group.
- Also terminate the direct child PID as a fallback.
- Suppress harmless "process not found" noise.
- Wait briefly for graceful shutdown, then force kill.
- Reuse the same cleanup path for restarts and final shutdown.

## Implementation Notes

- On Unix, keep using process groups for the nested Cargo/app process tree.
- On non-Unix, use the best available child kill behavior and document any
  limitations.
- Avoid introducing heavyweight runtime dependencies in the generated dev
  runner unless the ergonomics justify it.

## Validation

- Add a small integration-style test or script that starts `nextrs-dev`, sends
  SIGINT, and verifies the app port is released.
- Test restart cleanup separately from Ctrl-C cleanup.
- Confirm no noisy `kill: cannot find process` output appears in normal races.
