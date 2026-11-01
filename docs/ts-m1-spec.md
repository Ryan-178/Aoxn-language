# TS-M1 技术规范 — TypeScript 前端 → 现有编译管线

> 决策已定：方案 A（TS 前端产出相同 AST，经现有检查器 → C → clang）；
> 独立包管理（自有 manifest/lockfile/registry + 一次性 npm 导入工具）。
> 本文保留 W1 的设计分层，§0 及遗留项区分实际覆盖与后续目标；不承诺完整 TS 兼容。
> S3 曾移除旧导入，v0.43.0 已恢复 `import "path"`（等价整模块导入）。
> 当前绑定层见 [`src/resolve.rs`](../src/resolve.rs)，平台边界见
> [`platform-support.md`](platform-support.md)。

## 0. 分层路线

| 切片 | 内容 | 状态 |
|------|------|------|
| **S0** | TS 词法器（全 token 集 + ASI 换行标记） | **已完成**（`src/ts/lexer.rs`，测试见 `tests/ts_lex.rs`） |
| **S1** | TS 解析器：声明/表达式子集 → 现有 AST | **已完成**（`src/ts/parser.rs`，测试见 `tests/ts_parse.rs`） |
| **S2a** | 泛型函数 `<T>`、数组类型 `T[]`/`Array<T>`（→ `[T; N]` + 合成长度参数，复用单态化器）、模板串插值 `${}`（→ `str()` 拼接链）、显式类型参数检测 | **已完成** |
| **S2b** | f64 数值塔、联合/收窄、`any`/可选参数/元组、`as`/`!`、多长度参数数组 | **已完成**（多长度参数数组除外——单态化器仍是单长度参数，见 §0.1） |
| **S3** | 模块系统（import/export 解析）与当年的 stdlib/selfhost 迁移（旧导入现已恢复） | **已完成**（当时以 IR/COFF 固定点验证；当前改为 C/对象对比） |
| S4 | 包管理客户端 + npm 桥 | **已落地 beta**：`crates/aoxn-pkg`；实际命令 `aoxn npm-import`（v0.29.5），见 §4 |
| S5 | 验收样本三件套 + CI 接入 | 尚未宣告完成，不能由现有前端测试推定验收 |

S1 已覆盖（端到端验证）：`function`/`interface` 声明、`const`/`let`、
if/while/do-while/C 风格 for/for-of、break/continue/return、调用/成员/索引、
字面量与数组字面量、`console.log`→`print`、`x.length`→`len(x)`、对象字面量
→struct 字面量（需 interface 标注）、`i++`/复合赋值语句。`aoxn build foo.ts`
直接可用（`.ts`/`.tsx` 走 TS 前端，其余走 Aoxn 前端，汇入同一 AST）。

S2a 已覆盖（端到端验证）：泛型函数 `function f<T>(...)`（类型参数按名引用，
调用点自动推断，复用现有单态化器）；数组类型 `T[]`/`Array<T>`（函数签名内
→ `[T; N]` + 合成长度参数 `N`，调用点按字面量长度实例化）；模板串插值
`` `a${x}b` `` → `"a" + str(x) + "b"` 拼接链（支持嵌套模板与任意表达式）。
**限制**：每函数仅一个数组长度参数（多数组参数独立长度属 S2b）；数组返回
需有数组参数钉住长度；显式类型参数 `f<number>(x)` 明确报错（推断即可，
避免与 `a < b > (c)` 比较链产生歧义误判）。

S2b 已覆盖（端到端验证）：
- **f64 数值塔**：`number` 统一为 double（`Type::Float`）；数字字面量全按
  JS number 生成 f64；索引/长度等 int 位置经 `Cast`/`to_int` 转换；混合
  算术自动形态化（字面量向另一侧宽度靠拢）。位运算（`& | ^ ~ << >> >>>`）
  与 `%` 按 JS int32/fmod 语义降级到 `__ts_*` 运行时助手（Aoxn 源码注入）；
  `console.log` 与模板插值按 JS 口径格式化数字（`__ts_num`：整数值打印
  整数形式，非整数去尾零）。
- **联合 + null 判别收窄**：`T | null | undefined` 擦除为 `T`，null 走
  类型哨兵（string→`""`、number→`0`、bool→false）；`x == null` /
  `x != null` 降级为哨兵比较。**M1 偏差**：哨兵值与 null 不可区分
  （`""`/`0`/false 同时是 null）。非空并集 `A | B` 保留首成员（运行时
  标签属 TS-M2）。
- **`any`/`unknown`**：64 位 int 盒子（`as` 收窄）；`never` → `void`。
- **可选参数**：`x?: T` / `x: T = e` 调用点省略时填哨兵/默认值
  （post-pass 补参，函数提升顺序无关）。
- **元组**：`[A, B]` 合成值结构体（字段 `_0`/`_1`），`t[0]` 读字段。
- **`as`/`!`**：`x as T` 生成检查过的标量转换（int/float/bool）；
  `x!` 非空断言为恒等（M1 无可空值）。

S3 已覆盖：`import * from "p"`（整模块合并）、`import { a, b } from "p"`、
`import d from "p"`、`import "./p"`（副作用合并）；`export function`/
`export interface` 透传为普通声明；`./`/`../` 相对解析带扩展补全
（`.ax`/`.ts`/`.tsx`、`index.<ext>`），裸名走 `aox_modules/`（W2 的
`aoxn pkg` 客户端管理安装；编译器由 `src/pkg_manifest.rs` 读取入口）。
旧 `import "path"` 在 S3 曾删除，**v0.43.0 已恢复为整模块导入**；命名
导入/别名由 `src/resolve.rs` 解析，不再一概合并。自举 parser 有导入
语法但没有绑定层，选择性导入明确拒绝。固定点的准确范围见
[`selfhost.md`](selfhost.md)，不能据此声称两套 emitter 全功能一致。

### 0.1 遗留（S2b 未完成项）

- **多数组长度参数**：`f(a: T[], b: U[])` 的独立长度仍受"每函数一个长度
  参数"限制（GENERIC_LEN 单哨兵 + `len_param: Option<String>`）。解除需要
  单态化器携带长度参数向量（unify/subst/mangle 全链），计划在 W1 收尾
  或 TS-M2 前落地。
- `import * as ns`（命名空间对象）、`export default`、`export { x }`
  re-export、顶层 `const`/`let` 语句（模块初始化）→ 后续切片。
- `typeof`/`instanceof`/`in`、类、闭包 → TS-M2。

TS-M2（对象引用语义/闭包/GC/async）在 M1 之后，见
[web-platform-plan.md](web-platform-plan.md) §1 里程碑。

## 1. TS-M1 原始目标清单（不是当前解析器覆盖保证）

以下保留原始设计面；当前已实现范围以 §0 和实际
[`src/ts/parser.rs`](../src/ts/parser.rs)、[`tests/ts_parse.rs`](../tests/ts_parse.rs) 为准。
类、箭头闭包、`new`、switch、try/catch/finally 等不能因列在本节就视为已支持。

**声明**：`import`/`export`（named/default/re-export）、`function`、
`const`/`let`、`class`（字段+方法+constructor，无装饰器）、`interface`、
`type` 别名（对象/联合的受限形式）、`enum`（数字枚举）、`declare`（忽略）。

**语句**：`if/else`、`for`（C 风格 + for-of 数组）、`while`、`do`、
`switch`、`try/catch/finally`、`throw`、`return`、`break`/`continue`、
块语句、表达式语句。`var` 不支持（报错，语义为块作用域 let）。

**表达式**：字面量（number/string/boolean/null 受限/模板串）、标识符、
成员访问 `a.b`/`a[b]`、调用、`new`、一元/二元/三元、赋值复合、箭头函数
（表达式体+块体）、`typeof`/`instanceof`/`in`、展开 `...`（数组字面量）。

**类型标注**：`number|string|boolean|void|any|unknown|never`、对象类型、
接口名、数组 `T[]`/`Array<T>`、元组（定长）、函数类型、字面量类型、
联合 `A | B`（M1 受限：仅判别联合收窄）、`?` 可选、泛型 `<T>`（单态化
复用现有 GENERIC 机制）、`as`/`!` 断言。

**不在 M1**（明确报错并给迁移提示）：async/await、装饰器、namespace、
`var`、BigInt、正则字面量、动态 `import()`、原型改写、`this` 别名技巧。

## 2. 语义映射（TS 构造 → 管线构造）

| TS | 管线 | 说明 |
|----|------|------|
| `number` | `float`(f64) | JS 数字即 double（S2b 起全量）；位运算按 JS int32 语义降级到 `__ts_*` 助手；`%` 是 fmod；`console.log` 数字按 JS 口径打印 |
| `boolean` | `bool` | |
| `string` | `string` | 不可变字节串；UTF-8（JS 为 UTF-16，差异记入已知偏差） |
| `T[]`（定长用法） | `[T; N]` | M1 数组为定长值语义；`push/pop` 等动态操作属 M2 |
| `interface` | `struct` | 按结构检查（TS 结构化类型在 S2 校验层实现） |
| `class`（目标，未实现） | struct + 方法函数 | 不属于当前 M1 覆盖 |
| `function` | `def` | 闭包捕获属 M2；M1 禁止逃逸闭包 |
| 泛型 `<T>` | `def f[T, N]` | 复用现有单态化器 |
| `null`/`undefined` | 类型哨兵 | `T | null` 擦除为 `T`；null 值=类型哨兵（`""`/`0`/false），`x == null` 降级哨兵比较；偏差：哨兵与 null 不可分（TS-M2 引入真可空） |
| 模板串 `${}` | `"lit" + str(expr)` 链 | 与现有 f-string 降级一致 |

## 3. 模块系统（替代旧 `import`）

```typescript
import { render, type Page } from "./renderer";   // 相对路径
import express from "express";                    // 包导入（走包管理）
export function handler(): void { ... }
// export default handler;  // 后续目标，当前不支持
```

- **解析顺序**（S3 已实现）：`./`/`../` → 相对文件（`.ax`/`.ts`/`.tsx`
  与 `index.<ext>` 补全）；裸标识符 → `aox_modules/<pkg>`（`main`/`exports`/`types` 已由 `src/pkg_manifest.rs` 实现，
  见 [`pkg-manager.md`](pkg-manager.md)；不是 npm/node_modules 透明兼容）。
- **include-once 与循环检测**沿用 loader 语义；Rust `src/lib.rs` 与
  `selfhost/load.ax` 已同批迁移（自举侧新增 `.`/`..` 归一化保持 key 稳定）。
- **旧 `import "path"` 在 v0.43.0 已恢复**，等价 `import * from "path"`；
  `import * as ns` 仍拒绝，不能与 Aoxn 的 `import m` 编译期命名空间混为一谈。
- **`.css` 是一等资产，不是源码**（v0.34.0，W2 的 CSS 管线）：`import`
  到的 `.css` 在 `load_file` 被截获进 `src/assets.rs`，不进任何前端/
  lexer。`@import` 就地内联、保守压缩、指纹化，产物经 `styles()` /
  `styles_fingerprint()` 以编译期常量提供；`*.module.css` 额外提供
  `<stem>_class(name)`。详见 [`css-assets.md`](css-assets.md)。

## 4. 包管理（独立生态，A2）

| 项 | 设计 |
|----|------|
| manifest | `aoxn.json`：name/version/main/exports/types/dependencies |
| lockfile | `aoxn.lock`（内容寻址，含完整性哈希） |
| 包目录 | `aox_modules/`（扁平 + 版本后缀消解冲突） |
| registry | 已实现目录/git/只读 HTTP 镜像；不代表自建鉴权 registry 服务已完成 |
| **npm 桥（已实现）** | `aoxn npm-import <npm-pkg>[@ver]`（或 `aoxn pkg npm-import`）：用 npm CLI 获取含 `aoxn.json` 的包，写入 `vendor/<name>/` 并记录 path dependency；拒绝普通 JS 包，不自动翻译或发布到 registry。实现/测试见 `crates/aoxn-pkg/src/npm.rs` |

## 5. 验收样本（无真实项目期间的基线）

1. **REST API 服务**：路由分发、中间件函数、JSON 组装、错误处理。
2. **SSR 页面渲染**：HTML 拼装、CSS Modules 导入、循环渲染表格。
3. **泛型工具库**：泛型函数/接口/类 + 单元断言。

每个样本须在 M1 达成：`aoxn build sample.ts` 产出可执行文件、输出与
`tsc`+Node 参考实现逐字节一致（字符串/JSON 口径）。

## 6. 测试与回归门

- 保持现有编译器测试绿色；「97」是原始设计时的历史基线，不是当前总数。
  TS 前端汇入现有 AST/C 管线，测试应核对输出和诊断，不能只看是否解析。
- TS 前端自测：词法 token 流精确比对（S0 起）、解析 AST dump 比对、
  样本编译输出比对。
- 自举固定点当前比较跨阶段 selfhost emitter 的 C/对象（Windows 屏蔽
  COFF 时间戳）；直接 Rust/selfhost emitter 字节 parity 是独立公共子集测试，
  详见 [`c-compatibility.md`](c-compatibility.md)。
