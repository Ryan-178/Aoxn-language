# Security policy

## English

### Supported versions and reporting

The compiler is **0.55.0**, supported on the **0.55.x** line. The independent
package-manager crate is **0.42.0**. Fixes land on main first; earlier minor
lines are not maintained. Windows and Linux are supported, macOS is not.
Version history is in [CHANGELOG.md](CHANGELOG.md).

Report vulnerabilities privately to **zmjsjsg3@163.com**, before public issues
or disclosure. Include commit/version, OS/architecture, `aoxn doctor`, minimal
input, commands, expected/actual behavior and impact. For miscompilation include
`aoxn c` output and whether O0 reproduces it. Redact secrets and private paths;
state credit preference. An initial nonsensitive email can arrange encryption.

Targets: acknowledgement within **3 business days**, initial assessment within
**10 business days**, coordinated disclosure within **90 days of acknowledgement
or when a fix ships, whichever comes first**. Confirmed critical issues target
an urgent patch or next release. Reporters receive progress and explanations;
credit requires consent. There is no bounty. Active exploitation may require
earlier protective guidance.

### Trust boundaries

Compiling or executing Aoxn is **not a sandbox**. Imports read files, asset
commands write files, and clang produces native code. Rust invokes clang using
argument vectors; the self-hosted driver also has a shell-based path. Isolate
untrusted projects with OS/container controls over files, network and credentials.
Type checking is not permission to execute untrusted output.

`src/` has zero external Rust crate dependencies. Package-manager dependencies
are pinned in Cargo.lock and do not depend on the compiler library. IDE and
benchmark JS/Tauri dependencies are separate supply-chain surfaces. Emitter/ABI
verification does not establish sandboxing or constitute a security audit.

### Compiler and build invariants

- Crafted sources, imports, assets and diagnostics must not corrupt the compiler,
  execute unintended commands or miscompile defined programs. Ordinary wrong
  diagnostics/panics are bugs; small-input catastrophic resource amplification
  also warrants private reporting.
- Generated names must derive from stable source identities/counters/names,
  never addresses. Hash iteration must not change C output. Cache keys include
  compiler identity, options and dependencies. Do not share writable caches with
  untrusted users.
- Direct Rust-vs-selfhost C parity and the self-host stage fixed point are
  different checks. Equal fixture outputs do not prove every feature equivalent.
  Selfhost lacks several newer language features. See
  [C compatibility](docs/c-compatibility.md) for exact limits and validation.
- ABI correctness requires a matching external declaration. Aoxn `int` maps to
  `long long`, bool to C `int`; neither implies arbitrary C `int`/`_Bool`
  interoperability. Fixed-arity externs do not implement variadic C calls.
  A mismatched user declaration may link yet have undefined behavior; tests with
  known C helpers are not a validator for arbitrary third-party signatures.
- Graph optimizations preserve semantics and diagnostic order. Reverse exception
  edges, borrowed module tables and reused acyclic proofs remove specific costs,
  not all denial-of-service opportunities. Recursive passes retain host-stack
  limits. Wrapped/array indirect struct cycles remain a known checker limitation.
- Packages take precedence over installed stdlib. Bare imports probe siblings
  first; quoted imports have sibling fallback. Environment paths, AOXN_CLANG and
  link flags are caller-controlled configuration, not an adversarial boundary.
- CSS emitted filenames are generated and checked against traversal/separators.
  URLs influence content, not output roots. Tailwind does not silently fetch Node.

### Package, installer and application boundaries

- Package checksum pins unpacked content; tarball_sha256 checks served bytes when
  supplied. Missing legacy transport digests never waive content verification.
  Warm reuse hashes the real tree, excluding **only its own index.ax shim**;
  mismatches rebuild and verify. Broader exclusions or unverified reuse are flaws.
- Extraction rejects escaping/absolute paths and links. Dependency/cache path
  components are validated. npm-import checks integrity and Aoxn metadata.
  "Absolute" is host-defined (`C:/x` is a drive path on Windows, a directory
  called `C:` on POSIX), and the guard follows the host because `Path::join`
  does; the host-independent property is that no entry writes outside its
  destination, pinned by tests on both spellings.
  HTTP registries are plain HTTP, not HTTPS; an untrusted index can replace data
  and hashes. Registry tiers are claims, not independent audits.
- Parallel workers own forked registry state, unknown schemas fail, and prod
  pruning must retain production-reachable dependencies.
- Windows installation stays under its selected root, updates PATH and rechecks
  the compiler after self-test. Downloads are disabled by default; InstallClang
  is opt-in. Quarantine is OS policy; falsely reporting install success is a bug.
- The IDE canonicalizes workspace paths, rejects link escapes/clobbering, recreates
  outputs rather than following hard links and validates commands in Rust.
  Hostile webviews must not acquire arbitrary shell/global command execution.
- JSON/UTF/SSE/URL/headers/glob/base64/UI parsers must advance and respect buffers.
  Missing slab indices are guarded; realloc helpers return live handles; builder
  growth cannot shrink below live children. Empty/nonfinite/deep JSON terminates.
  WinHTTP must not silently leak credentials across redirects or accept truncation.
- String bounds clamps are compiler/library safety properties, not interchangeable
  with unchecked array semantics. SDK keys are unsanitized heap strings: use
  environment configuration and redact dumps; WinHTTP transport is Windows-only.
- IDE/web advisories are in scope. Exceptions require reasons and exit conditions;
  do not drop the glib audit exception without resolving its dependency constraint.
- JS manifests carry no known advisory as of 2026-11-01: `ide` runs `next` 16.4.3
  and dompurify 3.4.17, `web/next-app` runs `next` 15.5.28. Re-check with
  `pnpm --dir ide audit` and `pnpm --dir web/next-app audit`. The one accepted
  exception stays `RUSTSEC-2024-0429` (glib, Linux-only webview backend, API
  never called), recorded with its 2026-11-01 re-check in
  `ide/src-tauri/audit.toml`.
- A bot-driven lockfile regeneration **can silently revert a security pin**:
  dependabot's `next` bump rewrote `ide/pnpm-lock.yaml` without the
  `pnpm.overrides` field in `ide/package.json`, dropping dompurify back to
  monaco's exact 3.4.15 while the manifest still described the pin. Overrides
  that carry a security fix live in `pnpm-workspace.yaml`, and reviewing a
  generated lockfile diff means checking its `overrides:` block, not only
  version lines.

### Repository checks and automation scope

Main CI names both compiler and package-manager packages explicitly; default
root Cargo testing does not imply member test execution. A separate Windows/Linux
step requires clang for the C parity and ABI targets, so their early-return skip
path cannot produce a green gate without the comparisons. Native command exit
codes gate Windows smoke/release steps, and installer downloads remain opt-in.
Local workflow checks are not proof of successful hosted Actions or installer
execution.

`tools/check_doc_links.py` is read-only and dependency-free. It reads local Git
history, never fetches URLs or executes documented commands, and defaults to
warning-only. Its explicit historical allowlist and command heuristics limit
coverage; it is not a security/link-completeness guarantee. The
[artifact audit](docs/repository-audit.md) documents provenance/retention rather
than deleting or certifying stored archives. No archive content was executed.

### Deliberate limitations and evidence

User-program signed overflow, unchecked array indexing, invalid raw-memory/FFI
use remain undefined behavior. `as_string` aliases memory that must remain live.
Strings are NUL-terminated bytes; concatenation leaks by design. There is no GC
or universal runtime memory safety. Corruption in libraries processing external
data is distinct from caller-authored unsafe operations. The window server and
registry trust tiers do not sandbox or certify programs.

The thirteen findings from the 2026-10-17 audit were fixed in 0.51.0/0.51.1;
that is history, not absence of new defects. Baseline **8f90aca** recorded
343 root + 104 package tests on Windows with explicit clang 23.1.2. Current-tree
validation passed **349 root + 105 package tests (454), 0 failed/ignored** on
Windows; the explicit required gate separately completed all four ABI and two
parity targets. Native ABI cases ran at O0/O3; original fixed point and
cross-process determinism passed. Linux Rust
workspace/tests cross-check passed, not Linux native runtime. An intermediate
wrong known-gap expectation failed and was corrected from direct C comparison;
see [verification evidence](docs/c-compatibility.md). No fresh security audit,
IDE validation, Linux native runtime or full installer release is implied.
The P2 maintenance follow-up reran the explicit locked compiler/package commands;
results and the Python checker tests are recorded in
[repository maintenance evidence](docs/repository-audit.md). Hosted CI was not
triggered; workflow syntax validation is local only.
That is no longer the Linux picture: the 2026-11-01 tarball-test fix was landed on
`main` and its hosted run passed on **both** jobs. Linux extraction containment is
now executed by CI, not only cross-checked, and the steps the red run had blocked
(`Smoke test`, X11 under Xvfb) ran again.

---

## 中文

### 支持版本与报告

编译器 **0.55.0**，支持系列 **0.55.x**；独立包管理器 crate **0.42.0**。
修复先落 main，旧小版本不维护；支持 Windows/Linux，不支持 macOS。
见 [CHANGELOG](CHANGELOG.md)。

漏洞先私报 **zmjsjsg3@163.com**，不要先发公开 issue。提供 commit/版本、系统架构、
aoxn doctor、最小输入、命令、预期/实际行为与影响。误编译附 aoxn c 与 O0 复现情况。
去除密钥/私有路径并注明署名意愿；可用无敏感内容邮件协商加密。

目标为 **3 个工作日确认、10 个工作日初评、确认后90天或修复发布两者较早时协调披露**。
严重问题安排紧急补丁/下一版本；提供进度与解释，署名须同意，无赏金。
主动利用时可能提前发布保护建议。

### 信任边界

编译/执行**不是沙箱**：导入读文件、资源命令写文件、clang 产出原生代码。
Rust 使用参数向量，自举驱动还使用 shell。对不可信项目使用系统/容器文件、网络和
凭据隔离；类型检查通过不代表可以安全执行。

src/ 无外部 Rust crate 依赖；包管理器独立依赖固定在 Cargo.lock，不依赖编译器库。
IDE/Web 的 JS/Tauri 为另一供应链边界。发射器/ABI 验证不是安全审计，也不提供沙箱。

### 编译器与构建不变量

- 特制源码/导入/资源/诊断不得损坏编译器、执行非预期命令或误编译定义良好程序。
  普通错误诊断/panic 是 bug；小输入灾难性资源放大值得私报。
- 生成标识来自稳定源码/计数器/名字，不能用地址；hash 迭代不应改变 C。
  缓存包含编译器、选项、依赖；勿共享不可信可写缓存。
- Rust-vs-selfhost 直接 C 比较和自举跨阶段不动点是不同检查；有限 fixture
  不证明全特性等价，自举仍缺新功能，见[C 边界](docs/c-compatibility.md)。
- 外部 ABI 依赖正确声明。Aoxn int 对应 long long，bool 对应 C int，不能推为
  任意 C int/_Bool 兼容；固定参数 extern 不支持 C 可变参数。错误声明可能链接
  成功却产生 UB；已知签名 C helper 测试不能校验任意第三方声明。
- 图优化保留语义与诊断顺序；反向边、借用模块表、无环证明复用只消除特定开销，
  不是消除所有 DoS。递归仍有宿主栈限制；数组等包装间接结构体环为既有缺口。
- 包优先安装 stdlib，裸导入先同目录，引号导入有同目录兜底；环境路径和 clang/
  链接选项是调用方配置，不对控制环境者构成安全边界。
- CSS 输出名由编译器生成并防路径逃逸；URL 只影响内容，Tailwind 不静默下载 Node。

### 包、安装器与应用边界

- checksum 锚定内容树，tarball_sha256 校验传输字节；旧索引缺摘要不豁免内容验证。
  热复用重算真实树，**只排除自身 index.ax shim**，不符则重建校验。
- 解包拒绝逃逸/绝对路径/链接，校验依赖和缓存路径；npm-import 验完整性与元数据。
  “绝对”由宿主定义（`C:/x` 在 Windows 是驱动器路径，在 POSIX 只是名为 `C:` 的
  目录），守卫跟随宿主是因为 `Path::join` 跟随宿主；与宿主无关的性质是**任何条目
  都不得写到目标目录之外**，两种拼法各有测试钉住。
  注册表 HTTP 为明文，恶意索引可同时替换数据和摘要；信任等级是声明而非审计。
- 并行 worker 各有 registry state；未知 schema 失败；生产剪枝保留生产可达包。
- 安装器限制在选定根、更新 PATH、自检后再确认二进制。默认不下载，InstallClang
  显式启用；系统隔离不是编译器漏洞，假报成功是 bug。
- IDE Rust 层限制工作区路径、防链接逃逸/覆盖、重建输出防硬链接，并验证命令。
  恶意 webview 不能取得任意 shell/全局命令能力。
- JSON/UTF/SSE/URL/头/glob/base64/UI 解析必须推进且不越界；缺失 slab 索引守卫、
  realloc 活句柄写回、builder 不缩到 live count 以下；空/非有限/深 JSON 终止。
  WinHTTP 不静默跨重定向泄密、不把截断当成功。
- 字符串 clamp 属安全属性，不得借数组不检查取消。SDK key 不清零，优先环境配置，
  转储脱敏；WinHTTP 为 Windows-only。IDE/Web 公告在范围内，例外需要理由与退出条件，
  未解决依赖约束前不得删除 glib audit 例外。
- 2026-11-01 起 JS 清单无已知公告：`ide` 为 `next` 16.4.3、dompurify 3.4.17
  （monaco 锁死精确版本，只能靠 override），`web/next-app` 为 `next`
  15.5.28。复核命令 `pnpm --dir ide audit`、`pnpm --dir web/next-app audit`。
  唯一保留的例外仍是 `RUSTSEC-2024-0429`（glib，仅 Linux webview 后端传递依赖，
  本项目不调用受影响 API），理由与退出条件连同本次复核写在
  `ide/src-tauri/audit.toml`。
- 机器人重新生成 lockfile **会静默回退安全 pin**：dependabot 升 `next` 那次重写
  `ide/pnpm-lock.yaml`，丢掉了 `ide/package.json` 里的 `pnpm.overrides`，
  dompurify 退回 monaco 锁定的 3.4.15，而清单正文仍在描述这个 pin。承载安全
  修复的 override 一律放在 `pnpm-workspace.yaml`；审查生成的 lockfile diff 要
  看 `overrides:` 段，不能只看版本号行。

### 仓库检查与自动化范围

主 CI 显式指定编译器和包管理器，不能把默认根包测试当成员测试已执行。
Windows/Linux 另有独立步骤强制 C parity/ABI 目标使用 clang，提前返回的跳过路径
不能让未实际比较的门禁变绿。Windows 冒烟/发布逐条检查原生命令退出码，
安装器下载仍为显式启用。本地 workflow 检查不证明托管 Actions 或安装器运行成功。

文档检查器只读、无外部依赖，仅读本地 Git 历史，不访问 URL 或执行文档命令，
默认仅警告。历史白名单与命令启发式限制其覆盖，不是安全/链接完整性保证。
[产物审计](docs/repository-audit.md)记录来源与保留决定，不删除或认证归档，
没有执行归档内容。

### 有意限制与证据

用户数组越界、带符号溢出、无效原始内存/FFI 是 UB。as_string 别名存储必须存活；
字符串 NUL 结束、拼接故意泄漏，没有 GC/通用运行时内存安全。库处理外部数据损坏
与调用方主动不安全操作有别；窗口服务和注册表等级不沙箱或认证程序。

2026-10-17 审计13项已在0.51.0/0.51.1修复，不保证没有新缺陷。
历史基线 **8f90aca** 在 Windows 显式 clang 23.1.2 记录343根+104包；当前树最终
**349根+105包=454项通过，0失败/忽略**，独立必跑门禁另完成4项 ABI 与2项 parity，
ABI 实际执行 O0/O3，不动点和跨进程确定性通过；Linux Rust workspace/tests
cross-check 通过，不等于原生运行。
中间 known-gap 错误预期曾失败，已据完整 C 比较纠正并最终全过，见
[C 兼容证据](docs/c-compatibility.md)。不暗示新安全审计、IDE 验证、Linux 原生运行
或完整安装器发布。P2维护后又执行显式locked编译器/包测试，结果与Python检查器
测试见[仓库维护证据](docs/repository-audit.md)。未触发托管CI，workflow仅本地语法校验。
2026-11-01 的 tarball 测试修复已落 main，其托管 CI **两个平台均通过**：Linux 解包
内含性由 CI 实测而不只是交叉编译校验，曾被红跑的 `Smoke test` 与 Xvfb X11 探测也
重新执行并通过。
