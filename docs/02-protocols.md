# 角色包、插件与运行接口

版本：0.1；日期：2026-10-26。本文是编码协议草案，与[系统设计](01-system-design.md)配套使用。示例展示字段关系，尚不是已发布格式，也不代表示例中的素材已存在。P0 应将这些约束落成 JSON Schema、接口类型和测试夹具。

## 1. 协议原则与版本

角色包提供数据，插件提供行为，宿主提供执行能力。导入角色包不能执行任意代码。安装插件、给角色启用插件和授予能力是不同操作。

建议暂用以下扩展名，正式发布前统一确认并注册文件关联：

| 文件 | 暂定扩展名 | ZIP 根目录清单 | 用途 |
| --- | --- | --- | --- |
| 角色包 | .ayase | manifest.json | 外观、人格、推荐行为配置和素材 |
| 插件包 | .ayplug | plugin.json | 插件说明、规则或隔离代码、配置表单 |
| 气泡主题包 | .aytheme | theme.json | 可选主题配置、PNG 等图形素材 |

这里的“文件头”指 ZIP 内固定位置的清单，不在 ZIP 前面拼接自定义字节。不要通过扩展名推断安全性或格式版本。

版本分为：包格式 formatVersion、插件执行 apiVersion、插件配置 configVersion、资源描述 schemaVersion、包自身 version。包更新不自动代表协议更新。首个可编码基线建议为 1.0，P0 尚未冻结前可修改；发布之后必须支持明确迁移或给出兼容结果。

- 同一主版本中新增的可选字段按声明兼容；未知扩展字段保留。
- 不能理解的角色包主版本：拒绝激活，当前角色继续运行。
- 可理解角色包中的未知插件或过新插件配置：保留原记录，暂停该配置的执行；不阻止模型运行。
- 未知渲染类型：有声明的有效静态后备模型才可降级，否则无法满足常规形态要求，应拒绝激活。
- 迁移失败不能覆盖原包或本地配置。

## 2. 角色包布局

建议布局：

```text
manifest.json
avatar/sprite.json
assets/visual/idle.webp
assets/visual/greeting.webp
assets/voice/hello.wav
assets/text/hello.txt
identity/persona.txt
licenses/asset-sources.json
```

允许子目录变化，宿主通过清单解析资源，不依赖固定角色名称。清单及描述文件采用 UTF-8。文件引用必须是包内相对路径；运行时使用资产 ID 和宿主资源 URL，不能暴露任意本机文件读取能力。

### 2.1 角色清单示例

```json
{
  "format": "ayase.character",
  "formatVersion": "1.0",
  "packageId": "local.ema.prototype",
  "version": "0.1.0",
  "displayName": "Ema 原型测试包",
  "identity": {
    "characterName": "艾玛",
    "userAddress": "你",
    "personaAsset": "persona.main"
  },
  "avatar": {
    "renderer": "sprite.v1",
    "descriptor": "avatar/sprite.json"
  },
  "assets": {
    "persona.main": { "kind": "text", "path": "identity/persona.txt" },
    "visual.idle": { "kind": "image", "path": "assets/visual/idle.webp" },
    "visual.greeting": { "kind": "image", "path": "assets/visual/greeting.webp" },
    "voice.hello": { "kind": "audio", "path": "assets/voice/hello.wav" },
    "text.hello": { "kind": "text", "path": "assets/text/hello.txt" }
  },
  "pluginPresets": [
    {
      "presetId": "startup-default",
      "pluginId": "ayase.startup-greeting",
      "configVersion": "1.0",
      "enabled": true,
      "priority": "normal",
      "config": {
        "source": "app.ready",
        "timeZone": "system",
        "windows": [
          {
            "id": "all-day",
            "start": "00:00",
            "end": "00:00",
            "allDay": true,
            "response": {
              "mode": "fixed",
              "variants": [
                {
                  "id": "hello-1",
                  "weight": 1,
                  "textAsset": "text.hello",
                  "audioAsset": "voice.hello",
                  "cue": "greeting"
                }
              ]
            }
          }
        ]
      }
    }
  ],
  "extensions": {}
}
```

该例省略非关键作者信息和完整性散列。P0 规定可选 sha256 字段，正式导出器应为所有打包文件生成完整性清单，导入时校验。文件被列入完整性清单却散列不符属于包完整性错误，不能当作正常的“可选素材缺失”略过。

用户称呼是角色建议值，可由本地覆盖；人格不因多个插件各写一份而分裂。插件配置不能指定 instanceId、记忆 bank、API 密钥或本机数据目录。

### 2.2 资源登记与引用

每个资产记录包含 kind 与 path，按类型可增加 MIME、语言、时长、尺寸、来源和散列。资产 ID 在包内唯一。JSON 中引用资产用 ID；需要原始文件路径的渲染描述必须在其类型的受控解析器中校验。

默认只允许本地包内资产。远程图片、语音或模型不在第一版导入阶段自动下载；未来支持时必须声明获取方式、缓存和失败后备。身份文本和固定台词可直接内联或引用文本资产，协议必须规定二选一，不能产生模糊优先顺序。

- 必需 idle 不可解码：模型无法激活。
- 可选 greeting 不存在：保留基础姿态。
- 可选音声不存在：不播放该音声；已有文字仍可显示。
- 可选文字不存在且未要求生成：允许只播动作或音声。
- 三个通道均为空：记录 skipped 结果，不创建空任务占用播放锁。
- 缺少可选内容在编辑器可见，但正常播放不连续弹错。

### 2.3 序列帧描述

以下是单张 idle 与横向 8 帧招手图集的示意，实际 Ema 转换器应从原素材读取尺寸，不能使用这些示例数字代替测量。

```json
{
  "schemaVersion": "1.0",
  "logicalSize": { "width": 512, "height": 512 },
  "anchors": {
    "feet": { "x": 0.5, "y": 0.95 },
    "bubble": { "x": 0.75, "y": 0.2 }
  },
  "clips": {
    "idle": {
      "asset": "visual.idle",
      "layout": { "type": "single" },
      "frameDurationMs": 1000,
      "direction": "forward"
    },
    "wave": {
      "asset": "visual.greeting",
      "layout": {
        "type": "grid",
        "columns": 8,
        "rows": 1,
        "frameCount": 8,
        "cellWidth": 512,
        "cellHeight": 512
      },
      "frameDurationMs": 100,
      "direction": "forward"
    }
  },
  "baseModes": { "idle": { "loop": "idle" } },
  "semanticCues": { "greeting": { "clip": "wave", "repeat": 1 } }
}
```

扩展布局应支持显式帧矩形；方向支持 forward、reverse、ping-pong，往返端点是否重复必须明确为字段或固定规则。支持逐帧时长时，数组长度必须与帧数一致。逻辑尺寸、像素尺寸与桌面 DPI 分开。

baseModes 可声明 enter、loop、exit。idle.loop 为必需；music、quiet、sleep 等可选。缺少请求的基础模式时使用 idle，而不是伪造该角色具有睡眠动作。退出某基础模式时是否播放 exit，由最新目标、转换规则和调度边界共同决定。

临时动作 repeat 必须有限。讲话期间的动作循环需显式绑定任务生命周期，由呈现协调器在文本和声音收尾后停止；不能等待无限循环自然完成。可选命中区域与拖动区域只影响交互位置，不提供业务触发判断。

## 3. 插件包与参数

### 3.1 插件标识和结构

pluginId 是带命名空间的稳定类型 ID，例如 ayase.scheduled-reminder。presetId 标识角色包中的一组配置；本地 pluginInstanceId 标识该角色下实际启用的一份配置。因此同一提醒插件可以配置多个时间点，不需要复制插件代码。

```text
plugin.json
schemas/config.schema.json
ui/config.ui.json
rules/main.json
README.md
assets/                 可选插件自带的通用素材
```

插件类型与公共能力分开。插件可以依赖能力版本和其他明确公开的服务，第一版不允许任意循环插件依赖。

### 3.2 23 点提醒插件清单示例

```json
{
  "format": "ayase.plugin",
  "formatVersion": "1.0",
  "pluginId": "example.nightly-rest",
  "version": "0.1.0",
  "displayName": "夜间休息提醒",
  "apiVersion": "1.0",
  "runtime": { "type": "declarative.v1", "entry": "rules/main.json" },
  "configVersion": "1.0",
  "configSchema": "schemas/config.schema.json",
  "uiSchema": "ui/config.ui.json",
  "defaultPriority": "normal",
  "capabilities": ["clock.schedule", "presentation.request"],
  "optionalCapabilities": ["role.reply", "voice.synthesize"],
  "defaults": {
    "time": "23:00",
    "timeZone": "system",
    "missedPolicy": "skip",
    "ttlSeconds": 300,
    "response": {
      "mode": "fixed",
      "variants": [
        {
          "id": "rest-1",
          "text": "已经很晚了，记得休息。",
          "cue": "reminder"
        }
      ]
    }
  }
}
```

defaults 提供“新增配置时”的初始值，不等于所有角色安装后自动启用。包内没有该插件配置的角色始终默认关闭；用户在设置中点击新增配置后才产生本地配置实例。

23:00、300 秒等是测试样例，不是宿主不可更改的常量。提醒时间、时区、错过策略、有效期、文字、声音、语义动作、AI 模式、优先级等均可调整。

### 3.3 参数 Schema 与表单

configSchema 使用固定版本的 JSON Schema；首版建议 2020-12，P0 根据前后端验证器兼容情况锁定共同子集。表单可用能力必须覆盖实际协议，不允许后端支持而用户无处修改。

公共参数支持布尔、枚举、数值范围、时间与时区、时段列表、资源选择、文本、多条配对回复、条件组合和语义动作。uiSchema 提供分组、标签、帮助、显示条件和资源选择控件；不能执行任意 JavaScript。

- 所有对外配置参数必须可编辑；内部实现状态不作为用户参数。
- 列表条目有稳定 ID，支持新增、删除、排序、禁用。
- 默认值、空值、删除和“恢复推荐值”具有不同语义。
- 非法配置不覆盖上一份有效配置，错误定位到具体字段。
- 前端实时校验、宿主保存校验和导入校验使用同一规范。
- 插件升级提供可测试的配置迁移；没有迁移时保留旧值并暂停受影响配置。
- 未知配置字段不得因表单重存被悄悄删除。

### 3.4 规则运行时

第一阶段提供有限、可校验的声明式规则，不提供 eval、任意文件访问或任意系统命令。规则只使用宿主暴露的事件、事实、配置、比较运算和动作。

建议规则文件示例：

```json
{
  "schemaVersion": "1.0",
  "subscriptions": [
    {
      "id": "daily-rest",
      "type": "schedule.daily",
      "timeFromConfig": "/time",
      "timeZoneFromConfig": "/timeZone",
      "missedPolicyFromConfig": "/missedPolicy"
    }
  ],
  "rules": [
    {
      "id": "rest",
      "on": "daily-rest",
      "dedupe": { "scope": "plugin-instance", "key": "schedule-occurrence" },
      "actions": [
        {
          "type": "presentation.request",
          "responseFromConfig": "/response",
          "ttlSecondsFromConfig": "/ttlSeconds"
        }
      ]
    }
  ]
}
```

schemaVersion、JSON Pointer 的求值类型和允许引用的根对象必须确定。不存在的配置引用应在安装或保存时发现；事件运行时不会把任意字符串当代码执行。

需要的首版动作包括请求互动、申请/释放基础姿态、更新插件自身的有限状态。更复杂的媒体和工作统计依赖宿主事实与专用服务，不把全部底层采集逻辑塞入 JSON。

无需修改宿主的承诺适用于已公开能力的组合。例如新装 23 点提醒插件可以复用 clock.schedule；未来插件需要全新硬件或系统接口时，仍需要新增能力适配器。

### 3.5 生命周期和能力

生命周期至少包含 installed、configured、enabled、disabled、uninstalled 以及 faulted 诊断状态。安装不自动启用所有角色；启用必须有有效配置和必要能力。

关闭或卸载属于明确用户操作：撤销订阅、计时器与基础姿态请求，取消该插件实例的待执行及正在执行任务，并清理其晚到回调。是否保留配置应在卸载流程中明确，默认建议保留以便重装。

后台插件错误停止产生新请求；已交给宿主且仍能完成的呈现任务继续完成。只有任务自身不可继续的故障才进入失败收尾，不能借 faulted 状态自动抢占。

后续代码插件运行在隔离 worker、进程或其他受限运行时中，具体方式在 P6 验证。它们只能通过版本化消息调用能力；不能持有音频设备、渲染窗口、全局数据库或 API 密钥。需具备资源配额、超时、崩溃隔离和安装来源展示。

## 4. 事件、任务和执行回执

### 4.1 事件信封

建议 EventEnvelope 包含 eventId、eventType、schemaVersion、source、occurredAt、receivedAt、instanceScope、payload。receivedAt 由宿主记录；source 由事件入口认证。插件不能自称 user 来源。

时间事件附带 scheduleOccurrenceId，供同一计划在恢复运行或重连时去重。首次启动、每日登录、睡眠恢复等不能共用一个不明确的 startup 字符串。

### 4.2 插件提交的请求

建议插件使用如下逻辑接口，字段类型需在 P0 固化：

```ts
type ResponsePlan =
  | { mode: 'fixed'; variants: ReplyVariant[] }
  | { mode: 'generated'; intent: string; voice?: VoicePlan; cue?: string }
  | { mode: 'generated-with-fallback'; intent: string;
      fallback: ReplyVariant[]; voice?: VoicePlan; cue?: string };

interface InteractionRequest {
  localRequestId: string;
  response: ResponsePlan;
  expiresAfterMs?: number;
  dedupeKey?: string;
  conditionRef?: string;
}
```

ReplyVariant 将文字、音声和动作放在一个条目中抽选，避免随机选中文字与另一句固定音声错配。空通道合法；无条件默认为 AI 生成不合法。generated-with-fallback 的固定后备必须由配置明确指定。

VoicePlan 可引用本地角色音色绑定及朗读策略，不传送密钥。固定录音与动态合成的选择是不同模式；未经配置不能同时播放两份音声。

### 4.3 宿主补全字段

宿主依据调用上下文补全 taskId、instanceId、pluginInstanceId、sourceEventId、enqueueSequence、effectivePriority、profileRevision、historyEpoch、acceptedAt、deadline、cancellationToken、presentationPolicy。

上述身份和排序字段不信任插件自行传入。能力检查、冷却和去重在宿主执行；插件自报“用户消息”不能获得 user 优先级。

- profileRevision 用于开始前检查配置是否仍可用；不因无关设置改变就丢弃全部任务。P0 定义依赖字段或分区版本。
- historyEpoch 用于遗忘、重置和角色隔离，防止旧摘要或旧请求写回已经清除的上下文。
- 超时、准备和等待使用单调时钟。展示时间戳使用墙上时间。
- 排队有效期在开始后不再取消任务。

### 4.4 任务状态与完成

核心转移为 queued → preparing → ready → starting → presenting → completed。固定素材已就绪可跳过 preparing；未开始任务可进入 expired/cancelled/failed；已开始任务只按完成、明确用户取消或不可继续的故障结束。

starting 发放短期且唯一的呈现授权。实际输出必须持有授权；首个可见字、可闻音声或有限动画开始时报告 started。授权超时先撤销再选下一任务，迟到的 started/输出必须被拒绝，避免两个任务同时认为自己有前台所有权。

呈现通道通过 taskId、channel、segmentId、generation 标识数据和回执。回执至少包含 started、segmentCompleted、channelCompleted、channelFailed、cancelled。完成聚合器对每个 taskId 只产生一次最终状态。

一个任务可以有多段文字和音声。分页或句子结束不能让其他插件插入。流式回复尚未结束时不能预先宣布整个文字通道完成；所有页面的阅读停留也算有限任务的一部分。

所有组件必须忽略旧 generation 的追加文本、TTS 数据、解码结果和完成通知。取消后清空该任务的未播缓冲，不清除其他任务的资源。

### 4.5 调度顺序

候选先通过功能、角色、环境、有效期和能力检查，再按 effectivePriority 降序与 enqueueSequence 升序排列。进行中任务不参与竞争。

同级队首尚在准备，后面的同级即使已准备好也不超车；队首的准备需要有限超时。高一级任务可成为新队首。准备并发度与缓存容量有上限，避免排队期间无限生成付费回复。

有持续流但始终不结束的生成需设置可配置的流停滞与最大输出限制。停滞视为生成故障并收尾，不伪装成高优先级中断；最大输出限制应在服务生成前告知，保证正常回复能够自然结束。

## 5. 基础姿态与渲染接口

### 5.1 基础姿态请求

BaseClaim 包含 claimId、owner、mode、有效条件和宿主记录的优先级、接受序号。其存在表示“在这个条件下希望使用某基础模式”，不是立即播放命令。

- 全屏静默策略即时影响新主动任务的准入。
- 当前任务播放完成之前不改变其模型表现和音声。
- 边界重新选择有效模式；尚未开始的旧转换被合并或撤销。
- 需要 enter/exit 动画时创建有限转换任务，按优先级和顺序排队。
- 没有过渡片段的模式变化也在相同边界提交，不能绕过高优先级 user 任务。
- 应用基础循环不持有有限呈现锁。
- 全屏解除后根据仍有效的媒体请求决定回 music 还是 idle。

用户直接静音是音频控制；插件的全屏安静状态不得伪装成用户静音，以此截断已开始的音频。

### 5.2 渲染适配器的最小契约

```ts
interface AvatarRenderer {
  load(model: ResolvedModel, context: RendererContext): Promise<Capabilities>;
  listClips(): ClipInfo[];
  resolveCue(cue: string): ResolvedCue | null;
  getAnchors(): Anchors;
  getHitAreas(): HitArea[];
  applyBase(mode: string, lease: BoundaryLease): Promise<void>;
  playFinite(command: FinitePlayCommand): Promise<PlaybackHandle>;
  cancel(playbackId: string, reason: UserOrFailureReason): Promise<void>;
  dispose(): Promise<void>;
}
```

这是逻辑契约，不强制跨 Rust/TypeScript 直接共享对象。实际跨进程传输采用版本化消息。播放句柄提供实际开始、完成和失败事件；有限动画播放完成不等于整个呈现任务完成。

所有命令带角色实例和播放代次，切换角色后旧句柄无权影响新模型。模型坐标使用逻辑坐标，窗口转换承担 DPI。加载与释放必须可重复，不能遗留纹理、定时器或音声。

## 6. 统一服务接口

行为插件调用能力时，宿主隐式注入角色作用域。可公开的逻辑 API 包括：

| 能力 | 调用目标 | 约束 |
| --- | --- | --- |
| role.reply | 使用统一人格生成对用户的回复 | 人格来自实例；插件传意图与有来源的事实 |
| voice.synthesize | 翻译后的朗读或原文合成 | 音色来自实例绑定，密钥来自服务配置 |
| memory.recall / memory.record | 检索和记录角色记忆 | bank 由宿主绑定；不得传任意其他实例 ID |
| screen.observe | 请求一次屏幕观察 | 区分手动、定时；遵守采集开关与保留策略 |
| search.query | 查询外部信息 | 结果有来源和时间，失败不能生成虚假事实 |
| weather.read / calendar.read | 获取天气和日期事实 | 明确地点、时区、来源与更新时间 |
| environment.read / subscribe | 读取全屏、媒体等状态 | 只读事实，不让插件直接操作其他应用 |
| presentation.request | 请求呈现 | 返回 taskId 或明确拒绝/跳过原因 |
| base.claim / base.release | 申请或撤销基础姿态 | 由宿主在边界协调 |

内部观察提取、翻译、摘要和记忆整理可使用专用任务提示词；凡是作为角色对用户说出的 AI 内容，统一走 role.reply 或其同一人格上下文，不能在某提醒插件中绕过人格。

截图、网页和远端记忆是外部数据，不能被当成插件指令执行。模型生成的结构化动作需经 schema 与能力白名单校验，不能让自然语言回复直接获得窗口、文件或插件管理权。

用户密钥保存在独立服务设置中。角色配置可引用逻辑 providerSlot 和音色建议，但导出时不附带本机服务账户标识。导入者自己为逻辑槽绑定服务；未绑定时显示待配置或使用明确后备。

## 7. 本地配置、实例与导出

### 7.1 配置解析

合并顺序为插件默认值 → 角色包推荐值 → 本地角色覆盖。全局环境策略和用户总开关在执行时进一步约束，不能反向写进角色模板。

有效启用需要同时满足：插件已安装、角色存在该配置、用户允许启用、配置有效、必需能力可用。运行时全屏策略可能暂时阻止主动输出，但不把 enabled 永久改成 false。

建议本地档案包含 instanceId、sourcePackageId、sourceVersion、displayName、identityOverrides、pluginOverrides、assetOverrides、tombstones、memoryBinding。凭据另存，运行统计与聊天记录另表。

插件覆盖按 presetId 或本地 pluginInstanceId 管理；列表按稳定条目 ID 管理。删除记录写入 tombstones，不能因为下次合并默认值又自动恢复。重置推荐值才清除对应覆盖与删除标记。

### 7.2 实例与记忆

memoryBinding 仅由宿主管理，包含本地命名空间和可选远端服务绑定。新建实例生成新 bank；改名、换皮和更新包保留 bank。删除实例、仅清空历史、遗忘记忆是不同操作，不能混成一个按钮。

P2 可以使用每实例独立 SQLite 文件减少串读风险；若使用共享数据库，则所有相关表、查询、外键、缓存、摘要和 outbox 都必须携带实例作用域。具体存储方式通过 ADR 确认，隔离语义不变。

统一回复生成使用实例、人格和上下文快照。只对实际呈现的回复提交正常对话记录；中断内容可标记 partial，不把未展示的剩余文本当成已交流事实。服务执行日志与角色记忆分开。

### 7.3 导出规则

导出将当前有效人格、外观、插件配置和被引用素材生成新角色包，可选择新 packageId 或创建新版本；不得改变当前实例 ID 或记忆绑定。

导出不包含：密钥、用户历史、远端 bank、机器绝对路径、正在运行的任务、订阅状态、操作统计及尚未完成的网络请求。作者明确编写的人格与称呼属于角色内容；导出界面展示内容清单，方便用户检查个性化文本。

未知插件记录按结构保留，不经未知 schema 重新解释。其关联文件应字节保留；无法判明引用关系时保守保留原资源集合，不进行激进的“未使用资产清理”。JSON 重排键不影响结构等价，但不得丢弃未知值。

用户删除仍可能被未知插件引用的素材时，先标记本地不使用。导出器需要保留原文件或让用户明确处理引用冲突；不能悄悄生成悬空引用。已知配置中的可选空引用可归一为未配置，必需 idle 删除则阻止保存为可激活角色。

### 7.4 包更新

P1/P2 至少区分新建实例与更新已有实例，并保留原版本以便回退。P4 完成基于旧推荐值、新推荐值和用户覆盖的合并预览，展示冲突和删除条目。

升级失败维持旧配置可用，不能先删旧包再验证新包。并存使用的资产按实例和版本引用管理，删除一个实例不影响其他实例。

## 8. 导入事务与兼容结果

导入流程：识别包 → 临时区读取清单 → 路径与大小检查 → 协议和资源校验 → 兼容分析 → 形成导入报告 → 原子提交 → 创建或更新实例 → 用户激活。

必要防护应实现在通用解包器：拒绝绝对路径、目录穿越、符号链接逃逸、重复规范化路径；限制条目数量、单文件/总展开大小、图像尺寸和解压比。限值在 P0/P5 根据实际 Ema 素材测量，不以任意小常量破坏正常图集。

规范化时考虑 Windows 大小写、保留设备名和尾部点/空格。资源解码也要有内存上限，不能只检查压缩文件大小。

导入报告区分：成功、成功但部分能力未启用、不可激活。未知插件、可选素材缺失和服务未配置不是整个导入失败；损坏 ZIP、必要 idle 失败、未知主版本等属于不可激活。

导入操作本身不触发问候、不发送网络请求、不写入角色记忆、不自动安装包中提到的外部插件。激活及用户已启用的插件遵循正常生命周期。导入失败时当前运行角色及本地修改完整保留。

## 9. 气泡主题与制作工作区

主题包可包含背景 PNG、九宫格边距、内容内边距、尾巴锚点、字号、前景色、背景色和最大宽度。图片提供外观，文字仍由独立文本组件排版，不能把流式文字画死在 PNG 中。

文字显现方式、速度与阅读停留也归入气泡主题设置范围。P0/P1 提供本机全局偏好：打字机/立即显示、10—80 字/秒、12—24 像素字号、1000—10000 毫秒最少阅读时间；默认分别为打字机、30、15、2800。完整显示当页后开始阅读计时，长页会自动延长，任务显式指定的阅读时间优先。它们暂不进入角色包或主题包导出；后续主题实现需复用这些行为并扩展主题协议。气泡大小跟随模型比例，排版容量由实际测量决定，最终页读完自动关闭。

第一版使用受限声明式主题，不加载任意 HTML/JavaScript。主题样式不得污染输入框、宿主设置页或模型动画。角色包可推荐主题，不能未经用户选择覆盖其全局主题偏好。

制作工作台保存的项目包含源资产、配置、编辑元数据和测试场景；发布角色包只包含运行所需内容。初期工作区可以是普通目录，后续是否单独设置工程扩展名再决定。

宿主简易编辑和工作台共享：资源解析、schema、配置合并、包校验、导入导出、语义动作解析和预览运行时。工作台预览默认采用虚拟事件和模拟服务；用户启用实服务测试后才调用实际 AI/TTS。

## 10. P0 必须落实的协议工作

1. 将角色、序列帧、插件、规则和主题的 schema 纳入 contracts，锁定共同验证器子集。
2. 建立静态 idle、Ema 多动作、无音声、未知插件、非法 idle 等正反样例。
3. 定义全部事件、任务状态、开始授权、取消和最终回执类型。
4. 固化配置删除、未知记录保留、包更新和导出排除字段语义。
5. 为例子中的全部公共参数生成可编辑表单，识别 schema 无法表达的跨字段校验。
6. 提供内存渲染器、假时钟和假服务，先验证调度，再接真实窗口。

协议冻结以测试样例和 ADR 为依据。未完成上述工作前，不能仅因本文包含 JSON 示例就声称格式已经实现或兼容。

