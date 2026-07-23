<div align="center">
  <img src="src-tauri/icons/128x128.png" alt="FocuSD Island Logo" width="96" height="96">

  <h1>FocuSD Island</h1>

  <p>
    一款 Windows 灵动岛效率工具，把待办、每日笔记、Codex/Claude Code 状态、剪切板历史、媒体控制和窗口定位放在屏幕顶部。
  </p>

  <p>
    <a href="https://github.com/wodetiannacao/FocuSD-super/releases/latest">下载 Release</a>
    ·
    <a href="https://github.com/wodetiannacao/FocuSD-super/issues">反馈 Issue</a>
    ·
    <a href="#路线图">查看路线图</a>
  </p>

  <p>
    <img alt="Version" src="https://img.shields.io/badge/version-0.1.7-blue">
    <img alt="Platform" src="https://img.shields.io/badge/platform-Windows-0078D4">
    <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24C8DB">
    <img alt="React" src="https://img.shields.io/badge/React-19-61DAFB">
    <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178C6">
  </p>
</div>

> 本项目基于原作者 [zzliu93-debug/FocuSD](https://github.com/zzliu93-debug/FocuSD) 继续开发。
> 当前仓库是由 [wodetiannacao/FocuSD-super](https://github.com/wodetiannacao/FocuSD-super) 维护的独立 Fork，
> 保留原项目归属，并在 v0.1.6 基础上继续完善到 v0.1.7。

## 目录

- [关于项目](#关于项目)
- [核心功能](#核心功能)
- [相对原项目的改进](#相对原项目的改进)
- [技术栈](#技术栈)
- [快速开始](#快速开始)
- [使用说明](#使用说明)
- [AI Agent 状态指示灯](#ai-agent-状态指示灯)
- [数据与存储](#数据与存储)
- [项目结构](#项目结构)
- [路线图](#路线图)
- [参与贡献](#参与贡献)
- [许可证](#许可证)
- [致谢](#致谢)

## 关于项目

FocuSD Island 是一个 Windows 优先的桌面效率工具。它以透明、无边框、始终置顶的 Tauri 悬浮岛形式运行，平时停靠在主屏幕顶部，需要时展开为一个紧凑的工作面板。

如果你正在寻找一个适合 Windows 桌面的「灵动岛」工具，FocuSD Island 的目标不是做一个装饰组件，而是把每天最常用、最容易打断注意力的入口收进一个小岛里：今日待办、每日笔记、AI 编程状态、剪切板历史、媒体控制和外观设置。它也适合正在搜索 windows灵动岛、灵动岛 或 Windows Dynamic Island 替代方案的用户。

项目当前处于早期 MVP 阶段，优先适配 Windows。欢迎通过 Issue 和 PR 一起完善它。

## 核心功能

| 功能 | 当前版本说明 |
| --- | --- |
| Windows 灵动岛 | 透明、无边框、始终置顶，支持折叠、展开、边缘收起和托盘隐藏。 |
| 今日待办 | 新增、编辑、完成、删除任务，并可设置当前专注任务。 |
| 任务延续与归档 | 支持未完成任务延续到下一天，并自动归档历史待办和每日笔记。 |
| Markdown 保存 | 将当天内容保存为 YYYY-MM-DD.md，方便接入本地笔记流。 |
| AI Agent 状态 | 同时支持 Codex 与 Claude Code 的运行、完成、失败和过期状态。 |
| 多实例状态灯 | 多个 Agent 会话分别显示为 codex(1)、codex(2)、claude(1) 等独立状态。 |
| Hook 自动安装/升级 | 自动写入 app-data 脚本，修复旧版内联 Hook，并兼容旧状态文件和旧 marker。 |
| Agent 状态面板 | 查看会话、提供商、阶段和 Codex 会话标题，并可单独清除状态。 |
| 剪切板历史 | 记录文本和图片剪切板内容，支持收藏、复制、删除和清空。 |
| 媒体控制 | 查看系统音频状态，控制播放/暂停、上一首、下一首。 |
| 自由定位 | 支持将悬浮岛拖动到自定义位置，并持久化窗口位置。 |
| 外观设置 | 调整透明度、缩放、顶部间距、主题色，并保存样式预设。 |
| 系统集成 | 支持系统托盘菜单和 Windows 当前用户开机自启动。 |

## 相对原项目的改进

当前版本基于原项目 v0.1.6 继续开发，主要改进如下：

| 模块 | 当前版本改进 | 实际效果 |
| --- | --- | --- |
| Agent 状态模型 | 从单一 Codex 状态扩展为 Codex/Claude Code 多实例 instances 模型 | 多个窗口同时运行时不会互相覆盖状态 |
| 会话识别 | 使用 session_id 和 turn_id 配对运行事件 | 中断后继续对话不会生成重复灯或幽灵状态 |
| Hook 入口 | 新增统一 focusd-agent-hook.cmd，集中解析 stdin 并转发状态 | 避免长段内联 PowerShell 在 Codex 中退出码异常 |
| Hook 升级 | 检测旧版 Hook，自动重写为当前 app-data 脚本路径 | 安装新版后无需手动编辑旧配置 |
| Hook 兼容 | 兼容旧版单对象状态文件、旧 marker 和旧配置 | 升级时尽量保留已有用户状态 |
| 状态展示 | 增加完成保留时间、失败/过期提醒、单实例清除 | 可以区分已完成、仍运行和需要处理的会话 |
| Codex 信息 | 按 session_id 读取 Codex 会话标题 | Agent 面板中能看到更明确的任务名称 |
| 窗口布局 | 增加自由定位和位置持久化 | 悬浮岛不再只能固定在顶部居中 |
| 工程质量 | 增加多实例 Hook 集成测试、PowerShell 解析检查和构建验证 | 后续升级 Hook 时有回归保护 |

## AI Agent 状态指示灯

当前版本支持 Codex 和 Claude Code。状态灯会按会话分别显示：

- 红色：会话正在运行。
- 绿色：会话已完成或当前没有活动任务。
- 黄色：会话失败、可能中断，或运行 marker 超过 10 分钟未结束。
- 多个灯：表示多个 Agent 会话正在同时运行。

### Hook 安装与配置

在 FocuSD Island 中打开 设置 → AI Agent 状态灯，点击 安装/修复。安装器会：

1. 将当前版本的 Hook 脚本写入 %APPDATA%\com.focusd.island\。
2. 更新 %USERPROFILE%\.codex\config.toml。
3. 更新 %USERPROFILE%\.claude\settings.json。
4. 自动识别并升级旧版 Hook 配置。
5. 保留旧状态文件和旧 marker 的兼容读取。

### Codex Hook 信任

1. 点击 安装/修复。
2. 重启 Codex，或新开一个 Codex 任务。
3. 在 Codex 中打开 设置 → Hooks；CLI 中可输入 /hooks。
4. 找到 Updating FocuSD agent status 的 UserPromptSubmit 和 Stop Hook。
5. 确认路径位于 %APPDATA%\com.focusd.island\，然后点击 审核并信任 / Trust。

### Claude Code

Claude Code 使用 Windows exec 形式调用 cmd.exe，并通过 args 传递参数，适合 VSCode 终端和不同 Shell 环境。安装器会同时写入：

%USERPROFILE%\.claude\settings.json

### 状态数据

状态文件位于：

%APPDATA%\com.focusd.island\agent-status.json

当前格式以 instances 数组保存每个会话的：

- id
- provider
- displayIndex
- phase
- taskId
- updatedAt

## 技术栈

- [Tauri 2](https://tauri.app/)：桌面应用外壳与原生能力
- [React 19](https://react.dev/)：前端界面
- [Vite 7](https://vite.dev/)：前端开发与构建
- [TypeScript](https://www.typescriptlang.org/)：类型约束
- [Rust](https://www.rust-lang.org/)：窗口定位、托盘、文件写入、媒体控制和 Windows API 集成
- [lucide-react](https://lucide.dev/)：界面图标

## 快速开始

FocuSD Island 支持两种使用方式：直接下载 Release，或者通过源码自行构建。

### 方式一：通过 Release 安装

适合只想直接使用应用的用户。

1. 打开本仓库的 [GitHub Releases](https://github.com/wodetiannacao/FocuSD-super/releases/latest) 页面。
2. 下载最新版本的 Windows 安装包。
3. 推荐优先下载 `FocuSD Island_版本号_x64-setup.exe`。
4. 双击安装包，按提示完成安装。
5. 首次启动后，可在设置中配置 Markdown 保存目录、开机自启动、Codex 状态指示灯、剪切板历史和样式预设。

如果 Release 页面暂时没有安装包，可以使用下面的源码构建方式。

### 方式二：通过源码构建

适合想参与开发、自己构建可执行文件，或暂时没有可用 Release 包的用户。

#### 环境要求

- Windows 10 / Windows 11
- Node.js
- pnpm
- Rust / Cargo
- Microsoft Visual Studio Build Tools，并安装 C++ 工作负载
- Microsoft Edge WebView2 Runtime

#### 构建步骤

```powershell
git clone https://github.com/wodetiannacao/FocuSD-super.git
cd FocuSD-super
pnpm install
pnpm tauri build
```

构建完成后，安装包通常位于：

```text
src-tauri/target/release/bundle/nsis/FocuSD Island_0.1.7_x64-setup.exe
```

原始可执行文件通常位于：

```text
src-tauri/target/release/focusd-island.exe
```

如果只想生成 release 可执行文件、不生成安装包，可以运行：

```powershell
pnpm tauri build --no-bundle
```

开发模式：

```powershell
pnpm tauri dev
```

只启动前端开发服务器：

```powershell
pnpm dev
```

### 常用命令

| 命令 | 说明 |
| --- | --- |
| `pnpm install` | 安装前端和 Tauri CLI 依赖 |
| `pnpm dev` | 启动 Vite 前端开发服务器 |
| `pnpm build` | TypeScript 检查并构建前端 |
| `pnpm preview` | 预览前端构建产物 |
| `pnpm tauri dev` | 启动 Tauri 桌面开发模式 |
| `pnpm tauri build` | 构建 Tauri 桌面应用和安装包 |
| `pnpm tauri build --no-bundle` | 仅生成 release 可执行文件 |

## 使用说明

### 待办与每日笔记

- 在展开面板中添加今日待办，并将最重要的一条设为当前专注任务。
- 每日笔记适合记录当天补充信息、临时想法或任务背景。
- 跨天后，上一天内容会进入归档，方便回顾。
- 如需本地保存，可在设置中选择 Markdown 保存目录。

默认 Todo 保存路径为：

```text
%USERPROFILE%\Documents\FocuSD
```

### 剪切板

- 开启剪切板历史后，应用会记录文本和图片剪切板内容。
- 每条记录支持收藏、复制、删除。
- 收藏内容会保留在收藏栏目中，适合保存高频片段。

### AI Agent 状态

- 在设置中安装或修复 Codex/Claude Code Hook。
- 状态文件位于 %APPDATA%\com.focusd.island\agent-status.json。
- 每个会话独立保存，支持运行、完成、失败、过期和清除。
- Codex 会话可显示来自 session_index.jsonl 的任务标题。
- 完成状态的保留时间可以在 Agent 设置中调整。

## 数据与存储
- 待办、每日笔记、归档、外观设置等前端状态默认保存在 `localStorage`。
- 配置保存目录后，今日内容可写入本地 Markdown 文件，文件名为 `YYYY-MM-DD.md`。
- 剪切板历史、AI Agent 状态、Hook 脚本等原生侧数据保存在应用数据目录。
- 开机自启动使用 Windows 当前用户注册表路径：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`。

## 项目结构

```text
.
├── src/                    # React 前端
│   ├── App.tsx             # 核心 UI、状态和 Tauri invoke 调用
│   ├── App.css             # 主要样式
│   └── main.tsx            # React 入口
├── src-tauri/              # Tauri / Rust 桌面端
│   ├── src/lib.rs          # 原生命令、窗口定位、托盘、媒体和文件保存
│   ├── src/clipboard_history.rs
│   ├── src/main.rs         # Tauri 应用入口
│   ├── capabilities/       # Tauri 权限能力配置
│   └── tauri.conf.json     # Tauri 配置
├── scripts/                # Codex/Claude Code Hook 脚本和测试
├── docs/                   # 补充文档
├── package.json
└── README.md
```

## 路线图

- [ ] 开发并适配 macOS 版本
- [ ] 完善安装包发布流程和自动更新能力
- [ ] 增强多显示器定位策略
- [ ] 增加更完整的快捷键与键盘工作流
- [ ] 扩展任务分类、标签和筛选能力
- [ ] 增加数据导入、导出和同步方案
- [ ] 优化剪切板历史、媒体控制和 Codex 状态指示灯体验

如果你有更适合 Windows 灵动岛、桌面效率工具或 AI 编程工作流的想法，欢迎在 Issue 中讨论。

## 参与贡献

欢迎提交 Issue 和 Pull Request。

1. Fork 本仓库。
2. 创建你的功能分支：`git checkout -b feature/amazing-feature`。
3. 提交改动：`git commit -m "Add amazing feature"`。
4. 推送分支：`git push origin feature/amazing-feature`。
5. 发起 Pull Request。

提交 Issue 时，建议说明：

- 系统版本
- 应用版本
- 复现步骤
- 预期行为
- 实际行为
- 截图或录屏

提交 PR 时，建议保持改动范围清晰，并在说明中写明验证过的命令。

## 许可证

当前仓库暂未声明开源许可证。如需公开分发或协作复用，建议补充 `LICENSE` 文件。

## 致谢

- README 结构参考 [Best-README-Template](https://github.com/othneildrew/Best-README-Template)
- 感谢所有通过 Issue、PR 和反馈帮助改进 FocuSD Island 的用户
