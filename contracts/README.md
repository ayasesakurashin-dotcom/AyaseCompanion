# 1.0 协议的 P0—P2 实现补充

设计草案保存在 docs/02-protocols.md。本目录与 Rust 校验器共同定义当前可执行范围。

- character：ZIP 根 manifest.json，保留未知字段、未知插件配置及文件字节。
- sprite：单图、网格、显式矩形；正放、倒放、往返；可选逐帧时长；统一逻辑画布和锚点。
- plugin / rules：声明式订阅与动作，能力声明和配置引用均校验，不支持任意脚本。
- request：P1 的 fixed 配对回复继续兼容；P2 支持 generated 和 generated-with-fallback，统一使用实例人格与宿主服务设置。
- theme：受限数据格式预留；主题包编辑和导入界面在 P4，不宣称本轮支持主题制作。

## JSON Schema 共同子集

采用 2020-12 文档形式，但**不是完整 JSON Schema 2020-12 实现**。当前支持 type（单类型）、const、enum、properties、required、additionalProperties、items、min/maxItems、minimum/maximum、min/maxLength、pattern、anyOf、oneOf、同文件 `$defs`/`$ref`，以及 title/description/default/examples 元数据。无法理解的校验关键字在安装时拒绝，不静默忽略；不访问远端 `$ref`。规则、类型和公共参数均可在控制台编辑；嵌套列表有增删入口，完整 JSON 编辑器保留未知字段。

## 配置完整性与删除

`pluginPresets[].configMode` 缺省为 `overlay`：插件默认值 → 包推荐值 → 本地覆盖。

`complete` 表示已物化的完整推荐配置：完整包配置 → 本地覆盖，不再补入插件默认列表。这是导出器防止删除项目在往返中复活的标记。已知配置导出为 complete；未知配置不重新解释。配置中可选的文字、文字资产、声音和动作允许 null，表示明确清空。非空 text 和非空 textAsset 二选一。

列表按 id / presetId 合并，本地删除保存 `{ "id": "原条目", "_delete": true }`。运行时物化去掉墓碑；保存非法值不会替换上一份有效配置。

## 固定与后备的多素材

`variants[]` 与 `fallback[]` 的 `text`、`textAsset`、`audioAsset`、`cue` 均兼容原单个字符串/`null`，新增字符串数组。素材编号另可使用逗号（含中文逗号）或换行简写；普通 `text` 字符串不按标点或换行拆成多个素材。非空 `text` 与 `textAsset` 仍互斥。每种最多32条，文字正文或解析后的文字素材合计最多12000字符。

每个文字素材独立测量分页、打字机和阅读停留，段间直接进入下一素材，仅最后一段收起并完成文字通道；实际呈现记账以换行连接各段。音声和语义动作各按自身列表顺序连续播放，语义映射的 repeat 在对应动作处展开。三种列表长度可不同，通道独立运行，不按索引强行配对；全部归属于同一任务，所有通道结束后才能交接。缺失/停用的可选素材沿用跳过降级；全部没有可呈现内容时跳过/失败，不占锁。

同一伙伴、配置实例及 fixed/fallback 列表独立防连续重复：在开始授权前按实际呈现计划排除上一条效果，再按剩余条目的权重抽取。若有两个不同效果则交替；只有一个有效效果时正常播放。相同已解析文字/音声/动作计划也视为同一效果，避免不同 id 的复制条目绕过防重复。只有实际开始才更新本机记录，排队取消/过期不计入；记录重启保留，不随角色包导出。列表内容改变时重置该列表记录。

运行计划增加 `textSegments`、复用有序 `audioSegments` 和 `clips`；不增加子任务。`variant-started` 诊断包含任务、变体 id 和 fixed/fallback 来源，`audio-segment` 继续逐段报告开始/结束。

## 往返端点

四帧示例：forward 为 0,1,2,3；reverse 为 3,2,1,0。

ping-pong 缺省为 0,1,2,3,2,1。`returnToStart:true` 为 0,1,2,3,2,1,0；`repeatEndpoints:true` 为 0,1,2,3,3,2,1,0。startReverse 反转起始方向。Ema 转换使用不重复远端帧、回到起始帧的有限序列，并在审计文件中记录。逐帧时长按源帧索引配对。

## 资源限制

ZIP 最多 2048 条目，单文件展开最多 128 MiB，总展开最多 512 MiB；大于 1 MiB 的条目压缩比不得超过 500。拒绝路径穿越、绝对路径、反斜线、Windows 设备名、尾部点/空格、大小写规范化重名、文件/目录冲突和符号链接。

PNG/WebP 单轴最多 16384 像素，单图解码预算 128 MiB，注册图像累计最多 128 M 像素。JSON 单文件最多 2 MiB。每次导出生成资源完整性散列，已列入清单却丢失或散列不符是错误；未承诺完整性的可选素材缺失可以降级。

## 运行状态与回执

Rust scheduler.rs 是权威状态机：queued / preparing / ready / starting / presenting / completed / cancelled / expired / failed。接受时分配严格递增 sequence 和 generation；消息必须匹配 taskId、generation、活动角色与通道。普通插件不能取得 user 级别。实例和配置修订在开始前复核，开始后的 TTL 不截断任务。

纯基础循环不占任务锁。文字最后一页停留并收起后完成，音声 ended 后完成，有限帧序结束后完成。静音只结束 audio；停止、角色切换、关闭/卸载对应插件撤销相应任务。事件、通道失败和最终收尾可在运行记录关联查看。

普通互动含有限动作时，媒体在动作中停止则提前把恢复目标改为 idle，不打断动作或补播尚未重新显示的 music 收尾。若动作已经完成并回到 music，则播放完整退出片段，文字/音声继续。

后一种情况使用 scheduler.baseTransition，含独立 id/generation/instanceId、parentTaskId、来源/目标模式、clips 和开始授权。呈现窗口通过 base_transition_ack 报告 started/completed/failed。它不重新打开父任务的 animation 通道；父回复提前结束时，呈现器仍保留过渡，调度器继续阻止下一条互动。失败、开始超时或明确取消回退目标循环并撤销授权，不重复排队同一退出。角色切换、停止、对应插件关闭使旧回执失效。

动画完成回执可附 `visualBase`（idle/music/quiet）与 `visualKind`（base/finite）；只有任务/代次/通道回执有效时，宿主才写入 `animation-return` 诊断。它描述渲染器实际选择的恢复状态，不授予额外呈现权。`base-fallback` 记录被撤销循环的原模式、目标和关联互动任务。

base-transition-planned/started/ended 记录完整退出的授权与收尾，base-transition-rendered 表示实际有限片段完成回执。已开始的过渡按原规则完整播放，最新基础请求在之后重新求解。

## P2 请求与私有运行身份

response.mode 支持 fixed、generated、generated-with-fallback。fixed 必须提供配对 variants；generated 必须提供 intent；generated-with-fallback 还必须有明确 fallback 变体。voice.mode 可为 off / tts；voice.presentationPolicy 可为 text-first / speech-synchronized。它们均不携带人格、实例 ID、bank 或凭据。

角色资产可用 enabled:false 表示可选固定音声停用；当前只解释 audio 类型，原始文件和未知引用仍保留。人格导出物化 identity 的已知作者字段并保留未知字段，不包含本机音色绑定。

P2 运行的文字计划使用增量累积正文和 streamDone；语音使用有序 audioSegments、audioCount、audioDone，解码和播放回执带任务 ID/代次/段序号。正常通道完成仍由原调度器聚合；断流按失败收尾并记录实际显示部分。所有异步数据均受原实例、历史 epoch 和取消令牌限制。

实现与配置见 docs/adr/0002-p2-services-memory.md、docs/p2-user-guide.md。实机记录只留本机。

## P3 内置行为与证据

新增受限 `hostBehavior` 声明仅用于同名内置插件，不接受外部包覆盖。配置由内置 schema 与 Rust 校验器共同校验。工作、随机、点击、定时/周期、观察和关怀输出继续复用现有请求及完整任务调度。

sprite 可选 `hitAreas` 为最多32个归一化矩形，字段 id/x/y/width/height；未提供或指定区域不存在时使用模型可见区域。

屏幕票据仅在内存中绑定伙伴、历史 epoch、配置和窗口身份；跨窗口消息只传不透明票据，原生句柄及图像不由前端指定。观察原文、稳定序号、摘要水位和搜索来源为私有运行数据，不进入角色包。历史来源与实际呈现正文一起记账；观察删除/遗忘使相应旧摘要及引用失效。

统一主动预算、各行为配置、服务边界和真实服务说明见 docs/adr/0003-p3-capabilities.md 与 docs/p3-user-guide.md。P3 候选不表示已完成人工验收。
