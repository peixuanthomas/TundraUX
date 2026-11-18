# TundraUX

**在终端里管理设备的集成工作区。**

[English](../README.md) | 简体中文

TundraUX 把系统工具、文件和日常应用放在同一个终端工作区中。它主要面向 Linux 设备，包括没有图形桌面的设备，也可以在 macOS 和 Windows 上运行。部分系统管理功能仅在 Linux 上提供。

你可以使用键盘、鼠标或触摸屏操作。

## 首页

选择图标即可打开应用、设置或系统工具。

![TundraUX 首页](screenshots/Main_menu.png)

## 文件管理

浏览、搜索和整理文件与文件夹。

![文件管理器](screenshots/Explorer.png)

## 分屏

在一个工作区里同时打开和使用多个应用。

![分屏显示编辑器和命令行](screenshots/Split_View.png)

## 计划任务

在 Linux 上查看和管理计划任务。

![计划任务](screenshots/Tasks.png)

## 时钟

查看时间，使用闹钟和倒计时。

![时钟、闹钟和倒计时](screenshots/Clock.png)

## 从源码构建

在项目目录中运行：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
cargo run --locked -p shell --bin tundra-shell
```

Windows 没有 `python3` 命令时可使用 `python`。

所需环境见[构建指南](README-TECHNICAL.md#building-and-running)。

## 了解更多

[下载 TundraUX](https://github.com/peixuanthomas/TundraUX/releases) · [文档](README.md)

## 许可证

项目代码采用 [GPL-3.0-only](../LICENSE)。随包第三方组件保留各自的许可证。
