# 调查报告：Aoxn 能否摆脱 LLVM 依赖

> **历史边界与当前入口**：§0–§6 是 v0.27.0 时点的调查和方案，§7 保留
> v0.27.1 原型实测及 v0.29.0 迁移记录；其中行号、测试总数、旧文件、
> 环境变量和平台矩阵均属于对应版本，不是当前操作指南。L1 已完成，
> L2/L3 未完成：当前仍依赖 clang 编译/链接，不能称为完全脱离 LLVM 家族工具。
> 当前实现见 [`src/codegen_c.rs`](../src/codegen_c.rs)、
> [`selfhost/codegen.ax`](../selfhost/codegen.ax) 和
> [`selfhost/driver.ax`](../selfhost/driver.ax)；ABI 与已知差异见
> [`c-compatibility.md`](c-compatibility.md)。当前平台与 CI 是 Windows/Linux，
> macOS 不受支持，详见 [`platform-support.md`](platform-support.md) 和
> [CI 配置](../.github/workflows/ci.yml)。wiki 自 v0.29.3 起冻结，
> §5 当年的 wiki 同步计划不再适用。

| | |
|---|---|
| 日期 | 2026-09-30 |
| 基线 | v0.27.0（commit `95742cf`，127/127 测试） |
| 调查范围 | `src/llvm.rs`、`src/codegen.rs`、`build.rs`、`src/lib.rs`、`src/main.rs`、`selfhost/*.ax`、`tests/`、`docs/optimization-report.md`、`docs/spec.md`；第三方后端（Cranelift / QBE / MIR / libgccjit）按 2026-09 时点的公开资料核对 |
| 性质 | 技术调研，未改变任何行为；结论供"后端路线"拍板 |

## 0. 结论（TL;DR）

**可以，而且比表面看起来容易——但只有一条推荐路径。**

1. LLVM 在本项目只做三件事：IR 构建、优化（`default<Ox>` 管线）、目标码发射。Rust 侧的全部接触面是
   `src/llvm.rs`（177 行，86 个 C API 函数声明）+ `src/codegen.rs`（1,896 行，唯一消费者）+ `build.rs`
   （149 行安装探测与 DLL 复制），合计约占编译器 Rust 源码的 28%；自研编译器侧是 `selfhost/codegen.ax`
   （2,027 行，82 个 `extern def` 独立再声明同一 C API）。前端（lexer / parser / typecheck / TS 前端）与
   LLVM 零接触——合计约 10,000 行代码今天就不依赖它。
2. 使用的 LLVM 能力子集**非常窄**：标量类型 + 命名结构体 + 定长数组，约 45 个指令构建调用；没有向量
   IR 类型、没有异常处理、没有元数据、没有内联汇编。这个子集与 C 语言构造**几乎一一对应**（`nsw` = C
   有符号溢出 UB、`inbounds` GEP = C 指针运算 UB、值语义 memcpy = C 结构体赋值、短路 phi = `&&`/`||`）。
3. 因此**C 代码发射后端**（emitting C，交给现有的 clang 编译链接）是现实替代路线：1–3 周量级、零新增
   依赖（clang 本来就是链接的硬依赖）、"与 clang -O3 平价"的承诺在字面上依然可测（产物就是经 clang
   -O3 优化的原生码）。Cranelift、QBE、MIR、libgccjit 均因零依赖政策 / Windows Tier-1 / 支持面问题被
   排除；自研机器码后端是 3–6 个月起的长期选项，且要重写性能承诺。
4. 注意：**clang 链接是独立的第二个依赖**，与后端选择正交（两台编译器今天都 `system("clang ...")`）。
   "去 LLVM-C 库"（本文称 L1）与"去 clang"（L2）是两个决策；`docs/optimization-report.md` 已实测直连
   lld-link 无速度收益，L2 只有依赖纯度收益。

## 1. LLVM 现在到底用在哪里（实测）

### 1.1 代码接触面

| 文件 | 行数 | 角色 |
|---|---|---|
| `src/llvm.rs` | 177 | 全部 FFI：7 个 `extern "C"` 块、86 个函数声明；文件内唯一 LLVM 符号 104 个（含类型别名） |
| `src/codegen.rs` | 1,896 | 唯一消费者（`use crate::llvm::*`）：AST → LLVM IR |
| `build.rs` | 149 | 探测 LLVM 安装（`AOXN_LLVM_DIR` → `<repo>/LLVM` → 平台默认）、链接 `LLVM-C`、把 `LLVM-C.dll` 复制到每个 target 目录 |
| `selfhost/codegen.ax` | 2,027 | 自研编译器用 82 个 `extern def` 独立再声明同一 C API |
| `selfhost/driver.ax` | 93 | `system("clang ...")` 链接（与 Rust 编译器同路） |

占比（v0.27.0 实测行数）：Rust 侧 `src/` 7,754 行 + `build.rs` 149 行，其中 LLVM 相关 2,222 行（**28%**）；
前端与 CLI（lexer / parser / typecheck / ast / TS 前端 / lib / main / platform / files / hashing）5,681 行与
LLVM 无关。Aoxn 侧 `selfhost/` 6,347 行，其中 codegen 2,027 行（**约三分之一**）。两侧合计 14,250 行，
与 LLVM 无关的部分约 10,000 行。

### 1.2 使用的 LLVM 能力子集（窄——这是整个调查的关键事实）

- **类型**：`i1/i8/i32/i64/f64`、指针、定长数组、两阶段命名结构体（`LLVMStructCreateNamed` +
  `SetBody`）、函数类型。仅此而已。
- **指令**（约 40 个 `LLVMBuild*` 调用）：`nsw` 算术、`icmp`/`fcmp`、`inbounds` GEP、alloca/load/store、
  memcpy、call、br/condbr/ret、phi（仅 `&&`/`||` 短路与 `str(bool)` 分支）、select、zext/sext/trunc/
  inttoptr/ptrtoint、全局字符串。
- **没有用**：向量/SIMD IR 类型（`--cpu native` 走 TargetMachine CPU 特性；且 v0.26.3 实测 C API 不解析
  `"native"`，要传真实 CPU 名如 `skylake`）、异常处理、元数据、内联汇编、原子、地址空间、varargs、
  bitcode、JIT、debug info。
- **优化**：`LLVMRunPasses` + New Pass Builder 字符串 `default<O1/O2/O3>`（`codegen.rs:91-93`）；`--O0`
  = 无 IR 管线 + `CodeGenLevelNone`（fast-isel）。`AOXN_PASSES` 可覆盖管线文本。
- **发射**：`LLVMTargetMachineEmitToFile` → COFF/ELF/Mach-O 目标文件；目标注册仅 X86 + AArch64。
- **命令面依赖**：`aoxn ir`（`LLVMPrintModuleToString`）、`--O0..--O3`、`AOXN_PASSES`、`AOXN_DUMP_IR`、
  `--cpu`。
- **测试依赖**：自举固定点 `selfhost_driver_self_compiles` 自 v0.24 起逐字节比较两侧 IR，v0.25 起比较两
  侧 COFF 对象——强依赖 LLVM 发射的确定性。

### 1.3 第二个依赖：clang（与后端选择正交）

- 两个编译器都在最终链接时 spawn clang：Rust 侧 `src/lib.rs:371`（`AOXN_CLANG` → PATH → repo
  `LLVM\bin` → `C:\Program Files\LLVM\bin`），自研侧 `selfhost/driver.ax:59`。
- Windows 上 clang 再驱动 lld-link + MSVC CRT；用户程序调用的 `malloc/snprintf/_setmode/strcmp` 都来自
  CRT。
- `docs/optimization-report.md` 实测：链接均值 471ms/次；clang 驱动每次 ~136ms 做 MSVC 检测；**直连
  lld-link 255ms vs 经 clang 234ms，在噪声内打平，无净收益**；clang 23 默认链接器已是 lld-link。
- 含义：即使完全移除 LLVM-C 库，clang（或某种 C 工具链）仍是硬依赖。"去 LLVM-C"与"去 clang"必须
  分开决策。

### 1.4 CI 与分发面

- CI：Windows Tier-1 用 winget 装 LLVM；Tier-2（Linux/macOS）装 LLVM 18 并设 `AOXN_LLVM_DIR`。
- 分发：`LLVM-C.dll` 必须与编译器同行（build.rs 负责复制）；LLVM 安装本身 GB 级。
- 版本耦合史：FFI 按 LLVM 18 C API 编写、在 23 上未改即通过；`LLVMBuilderCreate` 缺符号改用
  `LLVMCreateBuilderInContext` 等坑均有记录——兼容性维护成本真实存在但至今可控。

## 2. "不依赖 LLVM"的三种含义

| 层次 | 内容 | 现状 | 代价 |
|---|---|---|---|
| **L1** | 不依赖 LLVM-C 库（IR 构建 / 优化 / 发射） | 未做 | 换后端（见 §3.1），1–3 周量级 |
| **L2** | 不依赖 clang 驱动（链接） | 未做 | 直连 lld-link / link.exe / cc；实测**无速度收益**（优化报告），只有依赖纯度收益。注意 lld-link 本身也是 LLVM 项目代码——严格"无 LLVM"在 Windows 上只剩 link.exe（MSVC）或 MinGW gcc |
| **L3** | 完全自包含（自研机器码后端 + 自写 COFF/ELF/PE + 微型 CRT，不依赖任何外部工具） | 未做 | tcc / LuaJIT 级工程，3–6 个月起（O1 级质量），O3 平价不保 |

多数"想摆脱 LLVM"的动机——安装摩擦、DLL 伴随、CI 时间、GB 级安装体积——**L1 就全部解决**；L2/L3
是独立得多的决策，建议分开拍板。

## 3. 候选替代方案评估

### 3.1 C 代码发射后端 ★推荐（达成 L1）

**做法**：`codegen.rs` 改为输出 C 源文本（或新增 `--backend=c`），交给现有的 clang 编译链接；
`selfhost/codegen.ax` 同步改为发射 C 文本。映射关系几乎逐条成立：

| 现有 LLVM-C 用法 | C 发射对应物 |
|---|---|
| Context / Module / Builder 生命周期 | 无对应物——直接拼文本 |
| `i1/i8/i32/i64/f64`、数组、命名结构体 | `bool/char/int/long long/double`、`T[N]`、`struct` |
| `LLVMBuildAlloca`（入口提升的临时槽） | 局部变量（C 编译器自己做 mem2reg/SROA） |
| `nsw` 算术 | C 有符号算术——溢出同为 UB（spec 语义本就按 C 制定） |
| `inbounds GEP`（结构体 i32 / 数组 i64） | `p->field` / `a[i]`——越界同为 UB |
| `LLVMBuildMemCpy`（值语义复制） | 结构体/数组赋值（C 原生值语义）或 `memcpy` |
| 短路 phi / select | `&&`/`||` / `? :` |
| 全局字符串 + malloc 拼接（malloc+两次 memcpy+NUL） | 字符串字面量；同一段 C 运行时调用 |
| `get_extern` 惰性声明（malloc/snprintf/strcmp/…） | `extern` 声明 |
| 聚合 ABI（指针传参 + sret 隐式首参） | 交给 C 编译器决定（内部 ABI 而已；`extern def` 本来就是 C ABI） |
| `main` 包装 + 用户 `main` 改名 `aoxn.main` | 用户 `main` 改名 `aoxn_main`，生成真 `main`（`_setmode` 原样保留） |
| `LLVMRunPasses default<O3>` | `clang -O3`（O1/O2/O0 同理；`--cpu` → `-march=native` / `-mcpu=<name>`） |
| `TargetMachineEmitToFile` → .obj | C 编译器产出 .obj/.o——链接路径完全不变 |
| `LLVMVerifyModule` | 不需要——C 编译器就是验证者 |
| `aoxn ir`（`LLVMPrintModuleToString`） | 新 `aoxn c`（输出生成的 C 源码） |

**为什么可行**：现有 codegen 纪律（聚合一律以地址表示 + 显式 memcpy + 入口提升槽）本来就是"C 形状"
的——它当初是为了绕开 LLVM 优化器对大聚合 SSA 值的爆炸而设计的，而这恰好就是合法、寻常的 C 写法。
语义层没有发明任何 C 表达不了的东西：`nsw` 与 `inbounds` 的 UB 语义直接来自 C。唯一要写进实现纪律的
是原始内存内建（`load_i64` / `store_u8` 等）在 C 里必须经 `memcpy` 实现，以避开对齐/严格别名 UB。

**工程量估计**：新后端约 1.2–1.8k 行 Rust（对照 codegen.rs 的 1,896 行）；`build.rs` 的 LLVM 探测与 DLL
复制整段删除；CI 不再装 LLVM。自举侧 `codegen.ax` 从"82 个 extern def 的指针舞蹈"变成纯字符串拼装
——比现状**更简单**；固定点对比对象从"IR + COFF 字节"改为"生成的 C 文本字节"（文本发射天然确定，
更容易），编译产物照旧比对 stdout / 退出码。

**性能预期**：产物是经 `clang -O3` 优化的原生码——`docs/spec.md:343` 的承诺（对照 `examples/bench_*.ax`
实测）在字面上保持可测。当前 IR 同样是"显式 alloca + memcpy"风格、靠 O3 清洗，clang 对同风格 C 做同
样的 SROA / 内联 / 向量化；理论上有得有失（C 前端丢失部分 IR 级信息、但 C 层别名/类型信息更丰富），
**必须重测**，不能假设。

**主要风险**：端到端编译时间——clang 的 C 前端比直接喂 IR 多一遍词法/解析/C 语义分析，7k 行自举输入
展开成的 C 可能明显变大。缓解：exe 内容哈希缓存（v0.26.3）对未变更输入仍是零成本；`--O1`/`--O0` 映射
clang 旗标；开发迭代可另配 tcc。

**判定**：唯一同时满足"零新增依赖 + Windows Tier-1 + 性能承诺可保留 + 自举可跟进"的方案。

### 3.2 Cranelift — 否决（零依赖政策硬冲突）

Bytecode Alliance 维护、rustc 的 cg_clif 实验后端；编译速度显著快于 LLVM，但运行时性能低于 LLVM
O2/O3（2025 年 TPDE 论文等公开对比的语境；cg_clif 至今未成为 rustc 默认）。`cranelift-object` 经
`object` crate 可写 ELF/Mach-O/COFF（COFF 上的 TLS 已支持）。**否决理由**：`Cargo.toml` 没有
`[dependencies]` 节是项目身份（全源可读的 AI 原生卖点）；引入 cranelift-codegen + object + 寄存器分配
器等数 MB vendored Rust 直接违背它。若政策豁免，集成约 2–4 周，且 `selfhost/codegen.ax` 要再写一套。

### 3.3 QBE — 否决（Windows Tier-1 不可行）

官方目标仅 amd64_sysv（Linux/macOS）、arm64、riscv64，**没有 Win64 MSVC ABI**；Hare 语言用它，也正
因此没有一等 Windows 支持。本项目开发机与 Tier-1 都是 Windows + MSVC 工具链——直接出局。此外还要分
发 qbe 这个外部二进制（与 clang 同级的外部依赖）。官方自述生成代码约为 GCC 的 70% 性能。

### 3.4 libgccjit — 否决

GCC 侧分发在 Windows 上缺位（MinGW-only）、GPL 生态与 Apache-2.0 项目混杂、API 面向 JIT 场景。对本
项目无收益场景。

### 3.5 MIR — 现状否决

Vladimir Makarov（Red Hat）的轻量 C 库 JIT/AOT；x86-64 / AArch64 / s390x，主平台 Linux/macOS，
**Windows 无完整支持**（2026-09 检索）。同为 C 库需要构建/携带，与零依赖政策冲突。

### 3.6 自研机器码后端 — 长期可选（达成 L3）

有利面：语言子集极小（六种标量 + 聚合，无闭包 / GC / 异常，纯 C ABI，泛型单态化在 typecheck 已完
成），指令选择与线性扫描寄存器分配都在可控范围；直发 COFF/ELF/Mach-O（乃至直接发 PE/ELF 可执行文
件）可以连 clang 一起去掉。工程现实：O1 级质量 3–6 个月起；**clang -O3 平价不可保**（向量化 / 内联 /
指令调度的差距是结构性的）——性能承诺必须改写。参照系：Zig 的自研后端走了多年仍在路上；Go 是"终
点形态"但成本量级完全不同。定位：远期独立拍板，不阻塞 L1。

### 3.7 解释器 / VM — 排除

改变"编译为原生码"的产品承诺（README 首条卖点），与调查目标错位。

## 4. 决策矩阵

| 方案 | 去 LLVM-C | 去 clang | O3 平价承诺 | Windows T1 | 零依赖政策 | 自举连带 | 工程量 | 判定 |
|---|---|---|---|---|---|---|---|---|
| **C 发射后端** | ✅ | ✗（保留） | 大概率可保（产物即 clang -O3 原生码），需重测 | ✅ | ✅ 零新增 | codegen.ax 改发射 C（变简单） | 1–3 周 | **推荐** |
| Cranelift | ✅ | ✗ | 低于 O2/O3 | ✅（object 写 COFF） | ❌ 数 MB crate | 再写一套 | 2–4 周 + vendoring | 政策冲突 |
| QBE | ✅ | ✗（还多一个 qbe） | ~70% GCC | ❌ 无 MSVC ABI | 外部二进制 | 再写一套 | 1–2 周 | 否决 |
| libgccjit | ✅ | 部分 | GCC 级 | ❌ 分发缺位 | 外部库 | — | — | 否决 |
| MIR | ✅ | 部分 | ~O1/O2 | ❌ 无 Windows | C 库构建 | 再写一套 | 数周 | 否决 |
| 自研后端 | ✅ | ✅（可直发 obj/exe） | ❌ 需降级为 O1 级 | ✅（自写 COFF/PE） | ✅ | 全重写 | 3–6 个月起 | 远期可选 |
| 解释器 / VM | ✅ | ✅ | N/A | ✅ | ✅ | — | 数周 | 改变产品定位 |

## 5. 推荐路线（分阶段）

**Phase 1（1–3 周）— `--backend=c` 实验**（**已于 2026-09-30 完成并落地为 v0.27.1，
实测结果见 §7**）：
1. 最小原型先行：用最小 emitter 编译 `examples/fib.ax` + `stdlib_demo.ax`，产物与现有 LLVM 后端逐 case
   对比 stdout / 退出码。500 行内的原型即可证实或证伪本报告的核心判断（映射可行性 + 性能量级）。
2. 补齐全语言面：struct / array / string / float / 已单态化的泛型、原始内存内建（经 `memcpy`）、f-string
   （沿用脱糖链）。
3. 验收：127 个端到端测试在新后端全绿；`examples/bench_*.ax` 与 web benchmark 对现有后端不劣化超过约
   定阈值；7k 行自举输入的端到端编译时间记录在案。

**Phase 2（可选，Phase 1 达标后）— 转默认、去 LLVM-C**：C 后端设为默认；`build.rs` 删除 LLVM 探测 /
DLL 复制；CI 停装 LLVM；`aoxn ir` → `aoxn c`；`selfhost/codegen.ax` 跟进，固定点改为 C 文本逐字节对
比。文档连带：`docs/spec.md`、wiki 的 `CLI-and-Tooling` / `Codegen-and-LLVM-FFI` / `Testing-and-CI` /
`Platform-Support` / `Performance-and-Benchmarks`。性能承诺措辞不需要降级——"与 clang -O3 平价"依旧
字面成立（优化者从 LLVM 管线换成 clang 驱动的同一优化器），但要以重测数据背书。

**Phase 3（远期，独立拍板）**：L2 链接直连（实测无速度收益，仅依赖纯度收益）与 L3 自研后端。仅当
"完全零外部工具链"被定为项目目标才启动。

**明确不建议**：引入 Cranelift（政策冲突）；换 QBE / MIR（Windows 不可行）；为换后端而先造"多后端抽
象层"——Phase 1 的答案会决定要不要这层抽象。

## 6. 风险与未决问题

1. ~~**本报告未含原型实测**~~ ——**已解决（2026-09-30）**：Phase 1 原型落地为 v0.27.1 的
   `src/codegen_c.rs`（约 900 行），实测 C 前端没有带来显著编译时间回归（7k 行输入端到端 +6%），
   详见 §7。
2. clang 依赖不随 L1 消失（§1.3）；"彻底无 LLVM 家族"是 L2+ 的事，且 Windows 上 lld-link 也算 LLVM 家
   族二进制。
3. 严格别名 / 对齐：生成的 C 中原始内存内建必须走 `memcpy`；这条纪律要写进 codegen invariants 文档并
   在代码评审中执行。
4. `aoxn ir` 是 CI / 调试面的一部分（`AOXN_DUMP_IR`、优化报告的管线实验都靠它）；C 后端下这些实验手
   段要重新设计（clang 的 `-emit-llvm` / `-mllvm` 可部分替代）。
5. 固定点测试重构（方向是变简单，但要做）；`tests/pipeline.rs` 里的 LLVM 安装探测（`llvm_dir()`）随之退
   役，Tier-2 CI 的 `AOXN_LLVM_DIR` 设置同步移除。

## 7. Phase 1 实测结果（2026-09-30，v0.27.1）

Phase 1 已完整落地并合入（v0.27.1）：`src/codegen_c.rs`（约 900 行）+ `--backend c` /
`AOXN_BACKEND=c` + 跨后端测试 `c_backend_matches_llvm_backend`。全部实测在本机
（Windows x64，LLVM 23.1.0 / clang 23）进行，计时按 AGENTS.md 的方法学交错取样取最小值：

| 验收项（§5 Phase 1） | 结果 |
|---|---|
| 输出等价 | **11/11 可运行 `examples/` 双后端 stdout 逐字节一致、退出码一致**（含 stdlib_demo 的泛型/浮点/裸内存、strings 的拼接/比较/结构体/数组、vectors 的聚合值语义） |
| 测试套件 | **127/127 在 `AOXN_BACKEND=c` 下全绿**（pipeline 97 + TS 28 + UI 2）；默认 LLVM 路径同样 127/127 无回归。另新增跨后端对比测试（128/128） |
| 运行性能 | **±10% 内持平**：primes 0.97×、fib 0.99×、bench_array 0.99×、bench_for 0.92×、benchmark 1.10×（交错最小值之比，C/LLVM） |
| 编译时间（端到端含链接） | `examples/hello.ax` 836 → 884 ms（+6%）；**7k 行自举输入 4000 → 4226 ms（+6%）**。阶段分解（单次）：codegen 阶段 C 6.1s（发射 + `clang -c`）vs LLVM 4.9s（IR 构建 + O3 管线 + isel），链接层互有胜负 |

结论：**报告的核心判断被证实**——映射可行（bug 只出在发射细节：数组字面量括号、左值索引文本、
extern 命名，均为一小时内修复的实现错误），C 前端的编译时间回归在 O3 下不显著（+6%），运行性能
同为 clang -O3 产物、持平。"与 clang -O3 平价"的承诺按字面保持成立。

实现中确认的两条纪律（与 §6.3 一致）：生成的 C **不 include 任何 C 标准头**——运行时面走
`__builtin_*`、其余 C 函数以 Aoxn `extern def` 同形声明（与 LLVM 后端的 `get_extern` 完全同构，
且避免与 stdlib 自己声明的 `malloc` 等冲突）；裸内存内建（`load_i64`/`store_u8` 等）一律经
`memcpy` 小助手发射，绕开严格别名/对齐 UB。数组在 C 侧包成单字段结构体
（`typedef struct { T data[N]; }`）以获得与语言规范一致的 C 值语义（赋值/传参/返回按值复制）。

### Phase 2 已完成（2026-10-01，v0.29.0）：LLVM 依赖移除

Phase 2（转默认 + 去 LLVM-C 库 + `selfhost/codegen.ax` 跟进 + 固定点改 C 文本对比）按本报告的推荐
路线执行完毕，四项全部落地：

1. **C 后端转默认并成为唯一后端**：`src/llvm.rs`（LLVM-C FFI）与 `src/codegen.rs`（LLVM-IR
   代码生成）删除，`build.rs`（LLVM 探测/链接/DLL 复制）随 build script 一起移除——`cargo build`
   只需要 Rust 工具链。`--backend c` 仍被接受（即默认），`--backend llvm` 报错并指向 v0.29.0；
   `aoxn ir` 被 `aoxn c`（打印生成的 C 文本，旧 `ir` 拼写保留为别名）取代；`AOXN_PASSES`/
   `AOXN_BACKEND` 移除，`AOXN_DUMP_IR` 更名 `AOXN_DUMP_C`。
2. **自举侧跟进**：`selfhost/codegen.ax` 从 LLVM-C 驱动重写为 C 发射器（镜像 `codegen_c.rs` 的
   映射：struct→C struct、数组包单字段结构体、`__builtin_*` 运行时面），`selfhost/driver.ax` 改为
   写出 `<out>.obj.c` 并 `clang -O3 -w -c` 生成目标文件；`emit_ir`→`emit_c`、`gen_ir_text`→
   `gen_c_text`、`gen_dispose` 删除（无句柄可释放）。
3. **固定点改 C 文本对比**：`selfhost_driver_self_compiles` 比较同一个 Aoxn driver
   经 Rust 编译和经 Aoxn 编译后执行所产出的 C 文本与对象；两条路径都执行
   `selfhost/codegen.ax`，并非 Rust emitter 与 selfhost emitter 的直接比较。
   C 文本逐字节比较，对象仅在 Windows 屏蔽 COFF 时间戳。测试在找不到 clang
   时跳过；当前支持的平台边界见 [`platform-support.md`](platform-support.md)。
   另有 [`tests/c_emitter_parity.rs`](../tests/c_emitter_parity.rs) 对明确公共子集
   直接比较两个 emitter 的完整 C 字节，已知差异单独断言，不计为 parity 通过。
4. **工具链面**：优化级别选择编译生成 C 的 clang `-O`（C 文本与级别无关，由
   `c_text_is_opt_level_independent` 测试固定）；CI 只装 clang（Windows 仍用 winget 的 LLVM 包
   提供其 clang，Linux `apt-get install clang`，macOS 用预装 Apple clang）。

已知差异的收尾：`aoxn ir`/`AOXN_PASSES` 两个 C 后端下的"不适配"项随 LLVM 后端一起消失；C 未
指定的操作数求值顺序语义不变（spec 本就未规定顺序）。

## 8. 参考

本仓库证据：`src/llvm.rs`（全部 FFI）、`src/codegen.rs:91-93`（管线字符串）、`src/lib.rs:371`（clang 探
测）、`build.rs`（LLVM 探测与 DLL 复制）、`selfhost/codegen.ax`（82 个 extern def）、`selfhost/
driver.ax:59`（clang 链接）、`docs/optimization-report.md`（链接层实测数据）、`docs/spec.md:303-343`（O
档位与平价承诺）、`CHANGELOG.md` v0.24 / v0.25（固定点升到 IR / COFF 字节级）。

外部资料（2026-09-30 检索）：

- TPDE: A Fast Adaptable Compiler Back-End Framework（2025，Cranelift/LLVM 编译时间与运行性能对比语境）——
  <https://arxiv.org/pdf/2505.22610>
- rustc 2025 后端与性能工作（cg_clif 现状）——
  <https://kobzol.github.io/rust/rustc/2026/01/05/my-rust-contributions-in-2025.html>
- cranelift-object（经 object crate 发射 ELF/Mach-O/COFF）——<https://crates.io/crates/cranelift-object>
- Cranelift Windows/COFF 进展（TLS for COFF，wasmtime#4546）——
  <https://github.com/bytecodealliance/wasmtime/pull/4546>
- Rust 编译器性能调查 2025（提及 Cranelift 相关工作）——
  <https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results/>
- QBE 官网（官方支持目标列表：amd64_sysv / arm64 / riscv64，无 Win64）——<https://c9x.me/compile/>
- MIR 项目（平台支持现状）——<https://github.com/vnmakarov/mir>
- MIR 项目介绍（Red Hat Developer, 2020/2021）——
  <https://developers.redhat.com/blog/2020/01/20/mir-a-lightweight-jit-compiler-project>
