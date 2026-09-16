# ZS1-094 — src/layout.rs 单文件完整复核（3.1.21）

状态 `[_]`：worker report-only candidate，Master 独立 G-FILE / G-STAGE 接收待定。唯一 owned path 为本报告；本轮不改 BentoBox 或任何产品文件，不处理 085 F2/F3，也不将本报告当作 120/090 或122验收。主控当前50/121仅是接单时状态。

## 权威、基线与本轮重新完整阅读

2026-09-13T01:28:27.549820+00:00 从主库只读捕获 `src/layout.rs`：54632 B、1567行，SHA256 `218c1b1cb7853da8f5c03b51e09d552f0634efb82284e029cbcfaeb434dac05a`。与3.1.21蓝图第119行及 selector.baseline_files.ZS1-094 完全一致。蓝图第210行指定本报告、依赖ZS1-001、G-FILE/G-STAGE及仅撤回报告；第240行090、第434行120是上层依赖，本轮不代验。

run_id=`zenpi-stage1-20260911`，requirement digest=`3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline snapshot=`92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。捕获蓝图153896 B，snapshot=`aa9412fe226f42c2dc6898b09cf94ba34e76ee26f268abd4d43d13fad57e7dc1`；selector active=true、3.1.21及 snapshot 匹配。状态后续改变不自动使本输入失效；不拿全局状态hash作无关任务阻塞条件。主库、blueprint、claims、master receipts和旧ready只读。

虽然源文件相对冻结基线没有变化，本轮重新按顺序读完：1–320（工具a9da2c）、321–640（e580e0）、641–960（e9f4cd）、961–1260（19f47b）、1261–1567至EOF（4c3bd7），各段完整输出未截断。`read-ledger.json`保存逐段字节边界/hash与tool chunk；联合覆盖[0,54632)，不是用旧报告、符号扫描或hash替代新读。全文的77个函数定义以及全部类型、常量、注释和分支纳入下面一份报告。

文件内没有测试模块或测试定义，inline/source tests=0。另完整新读当前直接支持测试 `tests/layout.rs` 1–277（3f6050）和 `tests/layout_persistence.rs` 1–166（4cb931），仅 context-only，共18个已有测试，实际执行0，不新增正式source owner。旧094报告全文也已读并保全；源语义相同的说明与77定义索引经本轮逐段核对后沿用，权威、阅读链、当前TUI映射、历史和交付边界重写。

本轮产品运行、Cargo、PTY、HTTP、release、旧runner均为0；未运行Rust算法或其等价脚本。下文手算与待验证矩阵不是执行证据。旧094及其嵌套历史 source/test/build/PTY/预算数字不重新累加为本轮结果。没有117/131/FIFO/DTrace/预算/预热工作，没有新任务或subagent。

## 身份与状态层

TabId是Project/Goal/Learn/Review/Session五种旧pane preset。ALL固定顺序，as_str与Display输出snake_case名字；它不是目录、project ID、session ID或provider profile。PaneId有19个稳定pane名，as_str/Display给出相应snake_case，is_optional仅Browser/Terminal。显示名与项目路径不在本模块关联。

Viewport/MinSize只是u16宽高值构造；PaneRect保存x/y/width/height，right/bottom用饱和加，contains半开区间，intersects先拒绝任一空rect再做严格交集，边缘相接不算重叠。它们没有屏幕原点偏移、border、鼠标坐标或Unicode排版。任意公有rect可由外部构造；算出的布局在0原点，宿主负责嵌入实际workspace。

Column的index固定Left0/Center1/Right2，FocusDirection的is_horizontal只Left/Right，is_forward为Right/Down。PaneCapabilities默认browser=false、terminal=false；仅是声明能力的布尔值，不探测适配器/权限/进程是否存在。

LayoutModel持tab、ratios、row_weights、collapsed、focused、capabilities；new从tab派生preset并用默认能力，preset每次重建内置列表。with_capabilities/set_capabilities、set_ratios/set_focused、set_collapsed都是低层原样写入，不能因为字段typed就认为状态已经validate。toggle_collapsed返回切换后的collapsed状态。focused_pane直接返回保存字段，不保证当前可见或可用。

PersistedTabLayout仅取ratios/row_weights/collapsed/focused；from_model复制这些，apply_to_model不会覆盖capabilities或tab。ProfileLayoutPreferences的tabs按TabId，LayoutPreferences的profiles按String，默认schema1空map。它们和PersistedTabLayout有deny_unknown_fields；ColumnRatios及legacy LayoutModel没有这一属性。把“所有深层未知字段都拒绝”写成结论不符合源码。

## 五个preset及最小尺寸

所有preset默认column weights=30/45/25。以下每列按row顺序列出`pane:row_weight:min_width×min_height`；右列全为optional。row_weight不是最终占整个viewport的精确百分比。

| TabId | 左列 | 中列 | 右列 |
| --- | --- | --- | --- |
| Project | ProjectConversation:45:24×4；Resources:30:22×3；GoalConversation:25:24×4 | Gantt:100:36×6 | Browser:55:32×6；Terminal:45:32×6 |
| Goal | GoalConversation:50:24×4；Resources:25:22×3；ProjectConversation:25:24×4 | Gantt:70:36×6；EventTimeline:30:36×4 | Browser:55:32×6；Terminal:45:32×6 |
| Learn | LearnConversation:45:26×4；LearnResources:35:22×3；LearnQueue:20:22×3 | LearnMapping:65:36×6；Evidence:35:36×4 | Browser:55:32×6；Terminal:45:32×6 |
| Review | ReviewConversation:45:26×4；Checks:35:22×3；ApprovalQueue:20:22×3 | Diff:100:44×8 | Terminal:55:32×6；Browser:45:32×6 |
| Session | SessionList:45:24×4；SessionConversation:35:26×4；ReplayControls:20:24×3 | EventTimeline:65:36×6；Gantt:35:36×4 | Browser:55:32×6；Terminal:45:32×6 |

LayoutPreset::project/goal/learn/review/session只是for_tab便利入口；pane helper把最小宽高构为MinSize，再PaneSpec::new，后者只把row_weight0修为1。LayoutModel不接受外部LayoutPreset替换来注入任意pane列表；LayoutPreset类型本身仍可序列化/构造用于其它用途。ApprovalQueue这个pane名字不处理真实approval响应，Browser/Terminal规格不启动其adapter。

## 比例、分配和计算顺序

ColumnRatios::new/get/set不校验。bounded先将每个weight至少1，weighted_partition把总数100分给三列，再各夹5..90，按第一个可调整index逐点修补总和100。adjust_column在delta=0时直接返回原值，连bounded也不调用；非零先bounded，以i32计算目标并夹5..90，增长按donor扣至MIN，缩小时给donor加至MAX。Left的donor次序Center→Right，Center为Left→Right，Right为Center→Left；不能概括为总是左右平均分担。

LayoutModel::adjust_ratio仅在结果与旧ratios不同才赋值并return true。adjust_focused_split只处理水平键，挑有效focused或第一个focusable pane，写focused，再按Right+5/Left-5调整该pane所在列；竖直键return false。一个边界是focus可被赋值而比例已到边界不变，最后仍return false，宿主不应把这个返回值泛化为全部状态未变。

allocate_lengths先算minimum总和。够放时先分配min，再把剩余按weight给每项，取整后的余数排序决定额外1格；不足时完全退回weighted_partition，不保证min尺寸。weighted_partition也将0权重视1，按权重切总数；两者remainders以`(remainder,index)`整体降序排序，余数相等时较大index优先，不是第一项优先。空spec/weights返回空Vec，注释“always sums total”须限定非空分配对象；全collapsed布局不会填满workspace。

默认preset至多7pane，列至多3，实际调用total是u16或100、权重u16，乘积提升u32；当前有界调用不等于任意u32 total/无限spec的通用算法保证。helper是私有，报告不扩展其API合同。主循环按已分长度饱和累加x/y，布局没有浮点累计误差，但min+remaining算法意味着最终列宽并非恰30%/45%/25%，行高也非恰45%/30%/25%。旧报告“保持比例”只能指保存权重语义。

compute只是compute_viewport的宽高wrapper，完整流水如下：

1. 由tab取内置preset，按width/height分类Breakpoint，所有pane先依capabilities标Visible或Unavailable。
2. width=0或height=0为Zero，可用pane置Hidden；1..79为Narrow，在有效、可用、未user-collapse的focused中优先选，其次第一个required，再第一个optional，只选一pane，其余可用paneCollapsed。80..99为Compact，右列可用paneCollapsed。100..159 Standard与≥160 Wide进入同一分支，未直接采用不同列数规则。
3. 再施加user collapsed和Unavailable；computed状态不回写用户collapsed，窗口变化不等于持久化折叠设置改变。
4. 非Zero/Narrow时调用collapse_optional_for_width。循环比较active列的raw按比重宽度和满足min分配宽度，任一个小于列max-min宽时，选该列可见optional中min_width最大者折叠，重算直到稳定。若没有可折optional，required可在小视口低于min；这不是将用户ratio偷偷扩大到保证所有optional可见。相同max key最终候选遵循迭代器max_by_key行为，未在本文件实现额外稳定ID排序。
5. 收集Visible、按Column并按row排序，仅active columns参与分配；空列不占位置，右列单独可见时也从x0开始。每列min取可见pane最大min_width，row_weights覆盖preset并在计算时夹1..1000。
6. 所有preset pane都返回PaneLayout；非绘制pane默认零rect，可见pane分到rect。height很小时某些Visible pane可得0行；Visibility不是rect非空或最小尺寸达标的同义词。

is_available对非optional恒true，对Browser/Terminal看能力；其它optional spec默认true，但内置preset没有额外种类。PaneState::new和is_available辅助保存computed可用性。Visibility有Visible/Collapsed/Unavailable/Hidden四种，需保留原因差别。LayoutSnapshot::pane按ID查找；visible_panes只过滤枚举，不过滤empty；visible_rects_non_overlapping两两检查rect交叠，空rect自然不相交。因此“非重叠通过”并不证明每块有内容、满足min、坐标一定在viewport内或实际UI没有标题遮盖。

## 焦点、折叠和reset

focusable_panes先compute，再按preset顺序过滤不可用及user-collapse。Narrow返回全部可用未collapse候选，使只有一块可见时仍可切换；其它断点仅返回Visibility::Visible，可能含height极小的零rect。focus_next/previous经focus_cycle环绕；无旧focus时next取首、previous取尾。无candidate返回None但不主动清空self.focused，调用者需要区分返回结果和保存字段。

focus_direction无候选返回None；旧focus无效时总取candidate首项。有效focus有非空rect时，以rect中心判断所选方向，先偏好垂直轴相交，再主轴距离，再次轴距离；相同score不替换先遇到候选。未找到方向项或narrow其它候选没有rect，则按direction.is_forward环绕，不把用户困在唯一visible pane。它是几何导航，不检查crossterm modifiers或modal焦点。

set_collapsed/toggle允许任意PaneId写到集合，没有所属tab检查，compute只处理preset中现有pane；这些无效值可以静默不影响几何，却在持久化validate被拒绝。低层折叠不修focus，不阻止全pane折叠。reset_layout恢复preset ratios、清row_weights/collapsed/focused，保留tab/capabilities；reset仅别名。模型没有undo journal或磁盘写入，reset的错误回退由host保存层处理。

## JSON、迁移与失败原子性

LAYOUT_SCHEMA_VERSION=1；输入/输出序列化文档限制256KiB，MAX_LAYOUT_PROFILES=64，MAX_LAYOUT_COLLAPSED_PANES=64。profile None映射`default`，名字必须1..128字节且全部ASCII alnum/下划线/横线，区分大小写，不接受路径、空格或中文。布局profile属于旧provider profile维度，不能把稳定project ID当任意目录直接填入。

validate按schema、profile数量、profile name、tabs数量、每tab状态验证。tab最多TabId::ALL.len=5；PaneId只有19种且单preset最多7，因此64 collapsed限额是防御上限，正常typed BTreeSet无法靠重复pane绕过；更早的未知pane所属检查实际更严格。ratio每项5..90且饱和总和100；row_weights必须属于该preset、值1..1000；collapsed/focused必须属于tab。不要求focus未collapsed或optional capability可用，也不要求有任何展开pane。InvalidRatios也用于非法row_weight，错误类型不是精细行权错误。

from_json_bytes调用with_migration并丢弃bool；with_migration在解析Value前检查输入len，随后以“schema_version键是否缺失”计算needs_migration，再migrate_value，再validate。解析使用Value中间态，报告不宣称重复JSON key检测或所有unknown深层字段拒绝。当前无文件I/O，解析legacy也不会自动回写磁盘。

migrate_value把schema_version的as_u64再转换u16，太大转u16::MAX后报SchemaVersion；有效数字1按v1结构deserialize。其余None分支deserialize为旧LayoutModel，再preferences.set_model(None,&legacy)。关键静态差距：缺失、null、字符串、负数或小数可能都得不到as_u64，但needs_migration只看键缺失；legacy LayoutModel没有deny_unknown_fields，如果同时有合法legacy必需字段，异常schema键或其它额外字段可能被忽略，返回的migrated还可能false。不能沿用注释“unknown legacy一律拒绝”。这是根据分支提出的待验证输入族，本轮没有执行Rust/等价行为模型，不写成已复现安全漏洞。

legacy所需tab/ratios/collapsed/focused/capabilities由Serde决定，其中row_weights有default；迁移后PersistedTabLayout故意丢弃capabilities，再model_for重建默认false，宿主需重新施加。已有legacy测试默认capability false，因此不能替代enabled capability迁移边界。v1外层deny_unknown_fields不扩展到未标注的ColumnRatios内部。

to_json_bytes先validate，再pretty serialization，追加换行后检查256KiB。输出检查发生在构造Vec后，不能声称从不分配超过上限。BTreeMap/BTreeSet提供稳定序，与序列化规则共同使相同值重复编码确定。set_model先验profile和新state、验self，再clone next、比较同值return false、插入并验证next、最后一次性*self=next；新profile第65个失败时原值不替换。它不调用to_json_bytes，内存模型验证成功不保证最终pretty文档一定在字节限制内。

model_for先全validate，再构建tab preset并应用保存值；即使只想取某个profile，其它profile非法也会失败。tab_state只检查所请求profile key，不全validate，所以公共构造的非法document可被该只读getter返回。reset_tab和reset_profile也只验证请求名字，不先validate整个document，允许从现有对象移除记录；reset_tab删除后empty profile移除。对预先存在的空profile，删除不存在tab可能返回false却移除空profile，这是低层返回“tab是否删除”的语义，不保证整对象没变化。

LayoutError包括SchemaVersion、InvalidProfile、TooManyProfiles、TooManyTabs、InvalidRatios、TooManyCollapsed、UnknownPane、Json，thiserror提供文字。不触及credentials、approval决策或磁盘权限；注释里的“损坏layout不破坏provider”依赖config把文件隔离，不能仅由本文件证明所有host路径成立。

## 当前 TUI adapter 的有界映射与职责边界

本轮 TUI 上下文只读快照为669038 B /16374行，SHA256 `6459360343a260c6dfe7b403a81cd22a8fcac0bb3e10fcabed2bd3c0b8567254`，与layout同次捕获。主控明确正在独立修F2；这个快照已出现共享 session_browser_scroll 和 SessionList 单行绘制，属于其进行中的产品delta，不是本worker修改或验收。仅下列有界片段新读；完整TUI原件仅用于确定片段身份，不声称本轮全文阅读TUI。抓取后可能继续漂移，原capture不覆盖。 交付前一次只读观察得到TUI 669057 B /SHA256 `7c0d9e5c2cb5f351412aa6e98ac233f5c55580ee1885a431653d8c91b908ba4b`；完整新读的有界diff仅将history_search_key字符guard增加SUPER，属于主控集成F1，不改变本报告布局片段。layout/蓝图/selector身份未漂移；差异和观察字节另存master-state，未运行其产品。

| 当前调用位置 | 已读行为及布局映射 | 所属与边界 |
| --- | --- | --- |
| 3438–3506 layout getters/restore/dirty/reset intents | 当前model及legacy preset集合；restore重建Project并给每个model重施当前capabilities，最后清dirty/reset；保存成功清bit与render dirty独立 | typed restore本身不validate；磁盘I/O与项目恢复不在纯模型内 |
| 3566–3779 focus/split/reset/preset/collapse/save/summary/capabilities | next/previous/direction返回Some即标dirty；split只在ratio changed标dirty；focus和collapse先验preset成员及optional可用；展开后聚焦，折叠当前focus时尝试next，无候选仍留原focus则清None；reset记录tab意图；set_workspace_tab保存旧preset，重施capabilities且不变项目身份 | wrapper比model setters严格；同状态collapse返回true代表有效请求，不代表发生变更；request_layout_save只是标记 |
| 3632–3640 workspace_viewport | last_area双0时fallback160×40，否则取last_area尺寸 | last_area由render_bentobox存完整frame，实际adapter取扣除头尾/input后的workspace_area；两者高度不同，不能声称键盘与绘制始终用同一height。是否造成焦点偏差需后续产品owner实证，本轮不修 |
| 5847–5898、5960–6020、6095–6185键盘有界片段 | Release早退；approval、history、picker、transcript browser有先行消费；slash菜单先行；在CONTROL分支且input空时Ctrl方向几何导航，CtrlShift水平调整，CtrlShift竖直仍导航，Ctrl0 reset；空prompt Tab/ShiftTab/BackTab循环pane；CtrlTab循环project | 不是全handle_key审计；不是所有modifiers/终端字节或modal情形都由此证明；模型不负责按键编码或Submit |
| 5386–5646完整handle_mouse | picker/browser/follow、palette/project hit前置；workspace adapter给pane hit；左键up清drag；行缝两格容差记录相邻行及原权重，拖动只转移该pair行权；列缝记录bounded ratios，按workspace宽度折算拖动并保留pair总量；pane点击聚焦；滚轮SessionList移动cursor±3、conversation滚transcript，其余pane_scroll±3 | 两格grip、Ratatui Position、滚动与draft/内容都是TUI策略，不在layout纯几何里；此处新读是上下文，不接管F2或拖拽产品验证 |
| 7292–7335 render_bentobox | 零frame返回；保存last_area；按投影wrap计算动态input高度；tabs/header各1、workspace Min1、input动态、footer1；内容后绘picker/browser/approval | 不改现有BentoBox；overlay优先级、Unicode/wrap和IO需对应owner |
| 7426–7524 render_workspace/render_workspace_pane/scroll | 保存workspace_area；零面积返回；adapter visible pane再排除零rect；Goal专用transcript、其它conversation走普通transcript，其余pane按内容owner取值；SessionList特殊scroll和不wrap，其他内容wrap | layout仅决定pane身份/矩形/visibility；Checks/ApprovalQueue/Diff等占位内容不能因模型有pane定义就算业务已实现 |
| 7526–7550 session_browser_content | 空列表No sessions；最多32项，cursor前加选中标记，拼session_id/turns/events/next，每项一行 | session数据来源/扫描/取消/会话打开在host；不把SessionList pane存在当作数据或状态验收 |
| 7790–7875 PaneFrame/BentoBoxLayoutAdapter/translate/conversation mapping | new用area宽高compute；保留所有pane状态并translate；offset夹到area，坐标饱和加，width/height夹剩余；visible_panes只过滤Visible；pane按ID查找，tab/breakpoint/snapshot/area为getter；conversation按5preset映射 | model在0原点；adapter引入实际区域原点并裁剪。render还要过滤empty，不能仅由Visible证明可绘 |

SessionList 的职责必须拆开：layout preset只规定Left row0、weight45、minimum24×4，和SessionConversation/ReplayControls共享左列；它完全不知道session_browser数组、cursor、32项上限、border减2、pane_scroll或哪一行点击哪条session。捕获TUI的render与mouse都调用session_browser_scroll，mouse再加减去上边框的内容行号，并只接受内部边框坐标；这解释F2修复落在TUI和项目workspace测试，而非layout。主控正在运行的反例/回归不在本包，不推断通过，不覆盖其源码。

模型的输入输出可用于headless/未来GUI，但本轮只证明模块不导入ratatui/crossterm且函数可被普通Rust调用；没有逐读headless endpoint，因此不把设计复用目标表述成某个已上线协议。LayoutPreferences内没有文件I/O或provider秘密；支持测试调用config保存层的安全文件规则，不以测试名称替代当前config完整审阅。本轮未新读config owner，旧094 config片段仅历史。

## 静态边界与手算示例（未执行）

Project在200×40、两optional能力启用时，raw列宽为60/90/50，均满足各列max minimum 24/36/32；实际分配先扣minimum总92，余108按30/45/25分成32/49/27（最大余数给中列），所以最终56/85/59。左列先扣高度4+3+4=11，余29按45/30/25得到13/9/7，最终17/12/11。此为阅读源码后的手算，不是运行样本，说明“ratio”是剩余空间权重而非最终百分比。

需要保留的分支限制：Zero导致可用pane Hidden但不改model；Narrow全user-collapse可以没有pane；低高度Visible可零rect；focus_candidates为空返回None但可能保留旧focused；adjust_focused_split可改focus却因ratio不变返回false；delta0不做bounded；reset_tab删除预存空profile时可能返回false；tab_state/reset并不全validate；model compute容忍非法权重并不等于preferences允许保存；schema存在却非as_u64时legacy回退与needs_migration标记可不同步。上述均按实现说明合同，后续验证输入见矩阵，本轮没有复现新缺陷、修改产品或提升风险等级。

取消/恢复/错误的适用性：本文件没有异步任务、网络、子进程、磁盘写入、终端模式或取消token，因而不存在本文件独立取消/资源reap/TTY恢复实验；同步几何计算和内存修改由host调用。可观察恢复是model_for/legacy迁移/reset及capabilities重新应用，持久化原子写入和失败恢复归config/项目checkpoint owner。typed LayoutError由调用者传播或展示，模型没有向用户弹窗或悄悄写默认文件。

## 外部测试全文阅读（18项，本轮执行0）

以下测试是现有支持性代码，完整文件已封存，未升格为新增正式owner或源内联测试。表内断言数是文本调用数，loop迭代不会重复计数。

### tests/layout.rs — 11 tests

- `every_workspace_tab_has_a_named_preset_and_required_panes` L7–29，7断言：五preset具名/非空、需要的Resources/Gantt及Browser/Terminal、min正数；不验每一精确权重。
- `breakpoint_bands_include_zero_and_narrow_resize_states` L32–40，7断言：0、79/80/99/100/159/160边界；未单独试width非零height0。
- `wide_project_layout_respects_ratios_minimums_and_has_no_overlap` L43–64，9断言：200×40 full caps，min、边界、列顺序及非重叠；没有断言精确30/45/25百分比。
- `optional_right_panes_are_unavailable_by_default_and_collapse_on_compact` L67–93，6断言：默认Unavailable、enable后90×30右列Collapsed。
- `narrow_layout_keeps_one_focusable_pane_and_zero_viewport_is_hidden` L96–118，5断言：Goal EventTimeline选中60×20单pane，0×0 Hidden。
- `custom_ratios_collapse_optional_panes_when_minimum_width_cannot_fit` L121–137，3断言：1/98/1在120×30令optional不Visible；只是低层非法持久化权重的compute例。
- `fully_collapsed_state_is_safe_at_a_normal_viewport` L140–153，2断言：required全折叠时无visible且非重叠，证明不是强制保底一pane。
- `pane_focus_cycles_in_preset_order_and_skips_collapsed_or_unavailable_panes` L156–202，7断言：200×40序列/next/previous、skip collapse及关闭能力。
- `directional_focus_prefers_same_row_or_column_before_falling_back` L205–235，5断言：上下左右几何偏好及60×20窄屏fallback；不是所有tie/zero-rect组合。
- `interactive_ratio_adjustment_preserves_bounded_total_and_reset_restores_preset` L242–265，9断言：0/98/1 bounded，Left+100到MAX再次不变、reset；不覆盖delta0无归一化。
- `focused_split_adjustment_uses_horizontal_arrows_only` L268–277，5断言：Down无比例修改，Right到left35、总100。

### tests/layout_persistence.rs — 7 tests

- `profile_and_tab_states_round_trip_independently_and_reset_is_idempotent` L13–48，8断言：tempdir中alpha/beta与Project/Goal隔离、reset幂等；不是稳定project同profile隔离。
- `invalid_state_is_rejected_without_replacing_a_valid_snapshot` L51–64，4断言：0/100/0存失败，磁盘bytes及有效model保留。
- `corrupt_and_oversized_documents_fail_closed_without_mutation` L67–86，4断言：非法JSON、256KiB+1文件读取拒绝并保持原件；不含rename故障注入。
- `out_of_range_and_unknown_pane_values_are_rejected` L89–107，2断言：left4与not_a_real_pane拒绝；未知enum字符串不同于有效PaneId但属于错tab。
- `legacy_layout_model_is_migrated_to_default_profile` L110–130，5断言：默认capability Goal旧model读回，显式migration true再false，schema/default profile。
- `layout_symlink_is_rejected_for_read_write_and_reset` L134–153，6断言：cfg(unix)，tempdir外指symlink读写reset拒绝且outside字节不变；不是所有平台/TOCTOU。
- `layout_preferences_json_round_trip_is_bounded_and_deterministic` L156–166，3断言：空默认preferences重复输出相同且<上限、读回等值；不是巨大profile边界。

没有新增测试或执行现有测试，外部测试通过与否不能从assert源码推断。等待后续产品owner运行时须绑定实际binary/config/项目状态、保存失败原件，并验证独立重启与真实TUI；纯模型测试不能替代键鼠操作/原始终端恢复。

## 旧094历史的原样保全

整个旧 `.ops/zs1-094-ready` 原样复制到本包 `historical/zs1-094-ready/`，旧manifest为8800 B /SHA256 `013e06a06e650f9d17b606021b7c2f444eb43ee806ce5055443a949828375fc5`。包括旧3.1.20报告、source/test读记录、旧静态脚本/审计及嵌套历史证据；本轮不执行其中任何程序。旧worker标准报告另存worker-before，主库标准报告捕获时不存在，故master-before为absent；不能伪称主控此前已接收旧094。

历史release/PTY/预算失败与通过记录仍在旧包原位置，其语义限定于各自当时输入。本轮既不重验其产品结果，也不累加旧source/test/断言/运行数量。原报告中的77定义和18外部测试是同一组源实体，不因重新阅读成为154定义或36个不同测试。新离线完整性审计仅核对复制字节，不能把旧结构passed提升为新的行为passed。

## 最小实验矩阵（10组，均未执行）

### P01 — 断点与几何可见性

输入：0×N/N×0，79/80/99/100/159/160及height1，5preset×4capability组合。判据：检查Visibility、rect边界/非重叠，不将Visible等同非空或满足min。本轮未执行，只是静态阅读后交给owner的验证要求。

### P02 — 最小值和权重分配

输入：200×40 Project含optional、极端1/98/1、零权重、余数并列、全collapse。判据：minimum先占位后分剩余；不足时退weighted；余数并列后index优先；允许无可见pane。本轮未执行，只是静态阅读后交给owner的验证要求。

### P03 — 用户折叠与能力变化

输入：Browser/Terminal enable→collapse→disable→enable，窄屏焦点及全折叠。判据：Unavailable不被展开强制启用；computed collapse不写model；恢复显式state不丢失。本轮未执行，只是静态阅读后交给owner的验证要求。

### P04 — 焦点与返回值边界

输入：旧focused不属于tab/已collapsed/zero-rect，方向无候选，调整已达ratio边界。判据：None可能保留旧focused；adjust_focused_split可改focus却return false；宿主dirty语义需逐项看。本轮未执行，只是静态阅读后交给owner的验证要求。

### P05 — 交互比例与持久化校验不同

输入：adjust delta0、±极值、任意weights；row_weights0/1001/未知pane。判据：delta0不归一化；非零bounded到5..90和100；compute夹行权但存储拒绝非法。本轮未执行，只是静态阅读后交给owner的验证要求。

### P06 — 偏好事务与大小边界

输入：profile64/65、0/128/129bytes/非ASCII、invalidstate set_model、pretty JSON 256KiB边界。判据：set_model验证/clone/commit保持失败原state；不等同to_json可写；None和default同key。本轮未执行，只是静态阅读后交给owner的验证要求。

### P07 — 迁移与未知字段

输入：legacy带capabilities，v1未知顶层/嵌套ratio字段、legacy未知字段、schema缺失/null/string/负数/超u16。判据：分别观察decoder/validator与migrated标志；不以注释断言一律拒绝；capability丢弃后由host施加。本轮未执行，只是静态阅读后交给owner的验证要求。

### P08 — 恢复与重置

输入：项目A/B同provider不同ratio，preset切换、能力重启变化、reset后写失败。判据：profile不是project身份；准确保存host key；reset不抹capability/用户其它项目。本轮未执行，只是静态阅读后交给owner的验证要求。

### P09 — 真实TUI尺寸和焦点

输入：顶部两行/动态prompt/footer缩减content，resize、拖拽、collapse和菜单overlay。判据：传入layout应是workspace尺寸；真实键鼠owner保留draft/layout，不用纯geometry通过抵PTY。本轮未执行，只是静态阅读后交给owner的验证要求。

### P10 — 磁盘与历史证据边界

输入：symlink、权限/rename失败、损坏/oversize、独立重启，历史1d7b2结果对照。判据：存储由config/host负责；现有历史不覆盖漂移菜单；所有本轮运行0，不伪造G-STAGE接受。本轮未执行，只是静态阅读后交给owner的验证要求。

## 逐定义索引

下表只作定位与完整性索引，不能替代前文整体算法、状态转换与错误边界。重名方法按impl分开，不按函数名去重。

| 行 | 所属 | 定义 |
| --- | --- | --- |
| 44 | `TabId` | `as_str` |
| 56 | `std::fmt::Display for TabId` | `fmt` |
| 89 | `PaneId` | `as_str` |
| 113 | `PaneId` | `is_optional` |
| 119 | `std::fmt::Display for PaneId` | `fmt` |
| 149 | `PersistedTabLayout` | `from_model` |
| 158 | `PersistedTabLayout` | `apply_to_model` |
| 187 | `Default for LayoutPreferences` | `default` |
| 221 | `LayoutPreferences` | `validate` |
| 249 | `LayoutPreferences` | `from_json_bytes` |
| 256 | `LayoutPreferences` | `from_json_bytes_with_migration` |
| 272 | `LayoutPreferences` | `to_json_bytes` |
| 287 | `LayoutPreferences` | `tab_state` |
| 301 | `LayoutPreferences` | `model_for` |
| 312 | `LayoutPreferences` | `set_model` |
| 334 | `LayoutPreferences` | `reset_tab` |
| 347 | `LayoutPreferences` | `reset_profile` |
| 355 | `LayoutPreferences` | `migrate_value` |
| 379 | `module` | `validate_tab_state` |
| 427 | `module` | `layout_profile_key` |
| 433 | `module` | `validate_layout_profile_name` |
| 458 | `Breakpoint` | `for_size` |
| 472 | `Breakpoint` | `for_width` |
| 485 | `Viewport` | `new` |
| 501 | `PaneRect` | `new` |
| 510 | `PaneRect` | `is_empty` |
| 514 | `PaneRect` | `right` |
| 518 | `PaneRect` | `bottom` |
| 522 | `PaneRect` | `contains` |
| 526 | `PaneRect` | `intersects` |
| 544 | `MinSize` | `new` |
| 559 | `Column` | `index` |
| 582 | `FocusDirection` | `is_horizontal` |
| 586 | `FocusDirection` | `is_forward` |
| 601 | `Default for ColumnRatios` | `default` |
| 611 | `ColumnRatios` | `new` |
| 631 | `ColumnRatios` | `get` |
| 639 | `ColumnRatios` | `set` |
| 652 | `ColumnRatios` | `bounded` |
| 686 | `ColumnRatios` | `adjust_column` |
| 753 | `PaneSpec` | `new` |
| 782 | `LayoutPreset` | `for_tab` |
| 864 | `LayoutPreset` | `project` |
| 868 | `LayoutPreset` | `goal` |
| 872 | `LayoutPreset` | `learn` |
| 876 | `LayoutPreset` | `review` |
| 880 | `LayoutPreset` | `session` |
| 885 | `module` | `pane` |
| 926 | `LayoutModel` | `new` |
| 937 | `LayoutModel` | `preset` |
| 941 | `LayoutModel` | `with_capabilities` |
| 946 | `LayoutModel` | `set_capabilities` |
| 950 | `LayoutModel` | `set_ratios` |
| 954 | `LayoutModel` | `set_focused` |
| 959 | `LayoutModel` | `focused_pane` |
| 969 | `LayoutModel` | `focusable_panes` |
| 993 | `LayoutModel` | `focus_next` |
| 998 | `LayoutModel` | `focus_previous` |
| 1006 | `LayoutModel` | `focus_direction` |
| 1084 | `LayoutModel` | `focus_cycle` |
| 1104 | `LayoutModel` | `adjust_ratio` |
| 1117 | `LayoutModel` | `adjust_focused_split` |
| 1146 | `LayoutModel` | `reset_layout` |
| 1154 | `LayoutModel` | `reset` |
| 1158 | `LayoutModel` | `set_collapsed` |
| 1166 | `LayoutModel` | `toggle_collapsed` |
| 1175 | `LayoutModel` | `compute` |
| 1179 | `LayoutModel` | `compute_viewport` |
| 1343 | `LayoutModel` | `is_available` |
| 1362 | `PaneState` | `new` |
| 1373 | `PaneState` | `is_available` |
| 1406 | `LayoutSnapshot` | `pane` |
| 1410 | `LayoutSnapshot` | `visible_panes` |
| 1416 | `LayoutSnapshot` | `visible_rects_non_overlapping` |
| 1426 | `module` | `collapse_optional_for_width` |
| 1495 | `module` | `allocate_lengths` |
| 1539 | `module` | `weighted_partition` |

## 唯一报告补丁、回滚与交付

本包只将本标准报告作为owned产物，`candidate/`内恰有此一路径。`master.patch`基于主库捕获absent新增报告；`worker.patch`基于worker-before的精确旧字节更新；对应reverse patch供仅此报告回滚。补丁由精确前后文本生成并由新离线结构核验重建比对，不宣称运行过git apply或产品测试。接收时如主库已有报告，主控先保存新前态并重新合并，不force、不用整树覆盖；回退只恢复该报告前态，绝不reset/stash/clean主库、用户会话或其它项。

`read-ledger.json`是连续全文阅读记录；`context-ledger.json`绑定有界上下文；`capture.json`封存权威及输入身份；`historical/`保全旧094；`master-state/final-observation.json`仅记录交付前有限漂移，未覆盖capture。新verifier完整静态阅读后，冻结包最多运行一次offline结构核验，stdout/stderr/exit/hash在包外；失败原样保留且不重跑。该结构结果不替代Master逐函数/分支语义G-FILE和G-STAGE。报告交付后停止，后续产品验证须另由主控分配。
