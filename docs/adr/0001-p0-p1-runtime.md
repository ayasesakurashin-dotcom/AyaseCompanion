# ADR 0001：P0/P1 技术基线与实现边界

日期：2026-10-26。状态：实现采用，P0/P1 已在 v0.1.0 完成人工实机验收。

## 选择

Windows + Tauri 2 + Rust 内核 + 原生 TypeScript/Canvas 界面 + SQLite。新应用标识为 `com.ayase.companion`，不读取旧桌宠数据库。前端暂不引入额外界面框架。

调度、插件规则、配置校验、包校验和业务计时在 Rust 后台执行。文字、Web Audio 与 Canvas 分别报告实际通道完成；三个通道共享任务 ID 和 generation。前端没有规则包脚本执行接口。固定音声使用 Web Audio，避免自身语音注册系统媒体会话后误触发 music。

三个窗口分别为透明且不主动获得焦点的桌宠窗口、可激活的输入窗口和控制台。气泡是桌宠窗口内的独立文本组件。托盘提供控制台、输入、找回桌宠和退出入口。

使用 npm + package-lock.json；Rust 使用 Cargo.lock。已验证基础版本：Tauri 2.9.5、tauri-runtime 2.9.2、tauri-runtime-wry 2.9.3、tauri-utils 2.8.1、tauri-codegen/macros 2.5.2、tauri-build 2.5.3；TypeScript 5.6.3、Vite 6.4.1、Node 24.16.0。部分 Tauri 子依赖的宽范围会解析到不兼容的新版本，所以正常构建必须使用 `--locked`，不能只固定顶层版本。

## 调度与配置

- 业务循环使用单调时间推进，日程按配置的 IANA 时区或 Windows 当前时区计算。不依赖 requestAnimationFrame 推算提醒时间。
- 同级按宿主序号 FIFO。未开始时允许高优先级调整顺序；已开始后所有有限通道收尾才交接。开始授权超时为 5 秒，准备超时为 30 秒，候选容量 64。
- 基础模式和有限任务分开。姿态转换也参与优先级排序；尚未开始的过时目标合并，开始后的 enter/exit 完整结束。
- 基础姿态恢复区分“动作仍覆盖 music”和“已回到 music”：前者直接更新恢复目标，后者在空闲动画通道播放完整退出片段。退出使用独立 token，不复用或重开父任务的动画通道；父回复先结束也保留正在播放的退出，下一条互动继续等待。动画结束与独立过渡分别记录实际回执。
- 暂存导入与激活、安装与角色启用分别操作。首次渲染器就绪产生一次 app.ready，普通角色切换不再次视为应用启动。
- SQLite 保存实例、覆盖、日程去重记录与包引用；任务队列不持久化，重启不会重播旧音声。角色包按压缩包散列存放，不原地改写；更新保留先前包引用。
- 导出的已知有效配置使用 `configMode: complete`，避免重新导入时补回已经删除的默认列表项。普通推荐配置默认 `overlay`；本地列表删除使用稳定 ID 与 `_delete` 墓碑。

## Windows 能力边界

全屏采集限定为桌宠所在显示器的**前台无标题栏窗口**，比较实际窗口边界与完整显示器边界，并支持按进程文件名排除。带标题栏的普通最大化窗口不算全屏。P1 不宣称检测所有后台全屏窗口。

媒体采集使用 Windows GSMTC，汇总所有已公开媒体会话是否 Playing。没有接入该系统接口的播放器不会被识别；P1 不提供 Core Audio 音量猜测后备。失败、未初始化和超时显示 unknown/unavailable，不能以 false 冒充有效事实。当前采集间隔为 1 秒，超过 5 秒未更新显示未知。

自身短音声使用 AudioContext 解码到 AudioBufferSourceNode，结束依据真实 ended 事件；每次完成、静音、停止或取消都释放节点和上下文，加载/解码过程检查任务身份，超时有界失败。该方式避免创建系统媒体会话，仍经过 Windows 正常的音量与输出设备。不能笼统排除 `msedgewebview2.exe` / `MSEdge`：自身 HTMLAudioElement 与其他 WebView2 / Edge 播放器可能使用共享身份。也不能因桌宠正在发声就暂停所有外部媒体采集。

当前仅为固定音声使用有界解码播放；AI/TTS 流与长音频缓冲策略属于后续阶段。素材读取经受限 asset 协议；CSP 的 connect-src 允许该本机协议，不增加网络服务或文件授权范围。

锁屏、系统休眠、音频设备切换的连续性和恢复表现仍需人工记录。P1 的系统时间间隔诊断和有界通道失败不是 P5 可靠性验收的替代。

## 素材与分发

Ema 转换器是独立工具，只读取原资源文件、帧描述、文字和音声清单，不继承旧业务状态机。当前素材来自用户本机，生成包和包含本机路径的审计清单放在被 Git 忽略的 local-assets/，不推送到 public 仓库。

本轮交付便携 EXE。安装程序、自动启动、文件关联和应用升级属于 P5。运行需要 Windows 已有 Microsoft Edge WebView2 Runtime。

## 参考

- [Tauri 窗口配置](https://v2.tauri.app/reference/config/)
- [Microsoft GSMTC SessionManager](https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssessionmanager)
- [Windows Rust 媒体接口](https://microsoft.github.io/windows-docs-rs/doc/windows/Media/Control/struct.GlobalSystemMediaTransportControlsSessionManager.html)

上述文档支持技术选择；原始验收与运行记录仅保存在本机。
