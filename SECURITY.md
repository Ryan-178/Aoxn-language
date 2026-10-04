# Security Policy

Aoxn is a compiler: it consumes untrusted text (`.ax` sources, import paths) and
produces native machine code that people then execute. Defects that turn that
pipeline into memory corruption or code execution are security issues, and we
want to hear about them privately before they are public.

---

# English

## Supported versions

Aoxn is pre-1.0. Only the latest release line receives security fixes; older
minors do not.

| Version | Supported |
|---|---|
| `0.40.x` (current) | ✅ yes |
| `0.39.x` and earlier | ❌ no — please reproduce on `main` or the latest release |
| `main` (development) | ✅ yes, fixes land here first |

Fix versions are always noted in [`CHANGELOG.md`](CHANGELOG.md). If you need a
fix backported to an older tag, say so in your report and we will discuss it.

## Reporting a vulnerability

**Email `zmjsjsg3@163.com`.** Please do not open a public issue, and do not post
the details in a discussion, chat, or social media before a fix is available.

A useful report contains:

1. **Version** — `aoxn version` (or the `version` field in `Cargo.toml`), the
   commit/tag if you build from source, and `aoxn doctor` if the issue is
   install-specific (it prints the resolved install root, stdlib and clang).
2. **Platform** — Windows version and architecture, plus the clang version
   from `aoxn doctor`; see
   [`docs/platform-support.md`](docs/platform-support.md).
3. **Reproduction** — the smallest `.ax` source you can manage, the exact
   command you ran, and observed versus expected behavior. If the problem is in
   generated code, include the generated C (`aoxn c file.ax`, or
   `AOXN_DUMP_C=1`) and say whether it still reproduces with `--O0`.
4. **Impact** — what an attacker gains and what they must already control
   (e.g. "compiling this source executes a shell command" or "the emitted
   function writes past a stack buffer").
5. **Credit** — the name or handle you want used, or a request to stay anonymous.

If you want to encrypt the report, say so in a first email with no details and
we will arrange a channel.

### What to expect

| Stage | Target |
|---|---|
| Acknowledgement of your report | within 3 business days |
| Initial assessment (accepted / not a vulnerability / need more info) | within 10 business days |
| Fix for a confirmed critical issue | next release, or a point release if the line is otherwise closed |
| Coordinated disclosure | 90 days after acknowledgement, or as soon as a fix ships — whichever comes first |

We will keep you updated at each stage and ask before publishing anything that
credits you. If we conclude something is not a vulnerability, we will explain
why, and we are happy to be corrected.

There is no bug bounty. We will credit reporters in the advisory and the
changelog unless you prefer otherwise. One thing we will not do is trade a fix
for silence: if an issue is being exploited in the wild, we will publish with
the information needed to protect users even if the reporter disagrees.

## In scope

- **Memory corruption or code execution in generated code** that the Aoxn source
  did not opt into — wrong `memcpy` sizes, aggregate copy/ABI mistakes,
  out-of-bounds pointer arithmetic in the emitter, string/buffer length
  mishandling, miscompilation that turns a defined program into an unsafe one.
- **Code execution or command injection in the toolchain** — the final link step
  shells out to `clang`, the C backend (the only backend since v0.29.0) compiles
  the generated `.c` file with `clang -c`, and the self-hosted driver calls
  `system("clang ...")`. Crafted file names, `-l`/`-L`
  values, or paths that escape into a shell are in scope.
- **Memory-safety defects inside the compiler itself** — unsafe Rust, use of
  freed or recycled AST nodes, raw-pointer misuse in the C emitter
  (`src/codegen_c.rs`) or the FFI helpers.
- **Build and supply-chain integrity** — the CI workflow, the
  published artifacts, or anything that lets a source file or config influence
  the compiler's own binaries. (The compiler library `src/` has zero external
  dependencies by design, which is also a security property; the separate
  `aoxn-pkg` crate uses pinned, widely used crates from `Cargo.lock`. A PR that
  adds a dependency to `src/` needs a very good reason.)
- **Build-cache poisoning** — `aoxn run`/`aoxn build` execute or copy
  executables from `target/cache` based on a content hash; a way to run
  attacker-controlled code through that path is a vulnerability.
- **The package-manifest reader** (`src/pkg_manifest.rs`, since v0.29.1) — a
  hand-rolled, zero-dependency JSON parser that reads untrusted
  `aox_modules/<pkg>/aoxn.json` files to resolve bare package imports. Like
  the lexer/parser, it consumes untrusted text, so a malformed manifest that
  causes memory corruption or an unbounded hang in the compiler process is in
  scope. (It is deliberately lenient — a bad manifest falls back to the
  directory probe rather than aborting — but a panic or memory-unsafe read is
  still a defect.)
- **The CSS asset pipeline** (`src/assets.rs`, since v0.34.0; `url()` rewriting
  and `--emit-assets` in v0.36.0) — reads stylesheets named by source `import`
  directives, inlines `@import` chains, rewrites class names in `*.module.css`,
  and (v0.36.0) rewrites and emits `url()` targets. Like the lexer and the
  manifest reader it is hand-rolled, zero-dependency, consumes untrusted text,
  and runs inside the compiler process — so a malformed or hostile stylesheet
  causing memory corruption, an out-of-bounds slice, or an unbounded hang is in
  scope. Its scanners walk byte strings with explicit index arithmetic (skipping
  strings, comments and balanced braces by hand), so out-of-bounds reads and
  non-advancing loops are the defects to look for.
  **v0.36.0 changed this surface**: `--emit-assets <dir>` writes files, and a
  stylesheet now influences their *contents* through `url()` rewriting. The
  invariants that must hold are: emitted names are compiler-generated
  (`<stem>.<16 hex>.<ext>`) and re-checked against a plain-name predicate that
  rejects separators, `..`, and absolute paths before any write; a `url()`
  target is **read**, never executed; and a stylesheet can never steer a write
  outside the directory the user named. A defect that lets an attacker-chosen
  stylesheet write or overwrite a file elsewhere on disk is a vulnerability.
- **The npm bridge** (`crates/aoxn-pkg/src/npm.rs`, since v0.29.5) — imports
  untrusted npm tarballs: extraction rejects `..` traversal, the sha512
  (`dist.integrity`) is verified before unpacking, and only packages with
  an `aoxn.json` root are accepted. A traversal entry that lands outside
  `vendor/`, an integrity bypass, or a path-dep recording that escapes the
  project directory is a vulnerability.
- **Denial of service that is not just "a bad program"** — a small, well-formed
  input that hangs the compiler indefinitely or exhausts memory catastrophically
  is worth reporting; see the note below on where the line is.
- **Parse-time expansion of the Python-parity forms** (since v0.29.7:
  augmented assignment, `//`, unary `+`, chained comparison) — these desugar
  into nodes the checker and the emitter already understood, so they inherit
  its rules. A chain or an augmented assignment duplicates the operand it
  uses twice (`a < b < c` evaluates `b` twice); the expansion is linear in the
  size of the written expression, and both the Rust parser
  (`src/parser.rs`) and the self-hosted one (`selfhost/parser.ax`,
  `dup_expr`) must stay bounded — an operand whose copy cost is
  super-linear in the source length, or a recursive `dup_expr` that a crafted
  depth can turn into stack exhaustion, is a defect.
- **Install-layout discovery** (since v0.30.0: `src/paths.rs`) — the compiler
  now derives its install root, stdlib directory and bundled clang from
  `AOXN_HOME` / `AOXN_STDLIB` and its own executable location, and resolves
  `import * from "stdlib"` by name. The precedence must stay explicit
  (`AOXN_CLANG` > `PATH` > bundled > system) and a project-local
  `aox_modules/<name>` package must keep winning over the installed stdlib.
  Anything that inverts that order, or that lets an attacker-controlled
  directory take over as the stdlib or as clang via `AOXN_HOME` or a directory
  beside the executable, is a vulnerability.
- **The single-file installer** (since v0.30.0: `src/setup/`, `dist/package.ps1`)
  — it writes to PATH, unpacks its appended payload and invokes winget. The
  payload must keep coming from the packager that produced the exe, unpacking
  must never write outside the install root (entry paths are checked), and
  the unpacked `bin\aoxn.exe` must be the one that actually runs. Anything
  that corrupts a user's PATH, writes outside the install root, escapes the
  payload sandbox, or leaves a different binary on PATH than the one
  installed is a vulnerability.
- **The IDE's workspace boundary** (since v0.31.0: `ide/src-tauri/src/fsops.rs`,
  extended in v0.31.1) — the IDE's webview reaches the filesystem only
  through the Rust command layer, and every path goes through
  `Workspace::resolve` first: canonicalised, refused if it resolves outside
  the opened folder (compared component-wise), creation commands refuse to
  clobber. A webview compromise is assumed; anything that lets it read,
  write, create, or execute outside the opened folder — a traversal the
  resolver misses, a sibling-directory prefix confusion, a command that
  skips the gate, or an invented path reaching the shell — is a
  vulnerability. (There is deliberately no read-any-path command and no
  shell plugin; adding either is a design change, not a bug fix.)
- **The IDE's package-manager gate** (since v0.33.0:
  `ide/src-tauri/src/pkg.rs`) — the packages panel runs `aoxn pkg` from the
  opened folder, and `ALLOWED_SUBCOMMANDS` is the entire surface the webview
  can name: inspection plus local project mutations. Outward-facing or
  global commands (publish, yank, cache, trust bootstrap, npm-import) are
  refused in Rust, and package names typed into the panel are validated
  before they reach clap. A way to run a non-whitelisted subcommand, to
  smuggle a flag through the name field, or to make a command run outside
  the opened folder is a vulnerability.
- **The registry download path** (since v0.32.0: `Registry::fetch_tarball` in
  `crates/aoxn-pkg/src/registry/`) — three backends fetch untrusted tarballs,
  and the integrity contract has two distinct digests that must not be
  confused again. `checksum` is the **manifest hash** over the unpacked file
  tree; `tarball_sha256` is the digest of the **tarball bytes as served**, and
  only the HTTP backend may compare against it. (Until v0.32.0 the HTTP
  backend compared the download against `checksum` and therefore rejected
  every real download — a failure, not a bypass, but the same confusion in
  reverse would be a silent integrity hole.) What is in scope: any path where
  a fetched or extracted tree is materialized without the post-extraction
  manifest-hash check against the value pinned in `aoxn.lock`; any change
  making the transport digest optional *bypass* the content check rather than
  merely skip the wire check; any traversal in `tarball::unpack`; any registry
  handle shared across the download threads added in v0.32.0 (each worker must
  keep its own `Registry::fork()` — backends carry mutable state).
- **Trust-tier handling** (since v0.32.0: `crates/aoxn-pkg/src/trust.rs`) —
  `trust.json` is a curated registry's *claims about itself*, parsed from
  untrusted text like any other registry file. `None` (no trust index) and
  `unreviewed` must stay distinguishable from each other and from a reviewed
  tier; an unknown `schema` must be refused rather than guessed at; and
  nothing in this path may weaken or substitute for the lockfile's manifest
  hash. A tier that silently promotes to `audited`, or a `--tier` gate that
  passes for an unlisted package, is a vulnerability.
- **`--prod` materialization** (since v0.32.0: `install::materialize`) — the
  dev/prod split decides what lands in `aox_modules/`. A package reachable
  from a production root must never be pruned, and a package named in both
  `dependencies` and `devDependencies` counts as production. A build that
  loses a dependency it declared is a denial of service on that project.
- **stdlib runtime parsers over hostile input** (since v0.40.1, and two
  consumers wide since v0.41.0: `stdlib/net/json.ax`, `sse.ax`, `http.ax`,
  used by both `stdlib/openai/` and `stdlib/anthropic/`) — the JSON DOM and
  its builder, the SSE framer, the URL/header splitting and the raw
  response-header scan parse bytes a remote server controls, inside the
  user's process. They are hand-written index arithmetic over malloc'd
  buffers, so a malformed response that overruns a buffer, walks off the
  DOM slab, or loops without advancing is a stdlib defect (same standing
  as the UI backend's message parsing below), not merely "the program's
  bug". Three specifics worth naming because each was found the hard way:
  the DOM accessors answer `-1` for an absent key and callers pipe that
  straight into another accessor (unguarded, it read before the slab);
  `jb_set_raw` / `jb_push_raw` splice a parsed fragment into an existing
  slab, so the stale pre-`realloc` pointer must never be used again; and
  the response-header lookup scans a block whose length comes from the
  server. The SDK test suites feed hostile fixtures offline precisely so
  these paths stay provable; the SDKs never run inside the compiler
  process and add no compiler-side surface.
- **Credential handling in the SDK clients** (v0.41.0) — `AnClient` /
  `OaClient` hold the API key as an ordinary Aoxn string, so it lives in
  the heap for the process's lifetime and is passed by pointer. Nothing
  scrubs it, and a core dump or a crash report will contain it. The
  reference Python SDK has the same property. Prefer the environment
  (`an_client_env()` / `oa_client_env()`) over a literal in source, and do
  not print an `AnClient` / `OaClient` struct: `print` on a struct renders
  its fields, key included.

## Out of scope (documented behavior)

These are deliberate design decisions, documented in
[`docs/spec.md`](docs/spec.md). Please do not report them as vulnerabilities —
though a bug report about the *documentation* is welcome.

- **Unchecked array indexing and signed integer overflow.** Like C, these are
  undefined behavior in the language contract. Indexing out of bounds or
  overflowing an `int` in Aoxn source is the program's bug, not the compiler's.
- **Raw memory builtins** (`load_i64`, `store_i64`, `load_f64`, `store_f64`,
  `load_u8`, `store_u8`, `as_ptr`, `as_string`). These exist as the
  self-hosting escape hatch and are unsafe by design; unchecked pointer
  arithmetic on their arguments is intentional.
- **Memory growth from string concatenation.** Concat results are never freed
  (immutable strings, no GC yet). It is stated behavior, not a leak bug.
- **Antivirus quarantining the freshly installed compiler.** Aoxn ships one
  unsigned `Setup.exe` that unpacks an unsigned `aoxn.exe`, and Windows Smart
  App Control / Defender routinely quarantine a new, unsigned executable
  *after* its first run. The installer does not pretend this cannot happen:
  it re-checks the binary after its self-test, waits out a short antivirus
  hold, and if the file is genuinely gone it fails the install naming the
  cause and the remedy (exclude `%LOCALAPPDATA%\aoxn` from real-time
  scanning) rather than leaving a PATH entry pointing at nothing. An
  environment whose policy forbids unsigned binaries should be given a
  signed release; that is a deployment decision, not an Aoxn vulnerability.
  What *is* in scope is a defect in the detection itself — for instance the
  installer reporting Done while the compiler has become unusable.
- **Compiler crashes, hangs, or wrong error messages on malformed input.** These
  are bugs — file them with the
  [bug report template](https://github.com/AlonechatWorkspace/Aoxn-language/issues/new?template=bug_report.yml).
  Escalate to a security report only if the failure involves memory corruption
  in the compiler process, code execution, or a wrong-code emission that
  silently makes a valid program unsafe.
- **Vulnerabilities in programs people compile with Aoxn.** Aoxn provides no
  sandbox and no runtime safety net; the compiled program's behavior is the
  program author's responsibility. (The UI toolkit's raw FFI and raw-memory
  helpers are unsafe by design, like everything above.)
- **The window server as an untrusted input source.** The Win32/GDI backend
  (`ui_win.ax`) speaks to whatever window owns the process, so a hostile (or
  merely compromised) window manager is in the same position as a hostile
  terminal — it can feed the toolkit crafted messages. This is inherent to
  speaking a windowing protocol and is not a sandbox boundary the toolkit
  claims. (The X11 backend that had the same property was removed in
  v0.30.0 along with the non-Windows platforms.) Defects in *how* the
  backend parses that input (buffer overruns from a crafted message,
  out-of-bounds writes into the scratch blocks) ARE in scope and should be
  reported.
- **A curated registry's trust index making unchecked claims.** The trust
  index says *who reviewed a package's source*, not what the source does. A
  registry listing a malicious package as `audited`, or a reviewer vouching
  for code they did not read, is a curation failure, not a vulnerability in
  Aoxn. Installing an `unreviewed` package warns and proceeds by design;
  refusing it in CI is `aoxn trust check <pkg> --tier audited` as a
  deliberate, visible step. What *is* in scope is a bug in the tier logic
  itself (see the in-scope entry above).
- **The TLS-free HTTP registry backend.** Aoxn's package manager HTTP client
  is hand-rolled on `std::net::TcpStream` and rejects `https://` outright, so
  a mirror fetched over plain HTTP has no transport authentication — neither
  party is authenticated and the response is not confidential. This is a
  documented design constraint, not an oversight: it keeps the crate's
  dependency set free of build scripts. Deploy such a mirror only on a
  network you trust. Defects *in* the client (request smuggling, a redirect
  that escapes the configured base, a body-length check that can be fooled)
  remain in scope.
- **Upstream clang / MSVC defects.** Report those upstream — but do tell
  us if the compiler depends on the broken behavior.
- **Anything requiring an attacker who already controls the machine** or the
  terminal the compiler runs in. The documented `AOXN_*` knobs are
  configuration, not an attack surface — but a crafted value that escapes into
  the link command or poisons the build cache is in scope (see above).

## Safe harbor

We will not pursue or support legal action against researchers who:

- act in good faith and follow this policy,
- test only against their own builds and data,
- avoid privacy violations, data destruction, and disruption of services they do
  not own,
- give us reasonable time to fix the issue before public disclosure, and
- do not use social engineering, physical attacks, or denial-of-service against
  infrastructure (the compiler is a local tool; there is no service to test).

If you are unsure whether something is in scope, ask first — email
`zmjsjsg3@163.com` with just enough detail to describe the area, and we will tell
you how we want it handled.

---
---

# 中文

Aoxn 是一个编译器：它消费不可信的文本（`.ax` 源码、导入路径），产出人们随
后执行的原生机器码。把这条管线变成内存破坏或代码执行的缺陷就是安全问题——
我们希望在公开之前私下收到报告。

## 支持的版本

Aoxn 处于 pre-1.0 阶段：只有最新的版本线接收安全修复，旧的次版本不再修。

| 版本 | 支持情况 |
|---|---|
| `0.40.x`（当前） | ✅ 支持 |
| `0.39.x` 及更早 | ❌ 不支持——请在 `main` 或最新发布上复现 |
| `main`（开发线） | ✅ 支持，修复最先落在这里 |

修复版本永远记在 [`CHANGELOG.md`](CHANGELOG.md)。如需把修复反向移植到旧
tag，请在报告里说明，我们再商量。

## 报告漏洞

**发邮件到 `zmjsjsg3@163.com`。** 请不要开公开 issue，修复可用之前也不要把
细节发到讨论区、聊天或社交媒体。

一份有用的报告包含：

1. **版本** —— `aoxn version`（或 `Cargo.toml` 的 `version` 字段），从源码
   构建的请附 commit/tag；若问题与安装有关，请一并附上 `aoxn doctor` 的输出
   （它会打印解析到的安装根目录、stdlib 与 clang）。
2. **平台** —— Windows 版本与架构，外加 `aoxn doctor` 打印的 clang 版本；见
   [`docs/platform-support.md`](docs/platform-support.md)。
3. **复现** —— 尽可能小的 `.ax` 源码、确切命令、实测行为与预期行为。若问题
   出在生成代码里，请附生成的 C（`aoxn c file.ax` 或 `AOXN_DUMP_C=1`），并说明
   `--O0` 下是否仍复现。
4. **影响** —— 攻击者得到什么、已须控制什么（例如"编译该源码会执行 shell
   命令"或"生成的函数越过栈缓冲写入"）。
5. **署名** —— 你希望使用的姓名/昵称，或要求匿名。

如需加密报告，请先发一封不含细节的邮件说明，我们再安排通道。

### 你会得到什么

| 阶段 | 目标时限 |
|---|---|
| 确认收到你的报告 | 3 个工作日内 |
| 初步评估（接受 / 不是漏洞 / 需要更多信息） | 10 个工作日内 |
| 确认的关键问题的修复 | 下个发布；若该线已关闭则发补丁版本 |
| 协同披露 | 收到报告起 90 天内，或修复发出即披露——以先到者为准 |

每个阶段我们都会同步进展；发布任何署名内容前会先征得同意。如果我们判定
不是漏洞，会说明理由，也欢迎你反驳。

没有漏洞赏金。除非你另有要求，我们会在公告与变更日志里署名致谢。有一件事
我们不做：用修复换沉默——若漏洞正在被野外利用，即使报告者不同意，我们也会
连同保护用户所需的信息一起公开。

## 范围内

- **生成代码中的内存破坏或代码执行**，且 Aoxn 源码并未主动选择危险行为——
  错误的 `memcpy` 尺寸、聚合复制/ABI 缺陷、发射器中的越界指针运算、
  字符串/缓冲长度处理错误、把良定义程序误编译成不安全程序。
- **工具链中的代码执行或命令注入** —— 最终链接步骤 shell 出去调 `clang`，
  C 后端（v0.29.0 起的唯一后端）会对编译器生成的 `.c` 文件调 `clang -c`，
  自举 driver 调 `system("clang ...")`；构造的文件名、`-l`/`-L`
  值或路径逃逸进 shell 的都在范围内。
- **编译器自身的内存安全缺陷** —— unsafe Rust、释放/回收后 AST 节点的误用、
  C 发射器（`src/codegen_c.rs`）或 FFI 辅助中的原始指针误用。
- **构建与供应链完整性** —— CI 工作流、发布产物，或任何让源码/
  配置影响编译器自身二进制的路径。（编译器库 `src/` 有意保持零外部依赖，
  这本身也是安全属性；独立的 `aoxn-pkg` crate 使用 `Cargo.lock` 锁定的
  成熟第三方 crate。往 `src/` 加依赖的 PR 需要充分理由。）
- **构建缓存投毒** —— `aoxn run`/`aoxn build` 按内容哈希从 `target/cache`
  执行或拷贝可执行文件；能通过该路径运行攻击者控制的代码即为漏洞。
- **包 manifest 读取器**（`src/pkg_manifest.rs`，v0.29.1 起）—— 手写的零依赖
  JSON 解析器，读取不可信的 `aox_modules/<pkg>/aoxn.json` 来解析裸包导入。与
  词法/语法分析器一样消费不可信文本，因此畸形 manifest 若在编译器进程中造成
  内存破坏或无界挂起，属范围内。（解析器刻意宽松——坏 manifest 回退到目录探针
  而非中止——但 panic 或内存不安全读取仍是缺陷。）
- **CSS 资产管线**（`src/assets.rs`，v0.34.0 起；v0.36.0 增加 `url()` 改写与
  `--emit-assets`）—— 读取由源码 `import` 指定的样式表、内联 `@import` 链、
  改写 `*.module.css` 的类名，并（v0.36.0 起）改写与产出 `url()` 目标。与词法
  分析器和 manifest 读取器一样：手写、零依赖、消费不可信文本、运行在编译器
  进程内——因此畸形或恶意样式表若造成内存破坏、越界切片或无界挂起，属范围内。
  它的扫描器用手写索引运算遍历字节串（自行跳过字符串、注释与配对花括号），所以
  越界读取与「不前进的循环」是重点排查对象。
  **v0.36.0 改变了这一攻击面**：`--emit-assets <dir>` 会写文件，且样式表现在能
  通过 `url()` 改写影响文件的**内容**。必须维持的不变量是：产出名一律由编译器
  生成（`<stem>.<16 hex>.<ext>`），并在写入前用「纯文件名」谓词复查（拒绝分隔符、
  `..` 与绝对路径）；`url()` 目标只被**读取**，从不执行；样式表永远无法把写入
  引到用户指定目录之外。能借助攻击者可控的样式表在磁盘别处写入或覆盖文件的
  缺陷，即属漏洞。
- **npm 桥接**（`crates/aoxn-pkg/src/npm.rs`，v0.29.5 起）—— 导入不可信的
  npm tarball：解包拒绝 `..` 目录穿越，sha512（`dist.integrity`）在解包前
  验证，且只接受根目录带 `aoxn.json` 的包。能让文件落到 `vendor/` 之外的
  穿越条目、绕过完整性校验、或把路径依赖记录逃逸出项目目录的行为，均属漏洞。
- **不只是"坏程序"的拒绝服务** —— 一个小的、格式良好的输入让编译器无限挂起
  或灾难性耗尽内存的，值得报告；界线见下文。
- **Python 对标语法的解析期展开**（v0.29.7 起：增强赋值、`//`、一元 `+`、
  链式比较）—— 它们降级成类型检查与代码生成本就理解的节点，因此沿用其规则。
  链式比较与增强赋值会把用到两次的那个操作数复制一份（`a < b < c` 会求值两次
  `b`），展开规模相对源码长度是线性的：Rust 侧解析器（`src/parser.rs`）与自举
  侧（`selfhost/parser.ax` 的 `dup_expr`）都应保持有界——若复制代价相对源码
  长度变成超线性，或构造出的嵌套深度能把递归 `dup_expr` 变成栈耗尽，即为缺陷。
- **Install-layout discovery**（v0.30.0 起，`src/paths.rs`）—— 编译器现在从
  `AOXN_HOME` / `AOXN_STDLIB` 环境变量与自身可执行文件位置推导安装根目录、
  标准库与随包携带的 clang，并按名字解析 `import * from "stdlib"`。环境变量
  优先级必须保持显式（`AOXN_CLANG` > `PATH` > 随包 > 系统），本地
  `aox_modules/<name>` 包必须始终优先于安装的标准库；能让该顺序被绕过、或让
  一个受攻击者控制的目录经由 `AOXN_HOME`/可执行文件邻接目录顶替标准库或
  clang 的行为，均属漏洞。
- **单文件安装器**（v0.30.0 起，`src/setup/`、`dist/package.ps1`）—— 它会写
  PATH、解开追加在自身后面的载荷并调用 winget。载荷必须来自生成该 exe 的打包
  脚本；解包绝不能写出安装根目录（条目路径已做校验）；解压出的 `bin\aoxn.exe`
  必须被实际执行。篡改用户 PATH、写出安装根目录、逃出载荷沙箱，或让 PATH 上
  留下与所装安装器不同的二进制，均属漏洞。
- **IDE 的工作区边界**（v0.31.0 起，`ide/src-tauri/src/fsops.rs`，v0.31.1 扩展）——
  IDE 的 webview 只通过 Rust 命令层触达文件系统，且每个路径都先过
  `Workspace::resolve`：规范化、解析后落在打开目录之外即拒绝（按路径分量
  比较）、创建类命令拒绝覆盖已有条目。威胁模型默认 webview 已被攻破：任何
  让它读到、写到、创建或执行打开目录之外内容的行为——解析器漏掉的穿越、
  同名前缀目录的混淆、绕过闸门的命令、逃逸进 shell 的路径——均属漏洞。
  （刻意不设"读任意路径"的命令，也不装 shell 插件；要加属于设计变更，
  不是修 bug。）
- **IDE 的包管理闸门**（v0.33.0 起，`ide/src-tauri/src/pkg.rs`）—— 包面板在
  打开的目录里运行 `aoxn pkg`，`ALLOWED_SUBCOMMANDS` 就是 webview 能点名的
  全部表面：只读检查加上本地项目级变更。对外或全局的命令（publish、yank、
  cache、trust bootstrap、npm-import）在 Rust 层直接拒绝，面板里输入的包名
  也会先校验再交给 clap。任何能运行白名单之外子命令、能让标志位从名字字段
  混过去、或能让命令在打开目录之外执行的方式，均属漏洞。
- **registry 下载路径**（v0.31.0 起，`crates/aoxn-pkg/src/registry/` 的
  `Registry::fetch_tarball`）—— 三个后端都拉取不可信的 tarball，而完整性契约里
  有两个**不可混淆**的摘要：`checksum` 是对解包后文件树的 **manifest 哈希**，
  `tarball_sha256` 是**所服务的 tarball 字节**的摘要，且只有 HTTP 后端才允许拿
  下载内容与后者比对。（v0.32.0 之前 HTTP 后端拿下载内容与 `checksum` 比，
  结果每一次真实下载都被拒——那是失败而不是绕过，但同一种混淆反过来就会是
  静默的完整性漏洞。）范围内：任何让取回或解包后的目录树未经「与 `aoxn.lock`
  中钉住的值做 manifest 哈希校验」就落盘的路径；任何把传输摘要变成可选时
  **绕过**内容校验（而非仅跳过线路校验）的改动；`tarball::unpack` 中的任何
  目录穿越；v0.32.0 新增的下载线程之间共享 registry 句柄（每个 worker 必须
  持有自己的 `Registry::fork()`——后端带可变状态）。
- **信任等级的处理**（v0.31.0 起，`crates/aoxn-pkg/src/trust.rs`）——
  `trust.json` 是策展 registry 对**自己**的声明，和其他 registry 文件一样解析自
  不可信文本。`None`（没有信任清单）与 `unreviewed` 必须彼此可区分，也必须与
  「已评审」可区分；未知的 `schema` 必须被拒绝而不是猜测；这条路径上的任何
  环节都不得削弱或替代锁文件里的 manifest 哈希。等级被静默提升为 `audited`，
  或未收录的包能通过 `--tier` 门禁，均属漏洞。
- **`--prod` 物化**（v0.31.0 起，`install::materialize`）—— dev/prod 的划分
   决定什么会落进 `aox_modules/`。从生产根可达的包绝不能被裁掉；同时出现在
   `dependencies` 与 `devDependencies` 的包按生产算。让某个项目丢掉它自己声明
   的依赖，即是对该项目的拒绝服务。
- **标准库中解析敌意输入的运行时解析器**（v0.40.1 起，v0.41.0 起有两个使用方：
   `stdlib/net/json.ax`、`sse.ax`、`http.ax`，由 `stdlib/openai/` 与
   `stdlib/anthropic/` 共用）—— JSON DOM 及其构建器、SSE 分帧、URL/请求头拆分与
   原始响应头扫描解析的是远端服务器可控的字节，运行在用户进程内。它们是对 malloc
   缓冲的手写索引运算：一个畸形响应若造成缓冲越界、走出 DOM slab、或不前进的死循环，
   属于标准库缺陷（与下文 UI 后端的消息解析同一待遇），而不只是"程序的 bug"。
   有三处值得点名，因为每一处都是踩出来的：DOM 访问器对缺失的键返回 `-1`，而调用方
   会把它直接喂给另一个访问器（无保护时它会读到 slab 之前）；`jb_set_raw` /
   `jb_push_raw` 把解析出的片段拼进已有的 slab，因此 realloc 之前的旧指针绝不可再用；
   响应头查找扫描的块，其长度来自服务器。SDK 测试套件离线投喂敌意 fixture，正是为了
   让这些路径保持可证明；SDK 从不在编译器进程内运行，也不新增编译器侧攻击面。
- **SDK 客户端中的凭据处理**（v0.41.0）—— `AnClient` / `OaClient` 把 API key 当作
   普通 Aoxn 字符串持有，因此它在堆里存活整个进程生命周期，并以指针传递。没有任何机制
   会擦除它，core dump 或崩溃报告里就会有它。参考 Python SDK 同样如此。请优先用环境变量
   （`an_client_env()` / `oa_client_env()`）而不是源码里的字面量，并且不要 `print` 一个
   `AnClient` / `OaClient` 结构体：`print` 会把字段逐个渲染出来，key 就在其中。

## 范围外（文档化行为）

以下是有意的设计决定，记录在 [`docs/spec.md`](docs/spec.md)。请勿作为漏洞
报告——不过针对*文档本身*的缺陷报告欢迎。

- **不检查的数组索引与有符号整数溢出。** 与 C 相同，语言契约中是未定义行为。
  Aoxn 源码里越界索引或 `int` 溢出是程序的 bug，不是编译器的。
- **原始内存内建**（`load_i64`、`store_i64`、`load_f64`、`store_f64`、
  `load_u8`、`store_u8`、`as_ptr`、`as_string`）。它们是自举逃生舱，设计上
  就不安全；对其参数做不检查的指针运算是有意为之。
- **字符串拼接的内存增长。** 拼接结果永不释放（不可变字符串，尚无 GC）。这是
  成文行为，不是泄漏 bug。
- **杀毒软件隔离刚安装的编译器。** Aoxn 只发布一个未签名的 `Setup.exe`，它解
  出一个同样未签名的 `aoxn.exe`；而 Windows Smart App Control 与 Defender 常常
  在新生成、未签名的可执行文件*首次运行之后*将其隔离。安装器不回避这一点：它在
  自检之后重新确认该二进制还在，会等出杀软短暂的占用窗口，若文件确实消失则以
  明确指明原因与补救办法的方式让安装失败（把 `%LOCALAPPDATA%\aoxn` 排除在实时
  扫描之外），而不是给用户留一个指向空处的 PATH 条目。若某环境的安全策略不允许
  未签名二进制，应给它一份签名发行版——那是部署决策，不是 Aoxn 的漏洞。**属于范
  围内**的是该检测本身的缺陷，例如安装器报告 Done 而编译器已不可用。
- **编译器在畸形输入上的崩溃、挂起或错误信息。** 这些是 bug——用
  [bug report 模板](https://github.com/AlonechatWorkspace/Aoxn-language/issues/new?template=bug_report.yml)
  提交。仅当涉及编译器进程内存破坏、代码执行，或把良定义程序静默输出成不安
  全代码时，才升级为安全报告。
- **人们用 Aoxn 编译出的程序里的漏洞。** Aoxn 不提供沙箱和运行时安全网；编
  译产物的行为由程序作者负责。（UI 工具箱的原始 FFI 与原始内存辅助同理，
  与上述一切一样设计上不安全。）
- **把窗口服务器当作不可信输入源。** Win32/GDI 后端（`ui_win.ax`）会与拥有
  该进程的窗口对话，因此恶意的（或已被攻破的）窗口管理器与恶意终端处于同一
  位置：它可以向工具箱投喂构造的消息。这是「使用窗口协议」本身固有的性质，
  并非工具箱声称的沙箱边界。（具有同样性质的 X11 后端已随非 Windows 平台在
  v0.30.0 一并移除。）但后端*解析*这些输入时的缺陷（构造消息导致的缓冲区溢
  写、越界写进暂存块）**属于范围内**，请报告。
- **策展 registry 的信任清单做出未经核实的声明。** 信任清单说明的是*谁评审过
  某个包的源码*，而不是源码做了什么。一个把恶意包标成 `audited` 的 registry，
  或评审者为自己没读过的代码背书，属于策展失职而非 Aoxn 的漏洞。安装
  `unreviewed` 包时只告警并继续，属有意设计；在 CI 里拒绝它是显式的一步
  `aoxn trust check <pkg> --tier audited`。*属于范围内*的是等级逻辑本身的缺陷
  （见上文范围内条目）。
- **无 TLS 的 HTTP registry 后端。** Aoxn 包管理器的 HTTP 客户端是手写在
  `std::net::TcpStream` 上的，并且直接拒绝 `https://`，因此走明文 HTTP 取镜像
  没有传输层认证——双方都不认证，响应也不保密。这是有记录的设计约束而非疏
  漏：它让该 crate 的依赖集保持无 build script。此类镜像只应部署在你信任的
  网络上。客户端*自身*的缺陷（请求走私、能逃出所配置 base 的重定向、可被欺
  骗的响应体长度检查）仍在范围内。
- **上游 clang / MSVC 的缺陷。** 请报给上游——但若编译器依赖了该坏行
  为，请告知我们。
- **任何已控制编译器所在机器或终端的攻击者才能利用的问题。** 文档化的
  `AOXN_*` 旋钮是配置而非攻击面——但构造的值逃逸进链接命令或毒化构建缓存
  仍在范围内（见上）。

## 安全港

对符合以下条件的研究人员，我们不会追究或支持法律行动：

- 善意行事并遵守本政策；
- 只对自己的构建与数据进行测试；
- 不侵犯隐私、不销毁数据、不干扰不属于自己的服务；
- 公开披露前给我们合理的修复时间；
- 不使用社会工程、物理攻击或对基础设施的拒绝服务（编译器是本地工具，没有
  服务可打）。

不确定某问题是否在范围内，先问——发邮件到 `zmjsjsg3@163.com`，只需足够描述
领域的细节，我们会告诉你希望如何处理。
