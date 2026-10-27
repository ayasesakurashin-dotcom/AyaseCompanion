# 1.0 协议的 P0/P1 实现补充

设计草案保存在 docs/02-protocols.md。本目录与 Rust 校验器共同定义当前可执行范围。

- character：ZIP 根 manifest.json，保留未知字段、未知插件配置及文件字节。
- sprite：单图、网格、显式矩形；正放、倒放、往返；可选逐帧时长；统一逻辑画布和锚点。
- plugin / rules：声明式订阅与动作，能力声明和配置引用均校验，不支持任意脚本。
- request：P1 仅支持 fixed 配对回复；generated 等模式在 P2 接入。
- theme：受限数据格式预留；主题包编辑和导入界面在 P4，不宣称本轮支持主题制作。

## JSON Schema 共同子集

采用 2020-12 文档形式，但**不是完整 JSON Schema 2020-12 实现**。当前支持 type（单类型）、const、enum、properties、required、additionalProperties、items、min/maxItems、minimum/maximum、min/maxLength、pattern、anyOf、oneOf、同文件 `$defs`/`$ref`，以及 title/description/default/examples 元数据。无法理解的校验关键字在安装时拒绝，不静默忽略；不访问远端 `$ref`。规则、类型和公共参数均可在控制台编辑；嵌套列表有增删入口，完整 JSON 编辑器保留未知字段。

## 配置完整性与删除

`pluginPresets[].configMode` 缺省为 `overlay`：插件默认值 → 包推荐值 → 本地覆盖。

`complete` 表示已物化的完整推荐配置：完整包配置 → 本地覆盖，不再补入插件默认列表。这是导出器防止删除项目在往返中复活的标记。已知配置导出为 complete；未知配置不重新解释。配置中可选的文字、文字资产、声音和动作允许 null，表示明确清空。非空 text 和非空 textAsset 二选一。

列表按 id / presetId 合并，本地删除保存 `{ "id": "原条目", "_delete": true }`。运行时物化去掉墓碑；保存非法值不会替换上一份有效配置。

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
