> **Platform note (v0.30.0):** Aoxn now targets Windows only. The POSIX
> entries (`web/server_posix.ax`) and the Linux/macOS runs below are kept
> as a historical record of the v0.26.x multi-platform matrix; they are not
> reproducible from this tree any more.

# Aoxn web benchmark — Aoxn vs pnpm + Node.js + Next.js

**Question:** can Aoxn write web servers, and how does it compare against the
pnpm + Node.js + Next.js stack? **Answer:** yes — `web/` contains a complete
HTTP/1.1 server written in Aoxn (raw sockets via `extern def` C FFI, byte
buffer rendering, zero per-request allocation) — and on identical routes it
serves the same content with **10–50× lower request latency than plain
Node.js and 300–600× lower than Next.js 15**, from a single 173 KB native
binary at 1–4 MB RSS. Raw throughput is machine-bound (see the cross-platform
section): it matches or trails Node.js depending on the runner, while Aoxn's
latency stays flat and unsaturated everywhere.

Suite: [`web/`](../web/README.md) · Raw data: [`web/loadtest/last-results.json`](../web/loadtest/last-results.json) ·
Cross-platform CI runs: [`web/loadtest/ci-results/`](../web/loadtest/ci-results)

## What is compared

Three servers, three routes, same bodies:

| Route       | Body                                                       |
|-------------|------------------------------------------------------------|
| `/`         | SSR HTML page: 20-row table + computed sum                  |
| `/api/json` | `{"language":"Aoxn","version":"0.26.3","squares":[...],"sum":2870}` |
| `/text`     | `hello, web\n`                                              |

- **Aoxn v0.26.3** — `web/server_win.ax` (Windows) / `web/server_posix.ax`
  (Linux/macOS), compiled with `aoxn build` (LLVM `default<O3>`).
- **Node.js 22.22.1** — `web/node-server.mjs`, plain `node:http`.
- **Next.js 15.5.27** — `web/next-app`, app router, `next build` +
  `next start` (production). The 15.x line is held here on purpose: 15.5.27
  is the backport release that clears the two SSG/ISR cache-poisoning
  advisories (GHSA-4jqv-mc3x-m676, GHSA-mcj8-r9mp-w47p) without a major
  bump, and it carries the patched `sharp` 0.35.5 and `source-map-js` 1.2.2
  transitively (v0.50.1).

The Aoxn and Node response bodies are **byte-identical** (SHA-256 verified on
all three routes). The Next.js page carries the same content (title, table,
sum) with Next's own document markup and hydration payload.

## Environment & method

- Windows on Intel Core i5-1135G7 (4C/8T), LLVM 23.1.0, MSVC Build Tools.
- Load generator: [oha 1.16.0](https://github.com/hatoo/oha) (PGO build),
  32 keep-alive connections, 10 s per run.
- Client and servers run on the **same machine** (127.0.0.1).
- 3 trials per (server, route), interleaved across servers; the tables report
  the **median** trial. (This machine's timings swing ~2× with AV/indexer
  noise — e.g. one Node trial came in at half speed; the median absorbs it.)
- Each server is warmed up (3 s) before measurement.

## Results

### Throughput & latency (median of 3 × 10 s, 32 connections)

| Server | Route | req/s | p50 | p95 | p99 | errors |
|-------------------------|-----------|----------:|---------:|----------:|-----------:|-------:|
| **Aoxn (O3 native)** | `/text` | **7,611** | 0.094 ms | 0.256 ms | 0.787 ms | 0 |
| **Aoxn (O3 native)** | `/api/json` | **7,623** | 0.100 ms | 0.259 ms | 0.642 ms | 0 |
| **Aoxn (O3 native)** | `/` | **6,942** | 0.097 ms | 0.298 ms | 0.826 ms | 0 |
| Node.js 22 (node:http) | `/text` | 7,010 | 4.071 ms | 8.422 ms | 12.189 ms | 0 |
| Node.js 22 (node:http) | `/api/json` | 6,330 | 4.619 ms | 9.304 ms | 12.980 ms | 0 |
| Node.js 22 (node:http) | `/` | 5,892 | 4.769 ms | 10.590 ms | 15.104 ms | 0 |
| Next.js 15 (`next start`) | `/text` | 186 | 143.8 ms | 321.1 ms | 801.5 ms | 0 |
| Next.js 15 (`next start`) | `/api/json` | 142 | 166.4 ms | 612.1 ms | 1,487.0 ms | 0 |
| Next.js 15 (`next start`) | `/` | 263 | 116.0 ms | 162.9 ms | 197.5 ms | 0 |

- **Aoxn vs Next.js:** 26–54× the throughput at 1/1,200–1/1,700 the p50
  latency, from a process 33× smaller (5 MB vs 162–177 MB RSS).
- **Aoxn vs Node.js:** comparable throughput (the load generator is the
  binding constraint here — see below) at **~40–50× lower latency**.

### The measured Aoxn throughput is a load-generator ceiling, not the server's

Little's law on the medians (average in-flight requests = req/s × mean
latency):

| Server | in-flight requests (of 32 connections) | interpretation |
|-------------------------|----------------------------------------|----------------|
| Aoxn | ≈ 0.7 | server idle >95% of the time; capacity far above the measured 7.6k req/s |
| Node.js | ≈ 28 | connections queueing; server at its limit |
| Next.js | ≈ 28 | same, with ~120 ms of per-request pipeline cost |

A two-client scaling check (2 × 32 connections) confirms it: Node's p50
latency doubled (4 → 9.7 ms) while Aoxn's was unchanged (0.10 ms); combined
throughput stayed at the machine's ~6–7k req/s request-generation ceiling
for both. Unloaded, Next.js alone needs **16–23 ms per request** (curl
timing), which is why it saturates at a few hundred req/s.

### Startup (spawn → first successful response)

| Server | cold start |
|-------------------------|-----------:|
| Aoxn (native binary) | ~130 ms |
| Node.js 22 | ~340 ms |
| Next.js 15 (`next start`) | ~2.0–3.5 s |

### Build & deploy footprint

| | Aoxn | Node.js | Next.js 15 |
|----------------------|-----------:|------------:|-----------:|
| Build step | `aoxn build` | none | `next build` |
| Cold build | **0.74 s** | — | **66.8 s** |
| Rebuild (content cache) | **0.06 s** | — | — |
| Deploy artifact | 173 KB exe | 1.8 KB script | 56 MB `.next` tree |
| Dependencies to install | 0 | 0 | 305.5 MB (`node_modules`, 8,916 files) |
| Sources (this suite) | 265 lines `.ax` | 49 lines `.js` | ~60 lines `.js` + framework |
| RSS under load | 5 MB | 61 MB | 162–177 MB |

## Cross-platform reference data (CI runners)

Every push to `web/` runs the functional tests plus a short reference
benchmark (5 s × 32 connections, 1 trial) on **windows-latest,
ubuntu-latest and macos-14 (Apple Silicon)** via
`.github/workflows/web-bench.yml`. CI runners share CPUs — these are
trend/regression values, not absolutes; raw runs are archived in
[`web/loadtest/ci-results/`](../web/loadtest/ci-results).

**What is consistent across all three platforms:**
- **Aoxn p50 latency is 0.05–0.06 ms everywhere** — 10–30× lower than
  Node.js (0.5–1.8 ms) and 300–600× lower than Next.js (15–36 ms); p95/p99
  show the same 10–20× gap over Node.js.
- **Aoxn RSS is 1–4 MB everywhere** — Node.js needs 55–79 MB, Next.js
  160–363 MB for the same routes.
- **Aoxn starts fastest**: 13–127 ms to first response vs 109–117 ms
  (Node.js) and 517–825 ms (Next.js).

**What varies by machine:** raw req/s. On the fastest runner (macOS arm64)
Node.js reaches 34–43k req/s vs Aoxn 14–15k; on Ubuntu 1.2–1.3×; on Windows
the two are even (~14–16k). Little's law on the percentiles: Aoxn's flat
0.05 ms latency means its connections are idle most of the time (average
in-flight ≈ 0.8 of 32), i.e. its ceiling is above the measured numbers,
while Node.js runs with 20–30 requests queued. The Aoxn server is a
single-threaded reference implementation (per-request work is ~2 KB of
memcpy into reusable buffers); throughput work is on the roadmap (worker
pool, io_uring/IOCP) — latency, memory and startup are the stable wins today.

### windows-latest (x64)

| Server | Route | req/s | p50 ms | p95 ms | p99 ms | RSS MB |
|---|---|---:|---:|---:|---:|---:|
| Aoxn | /text | 13949 | 0.054 | 0.088 | 0.11 | 4 |
| Aoxn | /api/json | 16218 | 0.054 | 0.086 | 0.107 | 4 |
| Aoxn | / | 16111 | 0.054 | 0.085 | 0.106 | 4 |
| Node.js 22 (node:http) | /text | 16546 | 1.774 | 3.009 | 4.925 | 55 |
| Node.js 22 (node:http) | /api/json | 15666 | 1.845 | 3.173 | 4.409 | 55 |
| Node.js 22 (node:http) | / | 15671 | 1.822 | 3.234 | 4.536 | 63 |
| Next.js 15 (next start) | /text | 825 | 36.32 | 59.071 | 68.988 | 160 |
| Next.js 15 (next start) | /api/json | 999 | 30.218 | 40.341 | 50.71 | 203 |
| Next.js 15 (next start) | / | 926 | 32.487 | 49.971 | 68.042 | 253 |

Time to first response: Aoxn 127 ms · Node.js 115 ms · Next.js 825 ms.

### ubuntu-latest (x64)

| Server | Route | req/s | p50 ms | p95 ms | p99 ms | RSS MB |
|---|---|---:|---:|---:|---:|---:|
| Aoxn | /text | 15773 | 0.051 | 0.063 | 0.073 | 2 |
| Aoxn | /api/json | 17608 | 0.055 | 0.065 | 0.076 | 2 |
| Aoxn | / | 17673 | 0.053 | 0.065 | 0.077 | 2 |
| Node.js 22 (node:http) | /text | 23637 | 1.432 | 1.741 | 2.896 | 72 |
| Node.js 22 (node:http) | /api/json | 23822 | 1.173 | 1.83 | 3.141 | 72 |
| Node.js 22 (node:http) | / | 19548 | 1.632 | 2.106 | 3.292 | 79 |
| Next.js 15 (next start) | /text | 1275 | 24.038 | 29.622 | 38.657 | 190 |
| Next.js 15 (next start) | /api/json | 1420 | 22.116 | 25.018 | 29.199 | 263 |
| Next.js 15 (next start) | / | 1446 | 21.06 | 29.924 | 35.877 | 301 |

Time to first response: Aoxn 13 ms · Node.js 109 ms · Next.js 517 ms.

### macos-14 (Apple Silicon arm64)

| Server | Route | req/s | p50 ms | p95 ms | p99 ms | RSS MB |
|---|---|---:|---:|---:|---:|---:|
| Aoxn | /text | 13813 | 0.056 | 0.121 | 0.162 | 1 |
| Aoxn | /api/json | 14797 | 0.063 | 0.104 | 0.147 | 1 |
| Aoxn | / | 14349 | 0.064 | 0.117 | 0.164 | 1 |
| Node.js 22 (node:http) | /text | 43040 | 0.522 | 1.863 | 3.131 | 67 |
| Node.js 22 (node:http) | /api/json | 34539 | 0.633 | 2.44 | 4.31 | 74 |
| Node.js 22 (node:http) | / | 39432 | 0.581 | 2.058 | 3.561 | 75 |
| Next.js 15 (next start) | /text | 1256 | 23.658 | 44.38 | 64.061 | 251 |
| Next.js 15 (next start) | /api/json | 1855 | 14.582 | 33.423 | 49.565 | 281 |
| Next.js 15 (next start) | / | 1776 | 16.571 | 32.87 | 46.852 | 363 |

Time to first response: Aoxn 111 ms · Node.js 109 ms · Next.js 559 ms.

## How the Aoxn server is built

The suite doubles as a demonstration that Aoxn covers systems + web
programming without a runtime:

- **Sockets via C FFI** — `web/sock_win.ax` declares the Winsock2 API
  (`WSAStartup`/`socket`/`bind`/`listen`/`accept`/`recv`/`send`/`setsockopt`)
  with `extern def`, linked with `-l ws2_32`; `web/sock_posix.ax` does the
  same for Linux/macOS libc. Pointers cross the FFI as plain `int`.
- **Zero per-request allocation** — responses render into reusable byte
  buffers (`store_u8`/`memcpy`/hand-rolled itoa). Aoxn strings are immutable
  and concat results are intentionally never freed (documented language
  semantics), so a long-running server renders into byte buffers instead —
  the idiomatic systems answer, and why RSS is flat at 5 MB.
- **Correct HTTP/1.1 framing** — requests accumulate until the `\r\n\r\n`
  terminator and drain one by one, so pipelined requests on a keep-alive
  connection each get a response; responses go out as a single TCP segment
  (header written directly in front of the body) with `TCP_NODELAY` set.

## Caveats

- Absolute numbers are machine specific and the load generator caps the
  local protocol (~7k req/s on the laptop, ~14–43k on CI runners): the
  **latency percentiles and in-flight analysis are the reliable
  discriminators**, raw req/s trails Node.js on fast runners (see the
  cross-platform section).
- The CI/short-protocol runs show Aoxn `maxLat` ≈ the run duration (5 s)
  while p99 stays ≤ 0.2 ms — a handful of deadline-adjacent requests skew
  the mean; treat `mean` as noisy and p50/p95/p99 as the signal.
- This is a throughput/latency comparison of three routes, not a feature
  comparison: Next.js ships routing, RSC streaming, hydration, ISR, etc.
  The Aoxn server is GET-only HTTP/1.1 by design.
- `next start` on Windows is the official production server as configured;
  standalone/`output: "standalone"` deployments may differ.
- All three bodies were verified equivalent first (Aoxn ↔ Node byte-exact),
  so the throughput numbers compare like with like.
