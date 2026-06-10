+++
title = "Deploy with Docker"
description = "Run a nextrs app on any container host — Fly.io, Railway, ECS, or a VPS"
section = "Deploy"
order = 11
+++

A nextrs app is a plain Axum binary, so serverful deployment is the boring kind: build a release binary, ship it with the `public/` directory, run it behind a reverse proxy. A container works on any host — Fly.io, Railway, Render, ECS, Cloud Run, or a VPS with Docker installed.

## The Dockerfile

A standard two-stage build (the repo ships this at the workspace root):

```dockerfile
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p site

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/site /app/site
COPY site/public /app/public
ENV NEXTRS_PUBLIC_DIR=/app/public
EXPOSE 3000
CMD ["/app/site"]
```

One detail worth knowing: **`NEXTRS_PUBLIC_DIR` points the binary at the shipped assets.** The default asset path is compiled in via `CARGO_MANIFEST_DIR`, which only exists on the build machine. The env var overrides it at runtime — set it anywhere the binary runs away from its source tree.

Add a `.dockerignore` with at least `target/` and `node_modules/` so the build context stays small.

## Build and run

```bash
docker build -t mysite .
docker run --rm -p 3000:3000 mysite
curl -i http://localhost:3000/
```

The server binds `0.0.0.0:3000`. Map whatever host port you like.

## Streaming and the reverse proxy

There's no Vercel adapter in this picture — axum streams chunked `text/html` natively, so loading shells work out of the box. The one thing that can break streaming is a **buffering reverse proxy** in front of the container. If you put nginx in front, disable response buffering for the app:

```nginx
location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_http_version 1.1;
    proxy_buffering off;
}
```

Caddy and Traefik stream by default. After deploying, run the smoke test from [Streaming](/docs/streaming#verifying-streaming-works) — if TTFB equals total time on a loading route, something in the path is buffering.

## Static assets

Serverful, the binary serves `public/` itself via a router fallback (`tower-http` `ServeDir`) — same URLs as the Vercel CDN path, no extra configuration. If you want a CDN in front, point it at the same root URLs; everything under `public/` is safe to cache.

## Logs and environment

The binary reads `.env` if present (via `dotenvy`) and respects `RUST_LOG` for tracing verbosity (`RUST_LOG=info` is the default). Container hosts that capture stdout get structured logs with no extra setup.
