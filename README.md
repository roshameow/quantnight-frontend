# QuantNight

[English](README.en.md) · [下载安装包](https://github.com/roshameow/quantnight-frontend/releases/latest) · [使用指南](docs/user-guide.zh.md)

量化研究任务的桌面控制台：管理普通、Priority 和 Super 任务，查看本地与远程执行进度，筛选研究结果。

![QuantNight 任务管理演示](assets/demo.gif)

## 先体验

无需数据库、平台账号或私有后端，即可运行使用合成数据的交互演示：

```bash
git clone https://github.com/roshameow/quantnight-frontend.git
cd quantnight-frontend
npm ci
npm run demo
```

打开终端给出的地址，在 `/demo.html` 查看任务卡片、切换本地/远程标记、筛选示例结果。演示中的启动、暂停、删除仅修改内存数据，刷新后重置；不会提交模拟任务。需要 Node.js 22 或更新版本。

## 这个仓库包含什么

| 内容 | 可用范围 |
| --- | --- |
| Vue + Tauri 桌面界面 | 本仓库公开源码，可自行构建 |
| 任务展示、配置界面、结果浏览 | 桌面运行需要配置 MongoDB；演示无需 MongoDB |
| 回测执行、任务生成与远程调度脚本 | 属于独立的私有 `quantnight` 后端，未包含在本仓库 |

下载安装包并不包含执行后端。完整研究工作流需要已有的兼容后端、脚本映射和相应平台账号。演示数据是虚构数据，不代表实际回测表现。

## 桌面安装与开发

从 [Releases](https://github.com/roshameow/quantnight-frontend/releases/latest) 下载 macOS 或 Windows 安装包，按[首次配置指南](docs/user-guide.zh.md)连接自己的环境。

```bash
npm ci
npm run tauri:dev
```

源码构建还需要 Rust 和 [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)。先用演示检查界面，再配置完整桌面环境。

## Priority 调度与暂停监控

配合支持per-root调度的后端，任务卡的「调度优先级」是数值越大越优先的正数权重（默认1），不是阶段号或预算。同一个原任务消费者队列按 **任务权重 → R3/R2/R1 → 同阶段已开始root → 稳定顺序** 调度，不抢占已运行的模拟，也不保证不同消费者/机器之间的全账号严格排序。每root独立最多3阶段×60候选，来源和预算保持不变。

请求停止显示当前实际步骤（任务身份查询、执行脚本、会话/进程退出核验、回执校验、暂停记录回写）。**成功读取 tmux 且没有精确匹配会话，按用户定义就是暂停一致状态，不是检查错误**：普通运行检查可显示「已暂停（tmux会话不存在）」；显式暂停则返回可核验的会话不存在回执，校验身份/目标并完成控制记录回写后幂等成功。若开始时会话存在，仍按原进程身份跟踪/退出核验后停止。两种依据分别保存为 `session_absent` / `runners_exited`，不能把没采集过的历史PID伪报全部退出，也不代表平台模拟取消。SSH失败、权限错误、空输出或格式损坏仍是具体检查错误，不冒充会话不存在。诊断长文默认折叠在卡片的「运行 / 停止详情」，摘要保留最后阶段，展开查看完整原因；状态徽标持续可见。失败仍须提供具体阶段和原因，不能只写「unknown／停止状态未确认」。日志、Ctrl+C或历史paused记录本身不是退出凭据。旧active/blocked事件不能覆盖停止操作状态，核验成功后的列表刷新超时也不撤销凭据。root进度、真实结果及预算保留；研究池快照不代表进程仍在运行，控制器退出也不代表平台模拟已结束。暂停的legacy任务若成功确认没有旧campaign汇总，只保留已有统计并明确指出缺少哪份进度证据，不反复报缺少账本错误，也不恢复任务；已有campaign继续观察准确ID的回执，真正的网络/账本错误仍会报告。

普通/SUPER/Priority卡片区分控制记录与实际执行现状。原检查脚本按目标主机检查精确tmux会话及pane/子进程；进程观察不等于平台请求进度或派发许可。原生桥逐行转发原脚本的 `QUANTNIGHT_TASK_STAGE` 进度，通过 `task-operation-stage` 事件绑定任务ID、执行目标及本次requestId。卡片显示最后真实到达的阶段；超时、SSH/远端启动错误、tmux/进程读取错误、输出格式错误、身份冲突、检查结果过期或缺少历史停止凭据均有具体解释，不显示笼统unknown标签。没有阶段回传时明确写等待Native阶段进度失败，不猜成TCP/SSH错误。迟到或不匹配的阶段事件不能覆盖新操作/已确认暂停，事件也不能替代停止回执或数据库CAS。

消费者存活不等于正在派发：同一任务/执行目标的新鲜派发元数据会区分“请求结果待核验”和“等待配额恢复”。它们是保留进程、心跳与已接受请求收集的等待状态，不是任务退出或已暂停；缺失、过期、身份冲突或进程未运行的元数据不能伪装成等待中。

这些改动需新版后端消费者及重新构建的前端原生程序配合；修改源码或刷新网页不会升级正在运行的进程，也不代表已发布新版安装包。

## 文档

- [用户指南](docs/user-guide.zh.md)：数据库准备、设置、任务与数据页面。
- [系统架构](docs/architecture.zh.md)：本地/远程语义、数据流、脚本执行边界。
- [版本记录](CHANGELOG.md)：源码与安装包版本说明。
- [问题反馈](https://github.com/roshameow/quantnight-frontend/issues)：请附版本、系统、复现步骤及脱敏日志。

## 版本与许可

当前源码版本为 **1.2.4**；可下载版本以 Releases 为准。历史 `v1.2.3` 安装包曾使用 `0.1.0` 文件名，详情见版本记录。

[MIT](LICENSE)

### Template runtime directories

REGULAR and SUPER defaults now use the configured QuantNight backend root:
`templates/generated/regular/` and `templates/generated/super/`. Frozen task
entries are `<name>.research.json`; genuinely custom entries may be `<name>.py`.
Priority templates are unchanged. The settings store migrates only exact known
old defaults (including packaged `./templates` / `./templates/super`); user-custom
paths are retained, not replaced on refresh.

Both add dialogs use persisted settings, accept descriptor/Python filenames, and
store only the bare template name. The file picker associates an **already
published backend template**; it does not upload or deploy it. When the generated
directory is empty, manually enter a legacy name or select its file; regular
`templates/<name>.py` and SUPER `super_templates/<name>.py` remain compatible.
Existing task records are not rewritten.

Native discovery and pre-delete research binding lookup call the backend's pure
`python -m templates.catalog` CLI through `<root>/.venv/bin/python`. This requires
the matching backend revision. Discovery accepts the relevant legacy/generated
directory only; a preserved unsupported custom path produces an explicit error,
not a silent fallback. Helpers/claims/temp files are excluded; duplicate names
within one task kind fail rather than shadowing. Dataset deletion still archives
statistics via original `taskctl archive-research` with exact task ID/remote target
and validated receipt before mutation; refine descriptors are not dataset bundles.

No automatic deployment or service restart is included. Remote generation still
requires the original dependency/sync preflight; ignored runtime files are not
proof that the executor has their manifests or loaders. See `tests/README.md` for
offline checks; these are not native-app or real-task smoke tests.
