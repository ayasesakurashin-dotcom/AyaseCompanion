# AyaseCompanion 开发约定

- 阅读 docs/01-system-design.md 至 docs/04-verification.md；实现决策与协议补充见 docs/adr 和 contracts/README.md。
- 这是独立工程。旧桌宠工程只可读作参考或提供本机测试素材，不在旧工程叠加开发，不共享应用数据。
- P0/P1、P2 与 P3 已由用户明确确认实机验收通过，当前开发基线固定为 v0.3.0；v0.1.0、v0.2.0 的标签与附件保留。后续开发从 v0.3.0 继续，仍逐阶段交付候选，用户确认通过后再发布；P4 尚未开始。
- GitHub 远端是 https://github.com/ayasesakurashin-dotcom/AyaseCompanion，public。每一阶段人工验证通过后才推送对应源码与发布版本；验收前可本地提交，不推送、不发布、不创建正式版本标签。
- 每次后续版本保留公开 Git 提交链，使用新的不可变标签和发布包。禁止 force push、移动既有版本标签、覆盖旧发布附件或改写已发布历史。全部约定设计内容验证后才发布 v1.0.0。
- 首次公开版本以前的开发历史含私人验收信息，仅保存在本机归档与 refs/private；不得推送这些引用、使用 --all 或 --mirror。
- Ema 图像、音声、个人数据、凭据、真实本机路径、具体测试记录、诊断、截图、原应用数据不提交公共仓库或 Release。仅提交源码、通用设计文档、测试实现、转换器、生成通用夹具的工具和独立规则插件源码。
- 发布前审计全部待推送历史与二进制；仅删除当前文件不能清除历史中的私人内容。生产构建必须映射本机编译路径。验收原件和审计输出留在被忽略的 artifacts/ 中。
- 三个呈现通道归同一任务。已有整条任务不会被后台任务、全屏或媒体变化打断；明确用户停止、切换、关闭对应插件可以取消。
- Rust 内核是调度、插件与配置校验的权威实现。前端只持有当前呈现授权；所有演示按钮经过宿主，不直接调用播放器。
- 不把自动测试、模拟环境或实际播放器回执当作用户体验验收。真实全屏/媒体切换、DPI、多屏、IME、锁屏和音频设备行为分别记录，记录只留本机。
- 必要检查：npm ci、npm run build、npm test、cargo test -p ayase-core --locked、cargo test -p ayase-companion --locked、cargo check -p ayase-companion --locked、cargo fmt --all -- --check。Windows 便捷入口为 tools/build.ps1。
- smoke-test 是单独编译特性，只用于本地原生回执测试；交付构建不得启用此特性。
