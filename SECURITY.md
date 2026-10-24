# Security policy

## English

### Supported versions and reporting

The current compiler is **0.55.0**, in the supported **0.55.x** line; fixes
land on `main` first. Earlier minor lines are not maintained. The independent
package-manager crate is **0.42.0**. Version history is in
[CHANGELOG.md](CHANGELOG.md). Windows and Linux are supported; macOS is not.

Report vulnerabilities privately to **zmjsjsg3@163.com**, not in a public
issue, chat or discussion before a fix is available. Include the compiler
version/commit, OS/architecture, clang version (`aoxn doctor`), smallest
reproducer, exact commands, expected/observed results and impact. For
miscompilation include `aoxn c file.ax` and whether `--O0` reproduces it.
Remove credentials and private paths before attaching logs or dumps. State
whether and how you wish to be credited; encryption can be arranged by an
initial email without sensitive details.

Targets: acknowledge within **3 business days**, initial assessment within
**10 business days**, and coordinated disclosure within **90 days of
acknowledgement or when a fix ships, whichever comes first**. Confirmed critical
issues target the next release or an urgent point release. We explain rejected
reports, keep reporters informed and request permission before naming them.
There is no bug bounty. Active exploitation may require earlier public guidance.

### Trust boundaries

Aoxn compiles source into native code; **neither compilation nor execution is
a sandbox**. The Rust compiler invokes clang using argument vectors; the
self-hosted driver also has a shell-based clang path. Compile and run untrusted
projects only within a suitable OS/container boundary with limited filesystem,
network and credential access. Imports read files and packages; compilation
with `--emit-assets` writes assets. Do not run untrusted native output merely
because the compiler accepted it.

The compiler library in `src/` has **zero external Rust dependencies**. The
separate `crates/aoxn-pkg/` dependency set is pinned by `Cargo.lock`; package
manager code does not depend on the compiler library. IDE and benchmark-app
JavaScript/Tauri dependencies are separate supply-chain surfaces, not covered
by the compiler library's zero-dependency claim.

### In scope and invariants

- **Compiler memory safety and defined-program miscompilation.** Crafted source,
  manifests, CSS, import paths or diagnostics must not corrupt memory, execute
  unintended commands or emit unsafe behavior for otherwise defined programs.
  Panics and wrong diagnostics are ordinary bugs unless they cross this boundary;
  small inputs causing catastrophic memory/time growth also merit private reports.
- **Compiler-time resource amplification.** Exception reachability now visits
  reverse call edges with a worklist instead of repeatedly scanning the entire
  graph; module rewriting borrows immutable member tables rather than copying
  the whole graph per module. No unsafe code, dependency or language changes
  were introduced. These remove particular repeated-work paths, **not every
  possible denial of service**. No general source-size/depth/time limit is
  claimed. Regression tests cover long chains, cycles and exception boundaries;
  performance results are not proof of complete adversarial robustness.
- **Reproducible emission and caches.** Generated identifiers must derive from
  source identities/counters/names, never heap addresses. Hash-map iteration must
  not change emitted C. `run`/`build` cache keys include source dependencies,
  compiler identity and options. Stale or attacker-substituted executable reuse
  is in scope; do not share writable cache directories with untrusted users.
- **Import and install discovery.** Project packages precede installed stdlib.
  Bare imports probe siblings first; quoted imports use sibling fallback only
  after package probes fail. `AOXN_HOME`, `AOXN_STDLIB`, `AOXN_CLANG`, PATH and
  explicit library flags are user-controlled configuration, not safe boundaries
  against another process already controlling that environment.
- **CSS asset writes.** Emitted names are compiler-generated and checked to reject
  separators, `..` and absolute paths. Stylesheet content may influence emitted
  bytes, never escape the requested output directory. URL assets are read, not
  executed. `--tailwind` is a built-in subset, not an automatic Node download.
- **Package integrity has two distinct digests.** `checksum` pins unpacked tree
  content in `aoxn.lock`; `tarball_sha256` pins served tarball bytes, checked by
  the HTTP backend when supplied. Missing legacy transport digests do **not**
  waive extracted content verification. Warm-install reuse re-hashes the actual
  existing tree before any keep decision, excluding **only `index.ax`**, the
  installer-owned loader shim. Mismatches rebuild and re-verify; a broader hash
  exclusion or unverified keep path is a vulnerability.
- **Archive/path confinement.** Dependency keys and cache checksum components
  are validated before filesystem use. Extraction rejects escape components,
  absolute paths and archive links; a tarball must not write outside its root.
  npm imports verify npm integrity and require Aoxn package metadata. HTTP
  registries are read-only **plain HTTP**, not HTTPS: there is no transport
  authentication/confidentiality, and an untrusted index can replace both bytes
  and advertised checksums. Use a trusted source/index and appropriate transport.
- **Package trust and filtering.** `trust.json` is a registry's claim, not an
  independent audit. Missing/unreviewed/reviewed tiers stay distinct; unknown
  schemas fail. Production-reachable packages must survive `--prod` pruning.
  Parallel registry workers own independent `Registry::fork()` state.
- **Installer.** The Windows installer unpacks only within its chosen root,
  installs the packaged payload, updates PATH and rechecks that the compiler
  remains present after self-test. It downloads **nothing by default**;
  `-InstallClang` explicitly opts in. Unsigned artifacts can be quarantined by
  Windows policy; that policy is not a compiler vulnerability, but reporting a
  successful install while leaving an unusable executable is a defect.
- **IDE filesystem/command boundary.** Rust canonicalizes paths and enforces
  containment within the opened workspace; creation refuses clobbering, folder
  selection uses a native dialog, and explorer links cannot escape the boundary.
  Build destinations are recreated rather than overwriting through hard links.
  The package command allowlist is enforced in Rust, including validation of
  names; a compromised webview must not invent arbitrary shell/global commands.
- **Runtime library parsers over hostile input.** JSON slabs/builders, UTF-8 and
  UTF-16 conversion, SSE framing, URL/header parsing, filesystem conversion,
  glob/base64 and UI message parsing must not overrun buffers or stop advancing.
  Slab indices including `-1` are guarded, reallocating callees must return the
  live handle, builder growth must not shrink below live child count, empty
  sinks must remain valid and nonfinite/deep JSON must terminate. WinHTTP does
  not silently follow credential-bearing redirects or accept truncated bodies
  as clean success. String indexing/slicing helpers clamp out-of-range accesses;
  removing those guards is a vulnerability, not an unchecked-array feature.
- **Credential exposure.** SDK keys are ordinary heap strings and are not
  scrubbed; process dumps and printing client structs can disclose them. Prefer
  environment configuration, avoid dumping clients, and redact reports. The
  SDK transport is Windows WinHTTP; SDKs do not run inside the compiler process.
- **Dependency advisories.** IDE and benchmark dependencies are in scope. An
  accepted advisory must remain recorded with a reason and exit condition;
  `ide/src-tauri/audit.toml` records the glib upstream-version constraint.
  Do not remove an exception without resolving its underlying dependency pin.

### Deliberate limitations and validation

Unchecked array indexing, signed-integer overflow and invalid raw-memory/FFI
use are undefined behavior in the **user program**. `as_string` aliases its
buffer rather than copying it; callers must not free that storage. Strings
are NUL-terminated bytes, concatenation results intentionally leak, and there
is no GC or general runtime memory-safety promise. User-authored unsafe code
is distinct from compiler/library corruption while processing hostile data.

The X11/Win32 window server is not a sandbox boundary; defects in parsing its
messages remain in scope. UI clipping/cache rendering differences alone are
ordinary correctness bugs. Registry tier claims do not certify safe code.
The self-hosted compiler supports a subset of the Rust implementation; its
fixed point does not prove equivalence of every Rust/Aoxn emitter feature.

The 2026-10-17 audit's thirteen findings were fixed in 0.51.0/0.51.1 (SDK
redirect/encoding/JSON/framing, archive/path/cache integrity and IDE workspace
issues). This is historical scope, not a guarantee that no other issues exist.
For this performance change, root and package test results, byte-for-byte output
comparisons and measurement limitations are recorded in
[docs/p2-compiler-performance.md](docs/p2-compiler-performance.md). Those checks
are correctness evidence, **not a new security audit**. Linux CI and IDE
validation remain separate from the local Windows compiler checks.

---

## 中文

### 支持版本与报告渠道

当前编译器 **0.55.0** 属于受支持的 **0.55.x** 系列，修复先落在 `main`；
更早的小版本不维护。独立包管理器 crate 版本为 **0.42.0**，历史见
[CHANGELOG.md](CHANGELOG.md)。支持 Windows 和 Linux，不支持 macOS。

请通过 **zmjsjsg3@163.com** 私下报告漏洞，修复前不要公开到 issue、聊天或讨论。
提供版本/提交、系统与架构、`aoxn doctor` 中的 clang 版本、最小复现、完整命令、
预期/实际结果与影响。误编译请附 `aoxn c file.ax`，说明 `--O0` 是否复现。
附件先去掉密钥和私有路径，并注明署名意愿；需要加密可先发不含细节的邮件协商。

目标为 **3 个工作日确认、10 个工作日初步评估**；协调披露期限为
**确认后 90 天或修复发布，以较早者为准**。严重问题目标为下一版本或紧急补丁。
我们会解释不接受的报告、同步进度并在署名前征求同意；没有漏洞赏金。
存在主动利用时，可能需要更早发布保护用户的说明。

### 信任边界

Aoxn 将源码编译为原生代码，**编译与执行都不是沙箱**。Rust 路径使用参数向量
启动 clang，自举驱动还有 shell 调用路径。处理不可信项目应使用合适的系统/容器
隔离，限制文件、网络和凭据权限；导入会读取文件，`--emit-assets` 会写文件。
编译成功不意味着可以安全执行输出程序。

`src/` 编译器库**不依赖外部 Rust crate**；独立 `crates/aoxn-pkg/` 依赖由
`Cargo.lock` 固定，包管理器不依赖编译器库。IDE 和 Web 对比应用的 JS/Tauri
依赖是另一供应链表面，不属于“编译器库零依赖”的承诺。

### 范围与必须保留的不变量

- **编译器内存安全、合法程序误编译**：源码、清单、CSS、导入路径或诊断不得
  引起内存损坏、非预期命令执行，或把本来定义良好的程序变成不安全代码。
  一般 panic/错误文案属于普通 bug；小输入造成灾难性耗时/内存增长也值得私报。
- **编译耗时放大**：异常可达性改为反向边队列，模块重写借用只读成员表，去除
  特定反复全图工作。没有添加 unsafe、外部依赖或语言变化；这**不代表全部 DoS
  已消除**，没有承诺通用输入大小/递归深度/时间限制。长链、环和异常边界回归
  证明特定行为，性能数字不是完整对抗性安全证明。
- **确定性与缓存**：生成名称只能来自源码标识、计数器或名称，不得来自堆地址；
  HashMap 顺序不得改变 C。缓存键包含依赖内容、编译器与选项；陈旧或被替换的
  可执行文件复用在漏洞范围内，不应与不可信用户共享可写缓存。
- **导入与工具链**：本地包优先于安装标准库；裸导入先同目录，引号导入只有包
  探测失败后才回退同目录。`AOXN_HOME`、`AOXN_STDLIB`、`AOXN_CLANG`、PATH 与链接
  选项属于用户配置，不是对能控制当前环境的其他进程的安全边界。
- **CSS 输出**：输出名由编译器生成，并拒绝分隔符、`..`、绝对路径。样式表可
  改内容，不能逃出目标目录；URL 资源只读不执行，`--tailwind` 不自动下载 Node。
- **两个摘要不可混淆**：`checksum` 固定锁文件中的解包内容树，`tarball_sha256`
  固定传输字节，仅 HTTP 后端在提供时检查。旧索引缺传输摘要不能跳过内容校验。
  热安装保留前必须重新计算已安装目录哈希，**仅排除安装器生成的 `index.ax`**；
  不匹配应重建并验证，扩宽排除项或未经校验直接复用都是漏洞。
- **解包与路径**：依赖键、缓存 checksum 路径片段在文件操作前验证。归档拒绝
  逃逸分量、绝对路径和链接，不得写出根目录；npm 导入检查完整性且要求 Aoxn
  元数据。HTTP 镜像是只读**明文 HTTP**，不是 HTTPS，没有传输认证/保密性；
  不可信索引可以同时替换数据与声称的摘要，必须信任来源并选合适的传输。
- **信任等级与过滤**：`trust.json` 是注册表自述，不是独立审计。缺失、未审阅、
  已审阅必须区分，未知 schema 拒绝；生产可达依赖不能被 `--prod` 裁掉。
  并行下载 worker 分别持有 `Registry::fork()` 状态。
- **安装器**：仅在选定根目录内解包打包时的负载，更新 PATH，自检后再次确认
  编译器仍可用。**默认不下载**，`-InstallClang` 才显式启用。Windows 隔离未签名
  文件是部署策略，不是语言漏洞；安装器假报成功却留下不可用编译器则是缺陷。
- **IDE 边界**：Rust 规范化路径并检查是否在打开的工作区内，创建拒绝覆盖，
  文件夹由原生对话框选择，浏览器树不能通过链接逃出工作区，构建目标先重新创建
  防止硬链接覆盖。包命令白名单和名称验证在 Rust 中执行，不能由受攻击 webview
  发明全局命令或任意 shell。
- **库解析器**：JSON slab/builder、UTF 编码、SSE、URL/头、目录、glob/base64、
  UI 消息解析不得越界或不前进。缺失索引 `-1` 要防护；realloc 后用返回的新句柄；
  builder 容量不能缩到存活元素以下；空 sink、非有限浮点与深层 JSON 必须正确终止。
  WinHTTP 不自动携带凭据重定向，不把截断响应当成功；字符串索引/切片必须保留越界
  空串防护，不能以“数组不检查”为由删除。SDK 不运行在编译器进程中。
- **凭据**：SDK 密钥是普通堆字符串，不清零；转储或打印 client 结构会泄露。
  优先环境配置，不打印 client，报告先脱敏；其传输是 Windows WinHTTP。
- **依赖公告**：IDE/Web 应用依赖漏洞属于本仓库范围；接受的风险必须记录原因与
  退出条件。`ide/src-tauri/audit.toml` 记录 glib 的上游版本约束，未解决依赖前
  不得直接删掉例外。

### 有意限制与验证

数组越界、带符号溢出、无效原始内存/FFI 操作为**用户程序**的未定义行为。
`as_string` 别名引用缓冲区而非复制，调用方不可释放对应存储。字符串为 NUL
结束的字节，拼接结果故意不释放，没有 GC，也不承诺普遍的运行时内存安全。
用户主动写的不安全代码与编译器/库处理外部数据时损坏内存是两回事。

X11/Win32 窗口服务不是沙箱边界，但消息解析缺陷仍在范围内；纯裁剪/缓存显示
差异是普通正确性问题。注册表信任等级不保证源码安全。自举只实现 Rust 的子集，
不动点不能证明两个发射器所有特性等价。

2026-10-17 审计的十三项问题已在 0.51.0/0.51.1 修复，涉及 SDK 重定向/编码/
JSON/响应分帧、归档路径缓存完整性和 IDE 工作区。这是历史状态，不是无其他漏洞
的保证。本次性能修改的测试结果、逐字节输出对比与测量限制见
[编译器性能报告](docs/p2-compiler-performance.md)；这些是正确性证据，**不是新安全审计**。
Linux CI 与 IDE 验证仍独立于本地 Windows 编译器检查。
