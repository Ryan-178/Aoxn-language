# Security policy

## English

### Versions and private reporting

The current compiler is **0.55.0**, supported on the **0.55.x** line; the
independent package-manager crate is **0.42.0**. Fixes land on main first;
earlier minor lines are not maintained. See [CHANGELOG.md](CHANGELOG.md).
Windows and Linux are supported; macOS is not.

Report vulnerabilities privately to **zmjsjsg3@163.com** before opening public
issues or disclosing reproducers. Include version/commit, OS/architecture,
`aoxn doctor` output, minimal input, exact command, expected/actual behavior
and impact. Miscompilation reports should include `aoxn c` output and whether
O0 reproduces it. Redact credentials/private paths. State your credit preference;
an initial nonsensitive email can arrange encryption.

Targets are acknowledgement within **3 business days**, initial assessment
within **10 business days**, and coordinated disclosure within **90 days of
acknowledgement or when a fix ships, whichever comes first**. Confirmed critical
issues target the next release or an urgent patch. Reporters receive progress,
an explanation of rejected reports, and a credit consent request. There is no
bounty. Active exploitation may require earlier public guidance.

### Trust model

Neither compiling nor executing Aoxn is a sandbox. Imports read files; assets
can write output; clang produces native executable code. The Rust compiler
uses argument vectors to invoke clang; the self-hosted driver also has a
shell-based path. Use OS/container isolation with restricted filesystem,
network and credentials for untrusted projects. Successful checking is not
permission to run untrusted native output.

The compiler library in `src/` has **zero external Rust crate dependencies**.
`crates/aoxn-pkg` has a separate dependency set pinned by `Cargo.lock` and does
not depend on the compiler library. IDE and benchmark applications have separate
JavaScript/Tauri supply-chain exposure. No external compiler dependency or
unsafe code was added by the struct-cycle performance change.

### In-scope invariants

- Crafted source, imports, manifests, CSS and diagnostics must not corrupt the
  compiler, run unintended commands, or miscompile otherwise defined programs.
  Ordinary wrong diagnostics/panics are bugs; small inputs causing catastrophic
  resource amplification also merit private reporting.
- Compiler graph optimizations retain semantics and diagnostic order. Exception
  propagation uses reverse edges; namespace rewriting borrows immutable tables.
  Struct-cycle checking reuses completed acyclic-node proofs across roots,
  retaining source-root order and path-local gray state. This removes a specific
  quadratic traversal, **not all denial-of-service opportunities**. Recursive
  walkers still have depth limits imposed by the host stack, and no universal
  source-size/time/memory bound is promised. The existing struct-cycle pass only
  follows direct `Type::Struct` fields; wrapped/array indirect cycles are a known
  checker limitation, not fixed or expanded by this optimization.
- Generated identifiers derive from stable source identities/counters/names,
  never heap addresses. Hash iteration must not alter emitted C. Build/run cache
  keys include dependencies, compiler identity and options; stale or substituted
  output reuse is in scope. Do not share writable caches with untrusted users.
- Project packages precede installed stdlib. Bare imports probe siblings first;
  quoted imports use sibling fallback after package probes. AOXN_HOME,
  AOXN_STDLIB, AOXN_CLANG, PATH and link flags are caller-controlled configuration,
  not security boundaries against a process controlling that environment.
- CSS emitted filenames are compiler-generated and checked against separators,
  `..` and absolute paths. Content may affect bytes, not escape output roots.
  URL assets are not executed; Tailwind support does not silently download Node.
- Package `checksum` pins the unpacked content tree; `tarball_sha256` checks
  served bytes when supplied by the HTTP backend. Missing legacy transport
  digests never waive content verification. Warm reuse re-hashes installed trees
  before keeping them, excluding **only the installer-owned index.ax shim**.
  Mismatches rebuild and re-verify. Broadening exclusions or unverified reuse
  is a vulnerability.
- Dependency/cache path components are validated. Extraction rejects escaping
  paths, absolute paths and links. npm-import checks npm integrity and Aoxn
  metadata. Read-only HTTP registries are **plain HTTP, not HTTPS**: no transport
  confidentiality/authentication is claimed; an untrusted index can replace
  both data and claimed hashes. Use trusted sources and appropriate transport.
- Registry trust tiers are claims, not independent audits. Missing/unreviewed/
  reviewed remain distinct, unknown schemas fail, and production-reachable
  dependencies survive prod pruning. Parallel workers own forked registry state.
- Windows installation stays within its chosen root, updates PATH and rechecks
  compiler presence after self-test. It downloads **nothing by default**;
  InstallClang is explicit opt-in. Windows quarantine is a deployment policy,
  not itself a compiler vulnerability; falsely reporting success is a defect.
- The IDE Rust boundary canonicalizes paths and enforces workspace containment,
  refuses creation clobbering and link escapes, recreates build outputs rather
  than writing through hard links, and validates package-command names/allowlists.
  A hostile webview must not invent arbitrary shell/global commands.
- Runtime parsers over hostile data (JSON, UTF encodings, SSE, URL/headers,
  glob/base64 and UI messages) must advance and respect buffers. Missing slab
  indices including -1 are guarded; reallocating helpers return live handles;
  builder growth never shrinks below live children. Empty/nonfinite/deep JSON
  inputs terminate. WinHTTP does not silently follow credential-bearing
  redirects or accept truncation as success. String index/slice bounds clamp;
  removing these guards is a vulnerability, not unchecked-array semantics.
- SDK keys are ordinary heap strings and are not scrubbed. Environment-based
  configuration is preferable; do not print clients and redact dumps/reports.
  SDK transport is Windows WinHTTP and does not run within the compiler.
- IDE/web dependency advisories are in scope. Accepted advisory exceptions need
  a reason and exit condition; the glib exception in ide/src-tauri/audit.toml
  must not be deleted without resolving the dependency constraint.

### Deliberate limitations and actual validation

User-program unchecked array indexing, signed overflow and invalid raw-memory/
FFI usage are undefined behavior. `as_string` aliases storage; callers must not
free that storage. Strings are NUL-terminated bytes; concatenation intentionally
leaks and no GC/general runtime memory safety is promised. User-authored unsafe
code is distinct from corruption in a library processing external data.
The window server is not a sandbox; message parsing remains in scope, while
pure visual clipping/cache issues are ordinary bugs. Registry tiers do not
certify code. The self-host fixed point proves products across stages, not
all Rust/Aoxn emitter feature equivalence.

The thirteen findings of the 2026-10-17 audit were fixed in 0.51.0/0.51.1;
that history is not a guarantee against new defects. This compiler performance
change is **not a security audit**. Final Windows validation with explicit
clang 23.1.2 passed **343 root + 104 package tests (447 total), 0 failed/ignored**;
Linux Rust workspace/tests cross-check also passed, not Linux linking/runtime.
An earlier attempt without clang had 202 passes/140 failures and is retained
in the evidence. Four new checker tests compare all three-node graphs against
an independent reference, including full first-root diagnostics, shared DAGs,
diamonds and cycles. Baseline/modified C, diagnostic bytes and statuses match
for 42 successful and 5 invalid inputs. See
[performance evidence](docs/p2-compiler-performance.md) for logs, measurements
and unresolved limits. IDE checks and a new full installer package were not
run; local Windows tests do not replace Linux CI.

---

## 中文

### 支持版本与私密报告

当前编译器 **0.55.0**，受支持系列 **0.55.x**，独立包管理器 crate **0.42.0**。
修复先落 main，更早小版本不维护；见 [CHANGELOG.md](CHANGELOG.md)。
支持 Windows/Linux，不支持 macOS。

漏洞请先私报 **zmjsjsg3@163.com**，修复前不要公开 issue 或复现。提供版本/
提交、系统架构、aoxn doctor、最小输入、完整命令、预期/实际结果和影响。
误编译附 aoxn c 及 O0 是否复现；清除密钥和私有路径，注明署名意愿。
可先发不含敏感内容的邮件协商加密。

目标为 **3 个工作日确认、10 个工作日初评、确认后90天或修复发布两者较早时
协调披露**。严重问题安排下一版本或紧急补丁；同步进度、解释拒绝原因、署名前
征得同意；没有赏金。主动利用时可能提前发布保护建议。

### 信任模型

编译和执行都不是沙箱：导入读文件、资源输出写文件、clang 产出原生程序。
Rust 使用参数向量调用 clang，自举驱动另有 shell 路径。处理不可信项目请用
系统/容器隔离并限制文件、网络与凭据权限；检查成功不代表原生输出可安全执行。

src/ 编译器库**无外部 Rust crate 依赖**；独立 aoxn-pkg 依赖由 Cargo.lock
固定且不依赖编译器库。IDE/Web 的 JS/Tauri 是另一供应链表面。本次结构体环
性能优化没有增加外部编译器依赖或 unsafe。

### 安全范围与不变量

- 特制源码/导入/清单/CSS/诊断不能损坏编译器内存、执行非预期命令或误编译
  定义良好程序。普通诊断/panic 是 bug；小输入引发灾难性资源放大也值得私报。
- 图优化保留语义和诊断顺序：异常走反向边、模块表只读借用；结构体检查跨根
  复用已完成无环证明，根按源码顺序、gray 仅活动路径。这去掉特定二次遍历，
  **不是所有 DoS 已消除**。递归仍受宿主栈限制，没有通用大小/时间/内存上限。
  原检查器只跟直接 Type::Struct 字段，数组等包装类型间接环是既有缺口，
  本次不扩大边定义或修语义。
- 生成标识只能来自稳定源码/计数器/名字，不可用堆地址；哈希顺序不能改 C。
  缓存键包含依赖/编译器/选项，陈旧或被替换输出复用在范围内；勿共享不可信可写缓存。
- 本地包优先安装 stdlib，裸导入先同目录，引号导入包失败才回退。AOXN_HOME/
  AOXN_STDLIB/AOXN_CLANG/PATH/链接参数是调用者配置，不是对环境控制者的安全边界。
- CSS 输出名由编译器生成并拒绝分隔符、..、绝对路径；内容不能逃出输出根。
  URL 不执行，Tailwind 不自动下载 Node。
- checksum 校验解包内容树，tarball_sha256 校验 HTTP 传输字节；旧索引缺后者
  不能跳过内容校验。热复用前重新哈希真实树，**仅排除自身 index.ax shim**，
  不匹配重建再验证。扩大排除项或未验证复用是漏洞。
- 依赖键/缓存路径组件必须验证，解包拒绝逃逸/绝对路径/链接。npm-import 验完整性
  与 Aoxn 元数据。HTTP 注册表是**明文 HTTP 非 HTTPS**，没有传输认证/保密性；
  恶意索引可同时改数据和摘要，需可信来源及合适传输。
- 信任等级是自述非审计，缺失/未审阅/已审阅有别，未知 schema 拒绝；生产可达
  包不得被 prod 裁掉；并行 worker 分别持有 fork 的 registry 状态。
- 安装器只在指定根解包，更新 PATH，自检后确认编译器仍在。**默认不下载**，
  InstallClang 显式启用；隔离策略本身不是编译器漏洞，假报成功是缺陷。
- IDE Rust 层规范化路径并限制工作区，拒绝创建覆盖/链接逃逸，重建输出避免写穿
  硬链接，在 Rust 校验包命令白名单/名称，恶意 webview 不能发明任意 shell/全局命令。
- JSON/UTF/SSE/URL/头/glob/base64/UI 等外部输入解析器必须推进且不越界。
  slab -1 等缺失索引守卫、realloc 返回活句柄、builder 不缩到存活子项以下；
  空/非有限/深层 JSON 要终止。WinHTTP 不自动携密钥重定向、不把截断当成功；
  字符串索引/切片必须保留越界空串防护，不能借数组不检查之名移除。
- SDK 密钥是普通堆字符串，不清零；优先环境配置，不打印 client，转储/报告先脱敏。
  SDK 使用 Windows WinHTTP，不在编译器进程运行。
- IDE/Web 依赖公告在范围内；接受风险须有原因/退出条件，未解决 glib 上游约束前
  不得删除 ide/src-tauri/audit.toml 例外。

### 有意限制与真实验证

用户程序数组越界、带符号溢出、无效原始内存/FFI 为未定义行为。
as_string 别名引用存储，调用方不能释放；字符串 NUL 结束，拼接结果故意泄漏，
没有 GC/通用运行时内存安全保证。用户主动不安全代码与库处理外部数据损坏有别。
窗口服务不是沙箱，消息解析仍在范围，纯显示问题是普通 bug。注册表等级不保证
代码安全；自举不动点不证明 Rust/Aoxn 全特性等价。

2026-10-17 审计的13项问题已在0.51.0/0.51.1修复，不等于无新问题。
本次性能修改**不是安全审计**。Windows 显式使用 clang 23.1.2 最终 **343根+
104包=447项通过，0失败/忽略**；Linux Rust workspace/tests cross-check 通过，
不等于 Linux 链接/运行。首轮缺 clang 的202通过/140失败保留记录。
新增4项检查器测试覆盖三节点全图参考比较、完整首根诊断、共享DAG/菱形/环。
基线和修改版42成功+5非法输入的 C/诊断/状态相同。日志与限制见
[性能证据](docs/p2-compiler-performance.md)。未运行 IDE 检查或新全量安装打包，
本地 Windows 不能代替 Linux CI。
