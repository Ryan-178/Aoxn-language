# P2 优化方案:编译器自身性能与代码质量

## 2026-10-24 实测实施：调用图与模块重写

本节是当前 Rust/C 编译器的测量记录；下方原方案保留为历史材料，里面的
LLVM 文件、行号、测试数和预期收益不能作为当前实现状态。

### 范围、环境与方法

- 基线：`3d537f2`，编译器 `0.54.1`，包管理器 crate `0.42.0`；未 bump 版本。
- Windows 11 x64 / Git Bash，本机 i5-1135G7；`cargo build --release`。
- 命令为 `AOXN_TIME=1 target/release/aoxn.exe c <sample>`，stdout 指向空设备；
  **不含 clang 编译或链接，不含生成程序运行**。`c` 不走可执行文件缓存。
- 保留同一工作树基线 release 二进制，before/after 交错执行，每样本每版本
  6 次，丢首轮热身，后 5 次给出原始值/min/median。偶数轮 before→after，
  奇数轮 after→before。每次都是独立进程。
- 第一轮与 cargo 编译/测试并行，墙钟波动达数倍，不能用于收益结论；完成测试后
  无主动并行构建再测。仍有启动/杀毒/调度噪声，真实小样本变化不作显著收益宣传。
- 两类扩展探针：1000/4000 个函数组成前向调用链，末端 `raise "end"`、main
  调用链首；100/400 个独立模块，每模块 10 个简单函数，入口逐一 star import。
  不是生成程序运行基准，分别隔离异常传播和成员表重复工作。

### 证据与修改

1. `src/codegen_c.rs::compute_fn_raises` 原来反复遍历所有函数与调用直到不动点。
   扫描顺序来自随机 HashMap，链越长，轮数越多；每次进程还可能测到不同成本。
   新算法保留 `scan_fn_body` 的既有判断，把调用边反向后从直接 raiser 出发，
   入队时标记，每个可达函数只处理一次，每条相应调用边至多检查一次，期望
   O(V+E)。没有直接 raiser 时不建立反向图，避免给 selfhost/UI 常见路径加工作。
   不改变 try body/handler/default 的扫描语义，也不借机修复其他语义问题。
2. `src/resolve.rs` 每重写一个模块都深克隆所有模块的 `members`；表在重写期间
   完全只读，直接传 `&members` 即可。去掉的是 O(模块数×总声明数) 的额外复制，
   **不声称整个 resolver 已线性化**。
3. `src/lib.rs` 给 resolve 边界加 `timed("resolve", ...)`。过去 AOXN_TIME
   不含这一段，不能把 lex+parse+typecheck+codegen 之和当完整 Rust 管线。

### 语义保持与验证

- 新单元测试与独立的旧式慢不动点参考算法比较，覆盖递归、无 raiser 的环、
  重复调用、默认参数中的调用、caught try、handler 再 raise、转发链；另有
  4096 函数长链及反向声明顺序测试。没有脆弱的毫秒阈值断言。
- 原有命名空间/别名/选择性导入/冲突/菱形导入测试继续验证 resolver。
- 基线/优化后二进制对 `examples/*.ax`、`selfhost/*demo.ax` 和上述探针共
  **43 个成功样本**的 stdout C、stderr 诊断、退出状态逐字节一致；另外 5 个
  非法输入（未定义调用、严格类型、解析失败、缺失导入、错误默认值）同样保持
  全字节诊断与失败状态一致。
- `cargo test`：**338 通过，0 失败，0 忽略**（根 crate，不含独立 IDE）。
  其中 compiler lib 20、setup 3、pipeline 197、TypeScript lexer 14/parser 21、
  assets 21+18、UI 14、symbols 8、安装 6、其余 stdlib/SDK 集成 16。
- `bash run_pkg_tests.sh`：**104 通过，0 失败，0 忽略**。总计 **442**。
  脚本只产生预期的测试构建标记，不把该产物当项目修改提交。
- 首轮新测试 fixture 写成 `except e` 导致 parse 失败，改为语言已有的
  `except as e` 后全量通过；不是 AV 或实现故障。未修改语言规则。
- `cargo check --workspace --tests --target x86_64-unknown-linux-gnu` 通过，
  只检查 Rust 跨平台编译，不等于 Linux 链接或运行测试。
- 版本、README 与 SECURITY 双语均从当前树重写；wiki 未修改。IDE 测试和
  Linux 真正的链接/运行仍由其独立验证/CI 承担。

### 复现输入

调用链可以用以下 Python 生成（文件应放在忽略的 `target/` 下）：

```python
from pathlib import Path
n = 4000
source = ''.join(
    f'def f{i}() -> int:\n    return f{i+1}()\n' for i in range(n - 1)
)
source += f'def f{n-1}() -> int:\n    raise "end"\n'
source += 'def main() -> int:\n    return f0()\n'
Path('target/chain.ax').write_text(source, encoding='utf-8')
```

模块探针：创建 `m0.ax` 到 `m<n-1>.ax`，每个写入 10 个
`def f<i>_<j>(x: int) -> int: return x + <j>`（使用真正缩进行）；入口每模块一行
`import * from "./m<i>.ax"`，随后 `main` 返回 `f0_0(1)`。

### 最终交错测量（毫秒）

每单元为 before → after；min/median 均取热身后五次。墙钟包括进程启动、
加载、诊断与退出，codegen 只包括 Rust 发射 C。

| 样本 | 墙钟 min | 墙钟 median | codegen min | codegen median |
|---|---:|---:|---:|---:|
| `selfhost/driver_self_demo.ax` | 271.184 → 261.733 | 539.460 → 367.872 | 29.412 → 31.376 | 31.517 → 37.379 |
| `examples/ui_gallery.ax` | 278.083 → 255.181 | 324.703 → 290.626 | 19.337 → 22.666 | 31.659 → 41.425 |
| `examples/stdlib_demo.ax` | 106.463 → 115.401 | 130.312 → 158.313 | 2.589 → 2.719 | 3.556 → 3.171 |
| `target/perf-inputs/chain-1000.ax` | 128.805 → 102.843 | 175.396 → 130.475 | 31.872 → 7.250 | 46.377 → 8.489 |
| `target/perf-inputs/chain-4000.ax` | 670.717 → 159.383 | 767.389 → 227.846 | 535.726 → 35.131 | 590.882 → 42.834 |
| `target/perf-inputs/modules-100/main.ax` | 208.235 → 183.507 | 274.757 → 213.499 | 6.454 → 7.213 | 8.449 → 10.157 |
| `target/perf-inputs/modules-400/main.ax` | 1086.684 → 432.766 | 1287.115 → 804.094 | 32.141 → 26.768 | 50.894 → 41.590 |

#### 原始五次墙钟 / codegen（ms）

- `selfhost/driver_self_demo.ax`
  - before wall: 400.725, 539.460, 562.778, 547.928, 271.184; codegen: 31.517, 58.146, 30.671, 38.713, 29.412
  - after wall: 299.361, 438.080, 694.414, 367.872, 261.733; codegen: 31.376, 71.618, 72.402, 36.111, 37.379
- `examples/ui_gallery.ax`
  - before wall: 392.606, 324.703, 365.027, 315.612, 278.083; codegen: 36.076, 31.659, 31.750, 29.629, 19.337
  - after wall: 290.626, 255.181, 306.663, 294.580, 279.105; codegen: 41.425, 22.666, 45.340, 45.465, 40.312
- `examples/stdlib_demo.ax`
  - before wall: 170.694, 106.463, 125.549, 130.312, 260.946; codegen: 4.994, 2.844, 3.556, 9.307, 2.589
  - after wall: 214.244, 118.118, 115.401, 158.313, 278.038; codegen: 2.830, 4.617, 4.096, 2.719, 3.171
- `target/perf-inputs/chain-1000.ax`
  - before wall: 200.323, 128.805, 175.396, 171.772, 352.180; codegen: 46.377, 31.872, 50.345, 53.517, 45.768
  - after wall: 130.475, 102.843, 124.106, 167.519, 139.327; codegen: 7.250, 8.435, 8.489, 8.716, 10.302
- `target/perf-inputs/chain-4000.ax`
  - before wall: 738.341, 670.717, 934.291, 767.389, 863.900; codegen: 590.882, 569.581, 769.413, 535.726, 614.519
  - after wall: 159.383, 234.761, 219.580, 227.846, 252.963; codegen: 42.834, 35.131, 38.086, 49.433, 45.115
- `target/perf-inputs/modules-100/main.ax`
  - before wall: 274.757, 245.113, 208.235, 322.257, 292.465; codegen: 8.252, 10.120, 8.449, 13.842, 6.454
  - after wall: 213.499, 203.523, 183.507, 216.720, 326.028; codegen: 8.100, 10.157, 7.213, 10.749, 12.252
- `target/perf-inputs/modules-400/main.ax`
  - before wall: 1086.684, 1161.665, 1783.307, 1455.995, 1287.115; codegen: 32.141, 50.894, 61.831, 59.281, 36.441
  - after wall: 432.766, 1002.218, 889.490, 630.644, 804.094; codegen: 26.768, 74.583, 72.497, 36.050, 41.590

真实 selfhost/UI/stdlib 的变化仍受小样本噪声影响，不能宣称普遍加速。
长调用链与多模块探针用于证明具体重复工作的消除，不代表所有真实项目的形状。
未测 Linux 实际性能、IDE 构建耗时、峰值内存或 clang -O3 全链路加速。

下方保留历史方案。

---

> 状态:方案(未实施)。P0(正确性)与 P1(生成代码运行性能)见 CHANGELOG;
> 本文档描述下一步"让编译器跑得更快、代码更干净"的工作项。
> 每项都标注了位置、预期收益和风险,按建议顺序排列。

## 目标

1. **编译速度**:大型/泛型密集程序(如 stdlib + 自举前端)的 typecheck 与
   parse 耗时下降,目标 20–40%。
2. **代码质量**:消除样板与重复、收敛巨型函数、补齐健壮性,使后续
   (自举数组支持等)更易维护。
3. **可度量**:先建立编译耗时基线,每项改动前后对比。

## 度量方法(先做)

- 新增 `AOXN_TIME=1` 环境变量,在 `lib.rs` 各流水线阶段打印 wall-clock
  (lex / parse / typecheck / codegen / link 各自耗时,仿 `AOXN_TC_TRACE` 风格)。
- 基准输入:`stdlib.ax` 全量 + `selfhost/typecheck_demo.ax`、
  `examples/stdlib_demo.ax`,用 `cargo run -- build` 计时;连续 3 次取最优。

---

## A. 编译器热路径(性能)

### A1. 单态化残余克隆(P0 已修大头,收尾)
- 位置:`src/typecheck.rs`
  - `:905` 每个具体调用点 `self.sigs[name].clone()`(Vec<Type> 深拷贝)
    → 改为借用或只 clone 返回类型;
  - `:895` `layout.clone()` 只为躲 `self.structs` 借用 → 用 `Rc`/分离借用改写;
  - `:650`、`:1194` 字段类型 clone → 返回 `&Type`。
- 预期:泛型密集文件 typecheck 时间可感知下降;风险低。

### A2. `bump()` 不再克隆 Token
- 位置:`src/parser.rs:36` — `self.toks[self.idx].tok.clone()` 对每个消费的
  token 整体克隆(含 `String` 载荷、`FStr(Vec<Token>)`)。
- 方案:`std::mem::replace(&mut self.toks[i].tok, Tok::Eof)` 取走所有权,
  或让游标携带 `&Tok`、仅在需要拥有时 clone。
- 预期:parse 阶段分配量减半级别;风险低(注意 `peek`/`peek2` 仍返回引用)。

### A3. 变量查找改 Map(作用域栈)
- 位置:`src/typecheck.rs:1554` `lookup()` 逆序线性扫描
  `Vec<(String, Type)>`,每次命中还 clone `Type`;作用域只推不弹
  (`:339`、`:445`),查询复杂度 O(全部局部变量)。
- 方案:每函数一个 `HashMap<String, (Type, 声明深度)>` + 作用域深度计数
  (出块时惰性失效,或用 Vec<HashMap> 作用域栈);返回 `&Type`。
- 同类:`src/codegen.rs` 的 `struct_fields` 每次字段访问线性扫描
  (`:895`、`:974`、`:1289`)→ 换 `HashMap<String,(usize,Type)>`。
- 预期:大函数体 typecheck 显著加速;风险低。

### A4. f-string 插值的重复词法分析
- 位置:`src/lexer.rs:571` — 每个 `{...}` 插值重新收集 `Vec<char>`、
  补空格前缀后整段递归 `lex()`。
- 方案:对插值子串直接构造子 Lexer(共享 char 缓冲,避免重复 collect);
  或解析插值时复用主 lexer 的切片视图。
- 预期:f-string 重的源文件加速;风险中(布局/列号语义要保持,
  `tests/pipeline.rs` 有 f-string 错误位置测试)。

### A5. 杂项
- `src/codegen.rs:28/43` `call_map.clone()` → `&HashMap` 借用(`Gen` 加生命周期)。
- `src/parser.rs:465-474` elif 折叠 `c.clone()/b.clone()` → `mem::take`/移动。
- 词法层 `Vec<char>` 全量拷贝(`lexer.rs:83`)→ 保留,收益小于改动风险;
  仅在 A4 落地后顺带评估。

---

## B. 代码质量(可维护性)

### B1. `Diag` 构造辅助
- 100+ 处五字段字面量样板(仅 `Parser::unexpected` 有 helper)。
- 方案:`Diag::at(stage, pos, msg)` + `Tc::err(...)` 便捷方法;机械替换。
- 收益:每处错误信息改动的 diff 显著变小;为"多错误收集"铺路。

### B2. 拆分巨型函数
- `Tc::check_expr`(`typecheck.rs:561`,~516 行):builtins、调用、各运算符
  拆子方法;
- `Gen::emit_expr`(`codegen.rs:922`,~350 行):内置函数分发拆出
  `emit_builtin`;
- `lex`(`lexer.rs:81`,~385 行)按 token 类别拆分。
- 风险中(纯搬移,靠 89 测试兜底)。

### B3. 内置函数分发三处合一
- `typecheck.rs:702-888`(检查)、`codegen.rs:757-788`(type_hint)、
  `codegen.rs:1046-1216`(发射)手工同步。
- 方案:一张 `BuiltinSig` 表(名字、参数个数/类型、返回类型、
  是否复合),三处从表驱动。新增内置函数只改一处。

### B4. 其余重复
- 结构体字面量检查两份(`typecheck.rs:638-698` vs `:1170-1241`)合一;
- `lvalue_type`(`:511-559`)与 `check_expr` 的 Index/Field 臂合一;
- 字段 GEP 降低三份(`codegen.rs:889/965/1288`)→ `field_ptr(base, sname, fname)`;
- `load_u8`/`store_u8` 指针计算两份(`:1109` vs `:1165`);
- `adv!` 宏两份(`lexer.rs:91` vs `:480`)、字符串转义两份(`:290` vs `:592`)。

### B5. 清理
- 注释 mojibake(乱码破折号 `鈥?`):`lexer.rs:450`、`ast.rs:102/199`、
  `lib.rs:84/90`、`parser.rs:479`、`codegen.rs:421/436/728`、
  `typecheck.rs:1419` 等 —— 统一替换为 `—`;
- 死代码:`typecheck.rs:799` 恒空字符串条件;`:236/244` 重复 clear;
- `llvm.rs:5` blanket `allow(dead_code)` → 逐项标注,删掉未用 FFI;
- `parser.rs:232-244` `cur_len_params` 双重清空;`lexer.rs:450` 报错文案
  说 `'('` 但可能是 `[`。

---

## C. 健壮性补遗(P0 未尽项)

### C1. `ty_of` 返回 `Result`(消灭空类型 → LLVM 崩溃)
- `codegen.rs:144` 未知结构体回退 `null`,后续 LLVM 调用直接崩;
  `Type::Array` 长度超过 `u32::MAX`(`GENERIC_LEN` 泄漏、超长标注
  `[int; 50000000000]`)静默截断。
- 方案:`ty_of`/`const_int` 改 `Result<_, String>`,~25+8 个调用点加 `?`
  (两处闭包 `:250/:271` 改循环)。编译器-内部错误一律走 `internal` diag。

### C2. `--O0` 语义
- 现状:`--O0` 跳过 `LLVMRunPasses` 但 codegen level 仍是
  `CODEGEN_LEVEL_DEFAULT`(`codegen.rs:393`)→ 改为 `CODEGEN_LEVEL_NONE`。

### C3. clang 解析顺序
- `lib.rs:246-266` PATH 优先于仓库/安装目录的 clang —— PATH 上的旧
  clang 18/19 会遮蔽 LLVM 23 的 clang。考虑把 PATH 挪到最后,
  或在版本不匹配时告警。

### C4. 多错误收集(可选,产品向)
- 目前 fail-fast 只报第一个错误。`Diag` 收集成 `Vec`、前端各阶段尽量
  恢复并继续,`--json` 输出全部错误 —— 对 AI 消费体验提升明显,
  但涉及错误恢复策略,单独立项。

---

## D. 建议实施顺序

| 批次 | 内容 | 预计工作量 | 风险 |
|---|---|---|---|
| 1 | 度量(AOXN_TIME) + A1 + A2 + A5 | 半天 | 低 |
| 2 | A3 + A4 | 半天 | 中 |
| 3 | B1 + B5 | 半天 | 低 |
| 4 | B2 + B3 + B4 | 1 天 | 中 |
| 5 | C1 + C2 + C3 | 半天 | 低 |

每批之后跑全量 `cargo test`(89+)并对 `examples/bench_*.ax` 做一次
编译耗时回归对比。

## 明确不做(记录在案)

- 字符串表示改 fat pointer / length-prefix header(P1 已用"长度伴随
  slot + known_len 缓存"解决热点拼接;彻底改表示属 ABI 变更,
  待自举稳定后另议)。
- LLVM pass 定制、LTO、PGO —— 收益不明,等 C 组落地后再评估。
