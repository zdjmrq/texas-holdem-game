# 原生界面与核心收尾报告（2026-10-23）

分支：codex/rust-ui-completion，基于 rust-rewrite 的 33d3afc。原版 main / D:\德州扑克\repo（e0c6c6f）仅作只读对照。

## 实现

- 保持 Rust + egui。恢复原版红金标题、雾蓝几何背景、绿色渐变台呢、104px 座位、顶部七座与侧边两座布局、蓝色斜纹牌背、emoji 头像、底部手牌和操作区、居中开局面板、双列档位设置与金红滑块。采用整页滚动，避免固定底栏裁切牌桌。
- 原生联机页接入 poker-client：独立 Tokio 线程、连接/大厅/房间/牌桌/结算准备、重连、恢复凭据、动作确认、旧状态去重、超时提示、快捷键、隐藏手牌和分析遮蔽。删除旧 Noop 控制器。
- 修复内嵌服务在发布 URL 后被同步 recv 阻塞的问题；增加真实连接/创建房间回归测试。
- 服务端默认真实 poker-ai，局内计算在 spawn_blocking；本地通过专用 FIFO 后台线程计算。公开行动和实际亮牌摊牌信息进入所有长期 AI 模型。
- 修正加注收益上限漏加自身投入、终局跟注候选选择、单一候选不应消耗混合频次等移植差异。
- 5/6/7 张完整手牌都使用查表；部分手牌仍保留 reference 路径。73,775 个点数直方图与随机差分测试覆盖。

## 深度 AI（用户追加要求）

生产路径统一采用 20,000 次 Monte Carlo 采样；标准德州符合条件的单挑河牌采用 8,000 次 CFR 迭代，并扩大到原参考桌型之外。原版对拍路径保留参考预算，以单独检验移植正确性。

固定翻牌场景，补全真实底牌、预热查表后：400 次采样标准误差 0.0246，20,000 次为 0.0035，约降低到 1/7；调试构建耗时约 1.1ms / 63ms。8,000 次河牌求解约 2.18s（WSL 调试构建，不能当作发布版帧率）。这验证采样精度和求解预算增加，不能据此宣称长期 BB/100 已提高。

本地交互 tick 不等待后台结果；独立回归测试用阻塞策略验证 UI 侧可立即返回并在结果到达后推进。原版 AI 行动节奏滑块仍只控制展示节奏。

## 行为对齐与验证

参考预算黄金决策：标准 174/180（96.7%）、短牌 82/82（100%），总计 256/262（97.7%）；金额一致 254/262，RNG 调用一致 259/262。标准动作 >=95% 纳入默认测试门槛。11 个标准轨迹决策仍有 2 个差异，不宣称逐手完全一致。深度生产模式允许作出不同决策。

- cargo test --workspace --offline：238 项通过（含深度采样、8,000 轮 CFR、后台不阻塞测试）。
- cargo clippy --workspace --all-targets --offline -- -D warnings：通过；cargo fmt --all --check：通过。
- GOLDEN_AI_STRICT=1 的黄金决策测试：通过；标准 >=95% 默认门槛同时通过。
- Windows 本机 MSVC + Windows SDK 原生 release 构建：通过。
- 从实际便携目录运行并截图：home / table / table-flop / table-shortdeck / settlement / online / settings，以及 900x600 compact，均正常退出；截图见 screenshots/。
- 原生音频探针：MP3 解码 44100Hz 双声道、可打开 48000Hz 双声道输出设备。没有将设备可打开等同于人工听音通过。

Windows 原生发布包：dist/texas-holdem-rs-native-x64/texas-holdem-rs.exe，解压 zip 时保留同目录 assets。
本机复现构建：先运行 D:\Tools\VSBuildTools\Common7\Tools\VsDevCmd.bat -arch=x64 -host_arch=x64，再运行 cargo +1.95 build --release -p poker-app --offline。

## 仓库体积

接手时已跟踪源码 crates 约 1.37MiB，字体/音乐资源约 22.68MiB，测试数据约 8.89MiB，文档约 3.34MiB。target、dist 不入 Git。新分支沿用验证过的核心，并不需要从零重写；只取当前分支可使用 --single-branch --branch rust-rewrite --depth 1。新工作分支尚未发布时，远端该命令取到的是旧基线。

## 验收边界

原生重画不等于像素级一致：字体渲染、发牌/翻牌动画、阴影及部分窗口控件仍有差别。中文 IME 组合输入、托盘/任务栏闪烁、长时间帧率和资源占用、真实多机网络仍需要人工验收，不因截图成功而标记通过。

初始的 oracle/interop-rust-rust.sh 与 scripts/verify-all.sh 本地修改保留，本轮没有覆盖其内容。

## 发布包校验

- Windows x64 MSVC 主程序约 22.7MiB；zip 33,906,828 字节（约 32.3MiB）。
- exe SHA256：2AE12AE9463689BC7C9E1E84B833FAE0F60C5EC3147B780BC5A5EEC450BEE01C。
- 小窗口顶部工具栏使用悬浮显示；900x600 截图已用最终包复核。
