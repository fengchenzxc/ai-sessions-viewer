<div align="center">

# Claude Session Viewer

[English](README.en.md) · **中文** · [日本語](README.ja.md) · [CHANGELOG](CHANGELOG.md)

<p align="center">一个专为 <strong>Claude Code</strong>、<strong>Codex</strong> 和 <strong>Gemini CLI</strong> 打造的原生桌面浏览器。在一处读取、搜索并管理三个 CLI 的本地会话记录。</p>

<p align="center">
<strong>忠实还原</strong> — 完整呈现思考链路、工具调用配对、结构化 Diff 与内嵌截图。<br/>
<strong>高效检索</strong> — 跨项目全局秒搜（<strong>⌘⇧F</strong>）直达具体消息，支持一键恢复终端会话。<br/>
<strong>深度统计</strong> — 基于本地记录聚合 Token 消耗与成本，多维分析（项目/模型/工具调用）活跃度与开销。<br/>
<strong>只读安全</strong> — 原始 JSONL 全程只读，删除仅移动至共享回收站，绝不物理抹除（<code>rm</code>）。<br/>
<strong>灵活导出</strong> — 单会话或批量导出为离线可读的 Markdown / HTML。
</p>

[![Tauri 2](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=fff)](https://tauri.app)
[![Vue 3](https://img.shields.io/badge/Vue-3-42b883?logo=vue.js&logoColor=fff)](https://vuejs.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<br />

<img src="docs/screenshots/cover.png" alt="Claude Session Viewer — Claude Code、Codex、Gemini CLI 会话统一浏览器" width="820" />

</div>

---

## 起因

Claude Code、Codex 和 Gemini CLI 各自把会话 JSONL 写到磁盘，但目录结构和入口命令各不相同，三者都没有可视化的浏览器。这个应用把三边合并成同一条时间线：

| Agent | 路径 | 分组方式 |
| --- | --- | --- |
| Claude | `~/.claude/projects/<目录>/<sessionId>.jsonl` | 按项目目录 |
| Codex | `~/.codex/sessions/<年>/<月>/<日>/rollout-*.jsonl` | 按文件里记录的 `cwd` |
| Gemini | `~/.gemini/tmp/<slug>/chats/session-*.jsonl` | 按 `slug`；cwd 从同目录 `.project_root` 读取 |

应用对原始文件**只读**——删除其实是软移入 `~/.claude/.session-viewer-trash/`，不会 `rm`。

## 本分支相对原版的改动

本仓库维护在 [fengchenzxc/ai-sessions-viewer](https://github.com/fengchenzxc/ai-sessions-viewer)，保留原来的 Tauri + Vue + Rust 架构。相比原版，主要增加了：

- **对齐 Codex 桌面端元数据** —— Codex 会话会结合 Codex app 的线程 metadata 判断，包括 app 列表 rank、首屏位置、归档状态，以及 guardian / auto-review 内部子线程。
- **Codex 过滤与标记** —— 设置里可以分别控制是否展示 Codex 内部审核会话、已归档会话；会话列表会单独标记“审核会话”和“已归档会话”。
- **终端偏好** —— 恢复会话可选择 Warp、Terminal.app、iTerm2。现在使用临时可执行脚本启动，不再依赖 AppleScript 自动输入，也不需要辅助功能权限。
- **更安全的临时脚本** —— 在项目目录创建隐藏 `.tmpXXXXXX` 脚本，执行 `codex resume <session_id>`，运行后自清理，并由后端延迟清理兜底。
- **Codex 风格主题与字号** —— 增加 Codex 浅色主题、Dracula 深色主题，启用 macOS 字体平滑，UI 基准字号 14px，代码 / diff 基准字号 12px。
- **统计与解析增强** —— 扩展 Codex/Gemini 解析、价格和模型归一化、工具/活动分类、搜索取消逻辑，并补充回归测试。
- **仓库迁移与自动发布** —— Release / 更新检查链接切到 [fengchenzxc/ai-sessions-viewer](https://github.com/fengchenzxc/ai-sessions-viewer)，GitHub Actions 会在推送 tag 时自动打包并发布安装包。

## 功能

- 🗂 **统一项目视图** —— 跨 CLI 按工作目录归并会话
- 💬 **聊天式还原** —— 文本、思考块、工具调用、结构化 diff、内嵌图片
- 🔎 **会话内搜索 + 范围筛选** —— 可全局搜，也能只搜用户消息 / 助手回复（含改动）/ 工具调用噪音；带上一/下一跳转与计数
- 🌐 **全局搜索（⌘⇧F / Ctrl+Shift+F）** —— Algolia 风格浮层，仅作用于当前 agent，匹配会话标题与你发出的消息；点击命中项直接跳到对应消息并闪烁；最近搜索支持单条删除
- 🔃 **会话列表搜索与排序** —— 关键词搜索走 Rust 后端，命中会话标题 + 用户消息正文（输入新字符即取消上一次搜索）；按时间 / 体积 / 消息数排序，或只看带 ID 的
- 🪗 **一键折叠/展开所有工具调用** —— 隐去工具噪音，聚焦对话主线
- 📤 **导出会话** —— 单会话导出为 Markdown 或 HTML（原生另存为，HTML 内联头像与样式，可离线打开）
- 🧰 **多选与批量操作** —— 批量选会话后一次移入回收站，或导出到一个 `export-YYYYMMDD-HHMMSS-{md,html}/` 文件夹
- 🔄 **恢复或新建会话** —— 在项目目录打开 Terminal，恢复已有会话（`claude --resume <id>` / `codex resume <id>`）或直接新建一个
- 📡 **实时 tail** —— 打开的会话会随 CLI 写入自动追加新消息，顶栏会亮 "● Live" 指示，滚到上面时新增内容会聚合成 "N 条新 ↓" 气泡提示
- 🗑 **共享回收站** —— 软删除、可预览已删会话的完整记录、单条或批量还原（多选）；两 agent 共用一个 trash
- 🏠 **欢迎页** —— 按 agent 列出最近打开过的项目，一键再进入，每条单独可删
- 📌 **置顶 / 沉底** —— 侧栏带色彩标识的小圆点；沉底项目自动落到列表底
- ✏️ **重命名会话** —— 改的名字会同步回 CLI，`claude` / `codex` 自带的 resume 选择器里也能看到
- 🌗 **浅色 / 深色 / 跟随系统** —— Codex 风格中性灰色调，仅 brand 色用于强调
- 🌐 **多语言 + 自动适配** —— 简体中文 / 繁體中文 / English / 日本語；首次启动按系统语言自动选择，匹配不到回退到英文
- ⚡️ **自定义 tooltip 与 agent 品牌图标** —— 杜绝突兀的系统原生气泡
- 🖼 **图片 lightbox** —— 查看会话中嵌入的截图

## 安装

### 预构建版本

到 [Releases](https://github.com/fengchenzxc/ai-sessions-viewer/releases) 下载：

| 平台 | 文件 |
| --- | --- |
| macOS (Apple Silicon + Intel) | `ai-sessions-viewer_<ver>_universal.dmg` |
| Windows x64 | `ai-sessions-viewer_<ver>_x64-setup.exe` |
| Linux x86_64 (Debian/Ubuntu) | `ai-sessions-viewer_<ver>_amd64.deb` |
| Linux x86_64 (便携) | `ai-sessions-viewer_<ver>_amd64.AppImage` |

macOS 上 `.app` 是 **ad-hoc 签名、未公证**，首次打开可能弹出「Apple 无法验证…」。两种绕过方式：

- Finder 里右键应用 → **打开** → 弹窗里再确认（一次即可）。
- 或在终端清掉隔离属性：
  ```bash
  sudo xattr -dr com.apple.quarantine /Applications/ai-sessions-viewer.app
  ```

Linux 上 `.AppImage` 是便携格式 —— `chmod +x` 后直接运行。`.deb` 安装：
```bash
sudo apt install ./ai-sessions-viewer_<ver>_amd64.deb
```

### 从源码构建

依赖：**Node 20+**、**Rust stable**，以及对应平台的工具链：
- **macOS** —— Xcode CLT。
- **Windows** —— MSVC + WebView2。
- **Linux** —— `libwebkit2gtk-4.1-dev`、`libappindicator3-dev`、`librsvg2-dev`、`libxdo-dev`、`patchelf`（Debian/Ubuntu：`sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev libxdo-dev patchelf`）。

```bash
git clone https://github.com/fengchenzxc/ai-sessions-viewer.git
cd ai-sessions-viewer
npm install
npm run tauri dev          # 开发模式
npm run tauri build        # 打包 .app / .dmg / .msi / .deb / .AppImage
```

`npm run build` 是类型检查步骤（`vue-tsc --noEmit` + Vite 构建）。单元测试放在 `test/` 下，跑在 Vitest 上 —— `npm test` watch 模式，`npm run test:run` 单次跑 CI，`npm run test:coverage` 出 v8 覆盖率报告。

## 使用

1. **切换 agent** —— 侧栏顶部分段控件（Claude 🟠 / Codex 🟢）
2. **选项目** —— 侧栏列出所有工作目录；右键可置顶 / 沉底 / 重命名
3. **打开会话** —— 中间栏渲染消息和工具调用，调用 → 结果会自动配对
4. **恢复** —— 工具栏 ▶ 按钮唤起 Terminal 并执行对应 CLI
5. **导出** —— 详情页工具栏 ⬇ 单条导出 Markdown / HTML；会话列表多选后顶栏 ⬇ 可一次性批量导出到 `export-YYYYMMDD-HHMMSS-{md,html}/` 文件夹
6. **删除 / 恢复** —— 工具栏 🗑 软删除；顶栏垃圾桶图标进入回收站

## 部分截图

<table>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/cover.png" alt="主视图 — 侧栏、会话与聊天" />
      <p align="center"><em>主视图 — 侧栏、会话列表与聊天，一键导出</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/chat.png" alt="忠实还原 — 思考、工具调用、结构化 Diff" />
      <p align="center"><em>忠实还原 — 思考、工具调用、结构化 Diff、实时跟随</em></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/search.png" alt="全局搜索浮层" />
      <p align="center"><em>全局搜索（⌘⇧F）直达目标消息</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/stats.png" alt="Token 与成本统计面板" />
      <p align="center"><em>按项目 · 模型 · 工具维度分析 Token 与成本</em></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/screenshots/export.png" alt="浏览器中预览导出的 HTML" />
      <p align="center"><em>导出 HTML — 完全离线，浏览器直接打开</em></p>
    </td>
    <td width="50%">
      <img src="docs/screenshots/trash.png" alt="共享回收站与恢复" />
      <p align="center"><em>共享回收站 — 软删除，一键恢复</em></p>
    </td>
  </tr>
</table>

## 技术栈

- **前端** —— Vue 3 + Vite + Tailwind CSS v4（CSS 变量式 design tokens）
- **后端** —— Rust + Tauri 2；每个 agent 的 JSONL 解析通过 `SessionSource` trait 隔离在 `src-tauri/src/agents/<agent>.rs`
- **JSONL 解析** —— 全部放在 Rust 侧，前端不直接读盘
- **图标** —— [iconify](https://iconify.design)（`lucide` / `material-icon-theme` / `arcticons`）编译期内联
- **没有 store** —— 状态住在 `App.vue` 的 ref 里，`localStorage` 仅保存语言 / 主题 / pin 偏好

贡献者可参考 [`CLAUDE.md`](CLAUDE.md)（架构笔记）和 [`docs/release-ci.md`](docs/release-ci.md)（发布流程）。

## 贡献

欢迎 PR。请使用 [Conventional Commits](https://www.conventionalcommits.org/)（`feat:` / `fix:` / `docs:` ...）—— `release-please` 会基于提交信息自动 bump 版本并更新 [`CHANGELOG.md`](CHANGELOG.md)。

## License

[MIT](LICENSE) © jerrywu001
