# 许可证审计（LICENSE_AUDIT）

> 版本：v0.9（夜间研究稿） · 日期：2026-11-03
> 目的：确保 FMap 开源发布时合规使用第三方成果，避免许可证污染。

---

## 1. 竞品/参考项目许可证

| 项目 | 许可证 | 对 FMap 的含义 | 状态 |
|---|---|---|---|
| WinDirStat (windirstat/windirstat) | **GPL-3.0+** | ⚠️ 可以使用其软件、学习其思路，但**禁止复制其源代码**进 FMap（会强制本项目变 GPL）。treemap 思想本身不受版权保护，可安全借鉴设计理念 | ✅ 已验证（api.github.com，2026-11-03） |
| SquirrelDisk (adileo/squirreldisk) | **AGPL-3.0** | ⚠️ 最强传染性（网络服务场景也触发）。watch/agent 模式只看思路不抄实现 | ✅ 已验证（api.github.com） |
| duvis (yamadashy/duvis) | **MIT** | ✅ 分类规则表与继承规则可 fork（保留版权声明），叠加 AI 工具映射层 | ✅ 已验证（api.github.com） |
| diskplot (rodrigokiller/diskplot) | **MIT** | ✅ 扫描/快照/Live 范式可全文精读移植 | ✅ 已验证（api.github.com） |
| ccusage | Other / NOASSERTION | 条款未逐字核对【待验证】；仅作需求证据引用，不接触代码 | ⏳ 待验证 |
| WizTree（闭源商品） | 专有（个人免费/商用付费） | 只能作为"产品行为参考"（MFT 扫描需管理员），不接触其代码 | ✅ 已知 |
| Everything (voidtools) | 专有免费 | 同上，仅作行为参考 | ✅ 已知 |
| SpaceSniffer | 专有免费 | 同上 | ✅ 已知 |
| notify-rs/notify | **notify 核心 = CC0-1.0（公有领域）；notify-types / debouncer-mini / debouncer-full / file-id = MIT OR Apache-2.0** | ✅ 可作为依赖直接使用；CC0 公有领域无任何附加条件 | ✅ 已验证（README 分 crate 声明） |

> 规则：对所有竞品，**只看思想、不看代码抄实现**（除非许可证明确为宽松许可且我们有意引入依赖）。文档中引用竞品截图/文本需注明出处。

## 2. 计划技术栈的许可证（全部宽松，可安全商用与闭源衍生）

| 组件 | 许可证 | 说明 |
|---|---|---|
| Tauri 2 | MIT OR Apache-2.0 | 双许可 |
| Rust 工具链 | MIT OR Apache-2.0 | |
| React 18 | MIT | |
| TypeScript | Apache-2.0 | |
| Tailwind CSS | MIT | |
| D3.js | ISC | 等效 BSD-3 |
| SQLite | Public Domain | rusqlite 绑定为 MIT |
| notify（核心） | CC0-1.0 | 公有领域；debouncer-full/file-id 等伴生 crate 为 MIT OR Apache-2.0 |
| serde / rayon / tokio（如用） | MIT OR Apache-2.0 | Rust 生态标准双许可 |
| reqwest 等（若引入） | MIT OR Apache-2.0 | 注意：FMap 无网络出站需求，原则上不引入网络库（见 SECURITY.md） |

> 结论：建议的技术栈**没有 GPL/AGPL 传染风险**。注意 Tauri 的 Windows 安装器模板（NSIS/WiX）许可证为 MIT/Zlib 级别，无问题。

## 3. FMap 自身许可证建议

两个候选（最终由用户决定）：

| 选项 | 优点 | 缺点 |
|---|---|---|
| **MIT**（推荐默认） | 最简单、对社区适配器贡献者最友好 | 无专利条款 |
| **Apache-2.0** | 有专利授权与商标条款，对企业更放心 | 文件头稍繁琐 |
| 双许可 MIT OR Apache-2.0 | Rust 生态惯例，贡献者二选一 | 初期略复杂 |

**建议**：采用 `MIT OR Apache-2.0` 双许可（与 Rust/Tauri 生态一致）；若求最简则 MIT。

## 4. 合规检查清单（发布前）

- [ ] `LICENSE` / `LICENSE-APACHE` / `LICENSE-MIT` 文件就位
- [ ] 前端依赖清单（package-lock）+ `npm licenses` 导出存档
- [ ] Rust 依赖清单（cargo deny / cargo license）存档
- [ ] 无 GPL 代码混入（WinDirStat 等仅思想借鉴）
- [ ] 规则包数据（AI 工具目录签名）为自主调研成果 + 官方文档引用（事实与路径不受版权保护，逐条注明来源 URL）
- [ ] 图标/logo 原创或使用允许商用的图标库（如 Lucide/Tabler，ISC/MIT）
- [ ] README 注明第三方组件与致谢
