# TundraUX

**在终端里管理设备的集成工作区。**

[English](../README.md) | 简体中文

TundraUX 把系统工具、文件、编辑器和日常实用程序整合到一个终端界面中，主要面向没有图形桌面的 Linux 设备，支持键盘、鼠标和触摸屏。项目使用 Rust、Ratatui 和 crossterm 构建。

## 主要功能

- **应用更新：** Linux 正式版显示下载进度和更新记录。测试版可在 Git 树中选择远端提交，并查看本机编译输出。

- **Linux 系统管理：**管理服务、进程、软件包、网络、磁盘、用户、防火墙、SSH 和计划任务，查看配置历史并恢复更改。
- **跨平台软件包：**使用 Linux 原生包管理工具、macOS 的 Homebrew 或 Windows 的 WinGet 搜索、安装、卸载和升级软件。
- **文件与编辑：**浏览目录、跨页面复制与剪切文件、撤销同一文件系统内的移动和重命名、管理权限，使用语法着色、查找替换、软换行和 JSON 格式化。按 F8 在内置编辑器和 Vim 之间切换。
- **开发工具：**在内嵌终端中运行程序，使用断点、单步、变量和调用栈调试 Bash 脚本。
- **系统动态：**查看资源占用、日志和通知，跟踪后台任务及其输出。状态消息和时钟在顶部显示。
- **个人工作区：**按名称或命令搜索启动器、整理自定义项目，选择主题和语言，使用时钟与天气应用。
- **独立应用窗口：**每个窗口都有边框和最小化、分屏、关闭按钮，全屏收进分屏菜单，从底部窗口栏恢复后台应用，逐窗切分，最多显示 2×2 四宫格。布局变化时保留已显示的应用画面；新窗口等待超过 100ms 才显示启动提示。独立 watchdog 监测主界面和应用进程，应用无响应时仍能直接终止。
- **终端界面：**导航、列表选择、弹窗和加载反馈参照 OpenCode，搭配细滚动条和 TundraUX 的 Glacier Night 深蓝配色。

| 平台 | 主要支持 |
| --- | --- |
| Linux | 设备操作与系统管理 |
| macOS | 终端工作区与 Homebrew 软件包管理 |
| Windows | 终端工作区与 WinGet 软件包管理 |

## 快速开始

使用至少 **108 列 × 20 行**的交互式终端。鼠标和触摸输入需要终端转发指针事件。

工作区以当前系统用户启动，首页显示系统用户名。首次启动需要设置个人偏好。系统用户管理仅在 Linux 提供。

窗口上边缘提供 **最小化、分屏、关闭** 按钮。分屏菜单提供左右、上下和全屏：原程序留在左侧或上侧，新区域通过顶部搜索选择最小化窗口或启动新应用；取消会撤销本次切分。全屏会将其他分屏窗口最小化，并清除旧布局。**Ctrl+W，再按 v/s** 左右/上下切分，前缀后按 **h/j/k/l** 切换窗口；编辑器、Vim 和终端保留原生 Ctrl+W，使用窗口按钮分屏。最多支持 2×2 四宫格，无法使用的分屏选项会置灰。全应用的选择类弹窗均提供顶部搜索，右键与更多操作菜单保持紧凑。底部窗口栏名称用于恢复窗口，独立减号用于正常关闭。所有运行应用仍共同决定最小尺寸，退出时统一清理后台应用。

### 使用 Linux 便携包

1. 从 [GitHub Releases](https://github.com/peixuanthomas/TundraUX/releases) 下载并解压 Linux x86_64 压缩包。
2. 保持解压后的文件在同一目录，在该目录打开终端。
3. 安装 `tux` 命令，然后启动程序：

```sh
./install-tux.sh
tux
```

请保留解压后的目录。如果安装时提示，将 `~/.local/bin` 加入 `PATH`。

检查运行环境：

```sh
./tundra-cli doctor
```

日常使用普通用户运行。需要管理员权限的操作会在执行前请求授权。

### 从源码构建

安装 **Rust 1.95 或更新版本**。构建随包 Vim 还需要 **Python 3.12 或更新版本**和本机 C 构建工具链。

在仓库根目录执行：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
cargo run --locked -p shell --bin tundra-shell
```

Windows 没有 `python3` 命令时可使用 `python`。更新源码后同时重新构建 Shell 和 CLI；`packaging/vim.json` 变化时重新构建 Vim。

检查源码构建的运行环境：

```sh
cargo run --locked -p cli --bin tundra-cli -- doctor
```

## 配套 CLI

`tundra-cli` 提供诊断、配置、日志、系统管理命令和运行中界面的控制功能：

```sh
tundra-cli --help
tundra-cli doctor
tundra-cli watchdog list
```

需要从另一个终端控制界面时，先在**设置 → 系统 → 外部 CLI 控制**中开启该功能，然后使用：

```sh
tundra-cli ui snapshot
tundra-cli ui wake
```

点击、文字输入、滚动和自动化示例见 [CLI 控制指南](../crates/cli/docs/ui-control.md)。

新窗口复用 Shell 在启动时验证的语言资源和共享会话设置。运行 `tundra-cli logs query --module ux.workspace --format jsonl` 查看首帧接收和终端输出耗时；字段说明见 [Shell 启动计时](../crates/shell/README.md#app-startup-timings)，复现步骤见 [性能验收](testing.md#workspace-performance-acceptance)。

[2026-11-17 Linux 实测](performance-linux-2026-11-17.md)中，六个内置 AppHost 启动的 P95 均低于 100ms，单窗、双窗和四窗反馈的 P95 均低于 50ms，使用正常动画和独立进程。报告另列后加 debug panic 路由修复的功能验证。

## 文档

- [English README](../README.md)
- [构建、架构、测试与打包](README-TECHNICAL.md)
- [Linux 系统管理](../crates/platform/docs/linux-management.md)
- [编辑器与脚本调试](source-editor.md)
- [CLI 命令与运维操作](../crates/cli/docs/operations.md)
- [文档索引](README.md)

## 许可证

项目代码采用 [GPL-3.0-only](../LICENSE)。随包第三方组件保留各自的许可证。
