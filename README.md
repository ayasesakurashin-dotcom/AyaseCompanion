# AyaseCompanion

独立开发的 Windows 桌面伙伴。Rust 宿主管理任务、规则插件、角色实例与配置，TypeScript / Canvas 呈现序列帧、文字气泡与音声。

**v0.2.0 是已完成人工实机验收的 P2 基线**，后续开发从该版本继续。v0.1.0 的 P0/P1 标签与发布包保留，制作工作台属于后续阶段。

v0.2.0 已接入统一人格、OpenAI 兼容聊天、Fish Audio 语音与独立记忆。按 [服务配置说明](docs/p2-user-guide.md) 配置服务；支持连接测试、快捷试听和从固定音声创建私有音色。模型完整生成后由本地动画显示，音声预缓冲后连续播放。P3 的本地 0.3.0 候选正在接受实机验证，新增能力使用方法见 [P3 指南](docs/p3-user-guide.md)；验收前不发布。

## 使用

在 [Releases](https://github.com/ayasesakurashin-dotcom/AyaseCompanion/releases) 下载 Windows x64 便携包，解压后运行 `AyaseCompanion.exe`。运行需要 Microsoft Edge WebView2 Runtime。

首次使用从托盘打开控制台，导入自己的 `.ayase` 角色包，再创建伙伴实例。角色素材独立分发，公开发布包不附带 Ema 图像或音声。已有本机实例与配置可继续使用；切换程序版本前请先从托盘退出正在运行的程序。

## 已实现

- 伙伴独立人格、音色、历史、摘要与 Hindsight 命名空间；服务凭据按类型和地址加密隔离。
- 固定、生成与明确后备回复；可选翻译朗读、文字优先或音声就绪后开始，生成正文可读化与完整任务交接。

- 有界 ZIP 导入、资源完整性校验、暂存与激活分离、实例创建/切换/改名、包更新与回退、有效配置导出。
- 整条任务非抢占，固定优先级与 FIFO，准备/开始超时、有效期、取消与迟到回执隔离。
- 透明桌宠、拖动缩放、找回位置、独立输入框、中文分页、固定音声、有限动作与循环基础姿态。
- 气泡随模型比例变化，固定下边缘向上展开并按排版分页；支持打字机/立即显示、字号与阅读时间设置，末页读完自动关闭。
- 有限动作恢复有效基础姿态；已显示 music 后停止媒体时播放完整退出片段，当前文字和音声继续完成。
- 固定音声通过 Web Audio 播放；外部媒体通过 Windows GSMTC 采集，避免桌宠音声误触发媒体姿态。
- 内置启动问候、全屏安静、媒体姿态；独立夜间提醒规则插件、表单编辑、启停、模拟时间与本机诊断。

当前同时运行一个伙伴。全屏检测、媒体会话和音频的能力边界见 [实现 ADR](docs/adr/0001-p0-p1-runtime.md)。

## 构建与开发

需要 Node.js 24、Rust MSVC stable、Visual Studio C++ Build Tools 和 WebView2。npm 与 Cargo 均提交锁文件。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build.ps1
```

产物为 `target/release/ayase-companion.exe`。脚本安装锁定依赖、执行类型/格式检查与自动测试，再构建生产版本；生产构建会映射编译路径，不启用 `smoke-test` 特性，也不自动发布。

```powershell
npm run fixtures
```

此命令生成无外部素材的静态样例和独立规则插件。`tools/create-fixtures.ps1 -EmaSource '自己的素材目录'` 可选地转换自己持有的旧 Ema 素材。`ayase-pack` 支持 `validate FILE`、`pack DIR OUT`、`fixture OUT`。

界面开发使用 `npm run tauri -- dev`。浏览器中的普通页面不能验证 Windows 原生能力。`tools/run-native-smoke.ps1` 提供隔离的原生回执检查，`-SelfMedia` 与 `-BaseRestore` 分别检查自身音声及基础姿态恢复；它们使用单独的测试构建，不能代替人工体验验收。

`python tools/create-preview-fixture.py <自己的角色包>` 可准备本机预览素材。启动 `npm run dev` 后，`/__test__/index.html` 可运行布局回归，`/__test__/control.html` 可预览控制台。这些页面不进入生产包。图标由 `python tools/build-icons.py` 从 `icon/icon.jpg` 生成，需要 Pillow。

## 文档与版本

- [系统设计](docs/01-system-design.md)、[包与协议](docs/02-protocols.md)、[阶段路线](docs/03-prototype-roadmap.md)、[验证方案](docs/04-verification.md)
- [当前实现协议](contracts/README.md)、[v0.2.0 范围](docs/releases/v0.2.0.md)、[v0.1.0 范围](docs/releases/v0.1.0.md)、[版本保留和发布规则](docs/release-policy.md)

每一阶段人工验收通过后才推送和发布。后续版本继续保留提交链，使用新的不可变标签和独立 Release 附件；全部约定设计验证完成后才发布 v1.0.0。实测批注、运行诊断、截图、个人路径与凭据仅保存在本机，不进入公开源码或发布包。
