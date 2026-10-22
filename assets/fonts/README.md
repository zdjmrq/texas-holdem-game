# assets/fonts

Phase 0 spike 没有提交字体二进制文件：Linux/WSL 用系统 Noto Sans CJK 验证，代码里也保留了
system font fallback。

发布版（Phase 6/7）建议在此放一个经过裁剪的简体中文字体，例如：

    NotoSansSC-Regular.otf

生成方式（Ubuntu，需 python3-fonttools；从系统 fonts-noto-cjk 的 TTC 中取 SC face 并裁剪）：

    # 1) 构造字符集：ASCII + GB2312 常用汉字 + 全角标点 + 常用符号/花色
    python3 - <<'PY'
    chars = set(chr(c) for c in range(0x20, 0x7F))
    for b1 in range(0xA1, 0xFF):
        for b2 in range(0xA1, 0xFF):
            try:
                chars.add(bytes([b1, b2]).decode('gb2312'))
            except UnicodeDecodeError:
                pass
    for c in range(0x3000, 0x3040):
        chars.add(chr(c))
    for c in range(0xFF00, 0xFFF0):
        chars.add(chr(c))
    for c in [0x2660, 0x2661, 0x2662, 0x2663, 0x2665, 0x2666, 0x2022, 0x00B7]:
        chars.add(chr(c))
    open('/tmp/gb2312_chars.txt', 'w', encoding='utf-8').write(''.join(sorted(chars)))
    print(len(chars), 'chars')
    PY

    # 2) 裁剪（实测 7,735 字符 -> 3,183,992 字节）
    python3 -m fontTools.subset /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
        --font-number=2 \
        --text-file=/tmp/gb2312_chars.txt \
        --output-file=assets/fonts/NotoSansSC-Regular.otf \
        --layout-features="*" --no-hinting --desubroutinize

体积参考（2026-10 实测）：

- 系统 NotoSansCJK-Regular.ttc（5 个 face）：19,484,784 B
- 上述 GB2312 子集：3,183,992 B（保留 layout features）/ 3,093,432 B（极简）
- 完整 Noto Sans SC OTF：约 10MB

代码里对应的加载路径见 crates/poker-app/src/fonts.rs：
- 运行期从本目录读取（--font-mode bundled 或默认打包模式）；
- 后续可改为 include_bytes! / rust-embed 打进单文件 portable exe。

许可：Noto Sans CJK / Noto Sans SC 为 SIL Open Font License 1.1（OFL），可随应用分发；
打包时需要保留版权与许可声明（可放在关于页/第三方许可文件）。

历史/来源：本 spike 使用 Ubuntu fonts-noto-cjk 1:20230817+repack1-3 的字体文件做体积与渲染验证。
