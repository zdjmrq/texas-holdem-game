# JS 基线（v1.5.8, commit e0c6c6f）

在 WSL (Ubuntu 24.04, Node v24.21.0) 上验证：

- `npm ci --omit=dev`：1 package（ws 8.21.3），0 vulnerabilities
- `npm test`：**152 tests / 152 pass / 0 fail**，1.70s
- 基准数据：`benchmarks/v1.5.8` 共 159 个文件（12 MB）
- 音乐资源 4 个 mp3（20 MB），牌面由 CSS/DOM 绘制（无位图牌面资源）

原始输出保存于 `docs/baselines/js-v1.5.8-npm-test.txt`。
