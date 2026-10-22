# scripts/

Windows 构建与冒烟辅助脚本（开发机为 Linux/WSL，最终验收在 Windows x64）。

## 文件

| 文件 | 作用 |
| --- | --- |
| build-windows.cmd | Windows 双击/命令行入口，调用 build-windows.ps1 |
| build-windows.ps1 | 原生 MSVC 构建并更新 dist/texas-holdem-rs-current/，不生成 zip |
| README.md | 本文件：Windows 构建环境准备与常见问题 |

## 一、Windows 构建环境准备（一次性）

### 1. Visual Studio 2022 Build Tools

安装 Visual Studio 2022 Build Tools（https://visualstudio.microsoft.com/visual-cpp-build-tools/），
勾选「使用 C++ 的桌面开发」（MSVC v143 + Windows 10/11 SDK）。不需要完整 Visual Studio。

验证（Developer PowerShell for VS 2022 或普通 PowerShell 均可）：

    link /?
    # 若提示找不到 link.exe，说明没在 VS 开发者环境或没装 C++ 工作负载

### 2. Rust 工具链（MSVC host）

安装 rustup（https://rustup.rs），确认 host 是 msvc：

    rustup toolchain install 1.95 --profile minimal --component rustfmt --component clippy
    rustc -Vv        # host: x86_64-pc-windows-msvc
    rustup target add x86_64-pc-windows-msvc

### 3. cargo 镜像（国内网络）

编辑 %USERPROFILE%\.cargo\config.toml（没有就新建）：

    [source.crates-io]
    replace-with = "rsproxy"

    [source.rsproxy]
    registry = "sparse+https://rsproxy.cn/index/"

    [net]
    git-fetch-with-cli = true
    retry = 2

    [http]
    timeout = 30

说明：static.crates.io 在部分网络下不可达（开发期已实测），rsproxy 同时镜像 index 与 crates
下载。不要用 cargo fetch --target ... 之类会拉全量依赖的命令；正常 cargo build 即可。

### 4. 获取代码

    git clone <仓库地址> texas-holdem-rs
    cd texas-holdem-rs

## 二、构建便携包

    scripts\\build-windows.cmd

或直接 PowerShell：

    powershell -NoProfile -ExecutionPolicy Bypass -File scripts\\build-windows.ps1

参数：

| 参数 | 说明 |
| --- | --- |
| -Configuration release | 构建配置（默认 release） |
| -SkipBuild | 跳过 cargo build，只重新打包（复用已有 exe） |
| -NoZip | 兼容旧命令；现在无论是否传入都不生成 zip |
| -Clean | 构建前将本项目 target 移入 Windows 回收站（会导致下次完整重编译） |

输出：

    dist/texas-holdem-rs-current/              # 唯一当前便携目录（exe + assets + README）

双击 texas-holdem-rs.exe，日常运行不要附加 --scenario 或 --seed。构建前关闭正在使用当前便携目录的游戏。脚本通过 vswhere 自动发现 MSVC（支持自定义安装盘符）；不把依赖重复编译到额外的 target triple 目录。

target 是可重建编译缓存，dist 是本地产物，两者均不上传 GitHub。工作结束后可将已不用的编译缓存移入回收站；保留当前程序、源码和运行资源，不自动清空全局 Cargo 缓存。

## 三、常见问题

| 现象 | 处理 |
| --- | --- |
| link.exe not found / LNK1104 | 没装 VS Build Tools C++ 工作负载，或没在 VS 开发者环境；重开 PowerShell 再试 |
| 下载失败/卡住 | 检查 %USERPROFILE%\.cargo\config.toml 是否已切 rsproxy；必要时重开终端 |
| error: package poker-app requires rustc 1.95 | rustup toolchain install 1.95 |
| x86_64-pc-windows-msvc target may not be installed | rustup target add x86_64-pc-windows-msvc |
| SmartScreen 拦截 exe | 右键属性 -> 解除锁定，或「更多信息 -> 仍要运行」（未签名的正常现象） |
| 托盘图标不出现 | 检查系统托盘溢出区（^）；Win11 设置里允许图标显示 |
| 无声音 | Windows 声音设置确认默认输出设备；应用内设置看音量/静音 |

## 四、开发机（Linux/WSL）能做什么

- cargo fmt/clippy/test、WSLg 截图、逻辑测试全部在 Linux 上做；
- cargo check -p poker-app --target x86_64-pc-windows-msvc 做 Windows 类型检查（不链接，
  不需要 MSVC 链接器）；
- 真正生成 exe 必须在 Windows 上跑 build-windows.ps1（不做 Linux 交叉编译）。
