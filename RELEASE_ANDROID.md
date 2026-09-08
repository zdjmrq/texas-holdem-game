# 德州扑克 Android v1.5.0

下载：[Android 安装包与校验文件](https://github.com/zdjmrq/texas-holdem-game/releases/tag/v1.5.0-android.1)。

- 系统要求：Android 8.0 及以上，使用系统 Android WebView。
- 包名：`com.zdjmrq.texasholdem`，版本号 `1.5.0`，版本代码 `10500`。
- Release APK 使用独立发布密钥签名，包含游戏页面、AI/概率后台线程和四首音乐，可离线进行本地 AI 对战。
- 手机默认横屏；保留原网页布局，较小屏幕需要上下滚动查看牌桌和操作区，尚未进行完整手机界面重设计。Android 16 大屏设备可能按系统规则允许其他方向。
- 下载 APK 后在手机上打开，并按系统提示允许此次安装来源。

## 联网

Android APK 不内置 Node.js 服务端，也不会自动在手机上启动房间服务器。

在电脑上安装 Node.js，在项目根目录执行 `npm ci`。PowerShell 启动局域网服务：

```powershell
$env:POKER_HOST = '0.0.0.0'
npm run server
```

手机和电脑连接同一网络，联网页面填写 `ws://电脑局域网IP:3000`，电脑防火墙须允许该端口。公网服务可填写 `wss://服务器域名`。如果服务端设置了 `POKER_ALLOWED_ORIGINS`，需包含 `http://appassets.androidplatform.net`。APK 的该地址由 WebViewAssetLoader 从本地资源响应，并非外部游戏服务器。

## 从源码构建

环境：JDK 17 或 21、Android SDK Platform 36 / Build Tools 36.0.0；Gradle Wrapper 固定为 8.14.3，Android Gradle Plugin 为 8.13.0。

1. 配置 `ANDROID_HOME`，或在未跟踪的 `android/local.properties` 中写入 SDK 路径。Windows 示例：`sdk.dir=D\:/Android/sdk`。
2. 项目根目录运行 `npm ci`、`npm test`。
3. 执行 `cd android`，再执行 `./gradlew assembleDebug lintDebug`（Windows 使用 `./gradlew.bat`）。
4. 调试 APK 位于 `android/app/build/outputs/apk/debug/app-debug.apk`。

Gradle 在每次构建前自动同步根目录的 `index.html`、`js/`、`css/` 和 `assets/`，无需手工复制网页。

签名发布版本：在仓库外创建如下 properties 文件，使用环境变量 `POKER_SIGNING_PROPERTIES` 指向它，然后执行 `./gradlew assembleRelease lintRelease`。未配置签名时只会生成未签名 release APK。

```properties
storeFile=/absolute/path/to/release.jks
storePassword=YOUR_PRIVATE_PASSWORD
keyAlias=texas-holdem
keyPassword=YOUR_PRIVATE_PASSWORD
```

正式 APK 位于 `android/app/build/outputs/apk/release/app-release.apk`。请保管发布密钥及密码，后续覆盖升级必须使用同一签名并增加版本代码。密钥与密码不提交到 GitHub。

## 本次验证

- 36 项 Node.js 自动测试通过。
- `assembleRelease`、`lintRelease` 通过（0 错误；保留 Gradle 版本提示、横屏策略等提示）。
- Android `apksigner verify` 验证签名成功。
- 浏览器 915 × 412 横屏尺寸验证开局、手牌展示、行动按钮与概率结果。
- 未连接安卓真机或运行安卓模拟器；系统 WebView 下的安装、后台恢复、声音和设备兼容性仍需真机验证。
