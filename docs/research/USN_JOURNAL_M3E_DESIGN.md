# M3-E 设计文档：USN Journal、后台监控与跨平台能力

> 状态：**部分实现 + 完整设计**（2026-11-07 本轮）· 前置研究：TECH_FEASIBILITY.md Q3（已验证）
> 本文档回答总指令 M3-E 的全部研究项；"已实现"部分有真机测试证据，"设计"部分明确标注。

---

## 1. 权限边界（决定性约束，已验证）

| 能力 | Windows API | 权限 | 产品决策 |
|---|---|---|---|
| 卷级 USN 枚举（MFT 快照） | `FSCTL_ENUM_USN_DATA` | 需管理员（GENERIC_READ 卷句柄） | **不实现**（红线：不默认提权） |
| 卷级 USN 增量读取 | `FSCTL_READ_USN_JOURNAL` | 同上 | **不实现**（同上） |
| **单文件 USN 记录** | `FSCTL_READ_FILE_USN_DATA` | **普通句柄即可，无需管理员** | **已实现**（能力探测 + 身份交叉验证基础） |

**运行时实证**（本轮真机）：
- `read_file_usn_data()` 在本机 NTFS（C 盘）实测成功：普通用户权限下返回
  FileReferenceNumber / ParentFileReferenceNumber / Usn / 文件名（platform 4/4 测试）；
- 卷句柄探测 `probe_volume_usn()`：普通权限下卷级 USN 不可用，探测如实报告
  原因（不伪装、不提权、不重试）。

## 2. 文件系统差异矩阵

| 文件系统 | USN Journal | 说明 | FMap 行为 |
|---|---|---|---|
| NTFS | ✅ v2/v3/v4 记录 | 主战场 | 全部 USN 能力可用 |
| ReFS | ✅（v3+ 记录） | 语义与 NTFS 有差异（v4 覆盖） | 支持判定 = true；增量读取设计见 §4 |
| FAT / exFAT | ❌ 无 USN Journal | FSCTL 返回 ERROR_NOT_COMMAND_SUPPORTED | 能力探测诚实报告 + 安全降级（§5） |
| 其他（CDFS 等） | ❌ | — | 同上 |

实现落点：`platform::journal::journal_capability()` 按 `GetVolumeInformationW`
的 fs_type 判定 `usn_supported_fs`（NTFS/ReFS → true）。

## 3. 已实现：单文件 USN（本轮交付）

**位置**：`crates/platform/src/journal.rs`

- `read_file_usn_data(path) -> FileUsnInfo`：打开普通句柄（FILE_READ_ATTRIBUTES +
  FILE_FLAG_BACKUP_SEMANTICS）→ `FSCTL_READ_FILE_USN_DATA` → 解析 USN_RECORD_V2
  （版本校验失败诚实拒绝；文件名与路径名可交叉核对）；
- `probe_volume_usn(volume)`：卷级访问探测（不提权），报告 denial_reason；
- `journal_capability(volume)`：一卷的完整能力画像（fs_type / usn_supported_fs /
  volume_usn / file_usn_available）；
- 用途定位：**M3-A 身份交叉验证增强**（FileReferenceNumber 与 file_entities
  的 file_id 同源，可校验身份重用嫌疑）+ 能力报告 UI。不是事件源。

**真机测试**（4/4 通过）：
- 文件 USN 读取：FileReferenceNumber ≠ 0、文件名一致、父引用 = 父目录自身引用；
- 目录 USN 读取成功；
- 卷级探测诚实报告不可用；
- 非 v2 记录版本拒绝。

## 4. 设计：卷级 USN 事件流（不实现，红线内保留设计）

**前置条件**：管理员权限。产品红线明确"不默认申请管理员、不绕过权限"，
因此卷级 USN 增量读取**不做为自动功能**。若未来产品决策改变，设计如下（不激活）：

- **数据流**：`FSCTL_READ_USN_JOURNAL`（增量 NextUsn 游标）→ 解析 reason 掩码 →
  写入 `events` 原始通知表（v6 结构零变更，watch_root = `usn://<volume>`）→
  与 RDCW 共用同一 gap 生命周期与对账闭环；
- **日志 ID 与回卷边界**：
  - USN Journal 可被 `FSCTL_DELETE_USN_JOURNAL` 删除/重建 → JournalID 变化 →
    旧游标失效（ERROR_JOURNAL_DELETED）→ 必须重开 gap → 对账扫描补齐；
  - 日志回卷（wrap）覆盖旧记录 → 以"上次 NextUsn 之后无连续性"判定 → 开 gap；
  - 系统休眠/卷重挂载后 JournalID 不变则续读，变化则 gap；
- **缺口语义**：USN 不可用/被清理期间 = 监控缺口，绝不伪装完整记录（与 B2 同规范）；
- **不把 USN 事件等同来源**：USN reason 只说明"文件被写过/更名"，
  不携带进程归属（ETW 需管理员且已被产品决策排除）。

## 5. 安全降级（已成立的行为）

USN Journal 不可用（FAT/exFAT、无管理员、日志被清理）时：
- 扫描差分（M3-B1）照常工作——事件的完整性来源是扫描代，不依赖 USN；
- RDCW 监控（M3-B2）照常工作——逐目录授权，无需管理员；
- 能力探测 UI 如实展示"该卷 USN：不可用（原因）→ 已降级"。

## 6. 后台监控不失控的保证（B2 已实现，此处登记为 M3-E 验收项）

- 应用退出钩子取消全部监控会话（stderr 实录验证过）；
- 监控句柄随 AppState 生命周期，无独立后台进程/服务；
- 不安装驱动、不注册服务、不自启（红线 #9/#11）。

## 7. 跨平台架构抽象现状

| 抽象点 | 现状 |
|---|---|
| `FileSystemProvider` trait（platform） | 磁盘发现/能力声明已抽象；Windows 实现完整，其他平台返回 Unsupported |
| 监控（watcher） | RDCW 是 Windows 专属；Linux 对应 inotify / macOS 对应 FSEvents，trait 位保留未实现（不做空壳伪装） |
| 身份（identity） | NTFS FileId 主身份 + 弱身份降级链——弱身份在非 NTFS 卷已可用（M3-A 设计即跨平台安全） |
| USN（journal） | 本轮新增；非 Windows 返回 Unsupported（诚实） |
| core（扫描/差分/关联/路径规范化） | 平台无关，天然可移植 |

**原则**：不为跨平台把 Windows 已验证能力削弱成空壳——Windows 能力保持真实实现，
其他平台在 trait 位返回明确的 Unsupported（如 impl），不假装可用。

## 8. 本轮 M3-E 交付清单

| 项 | 状态 |
|---|---|
| 单文件 USN 读取（无需管理员） | ✅ 已实现 + 真机测试 |
| 卷级 USN 能力探测（不提权） | ✅ 已实现 + 真机测试 |
| 能力画像 API + IPC + UI（监控页） | ✅ 本轮 |
| 卷级 USN 增量事件流 | 📋 设计完成（§4），红线内不激活 |
| inotify/FSEvents 适配 | 📋 trait 位保留，未实现（诚实 Unsupported） |
