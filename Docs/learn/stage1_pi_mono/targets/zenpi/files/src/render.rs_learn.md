# ZS1-096 · src/render.rs 当前完整文件复核（3.1.21）

当前模块已经提供头部与尾部两组渲染接口。尾部路径在接受的前256KiB输入范围内保留最新视觉行，保持已解析的代码/行内样式，并返回原视觉行偏移，供TUI稳定阅读位置。它还将角色前缀计入cell预算，按扩展grapheme换行，缓存未闭合链接的扫描位置。旧096报告基于901行版本，不能直接代表当前1706行实现；其原文、证据、失败和后冻结结果全部保留。

本次完整顺序读取当前主库 `src/render.rs` 的60098 B /1706 L，SHA-256 `b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d`。正式读段为L1–250、251–500、501–750、751–1000、1001–1250、1251–1500、1501–1706，精确覆盖[0,60098)。61连续语义单元、67函数声明（42生产、2测试helper、23测试）、77普通assert文本位置、0快照；所有源码测试仅阅读，运行0。数字用于核查完整性，以下逐分支行为与判据才是语义证据。

唯一owned报告为 `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/render.rs_learn.md`；本次只交付该报告的私有创建候选，主库和继承报告不改，产品运行/Cargo/PTY/HTTP/预算/旧runner均0。G-FILE/G-STAGE仍由master审阅，本报告不勾选权威状态。离线冻结审计结果只在包外记录，不能代替语义接受。

## 身份、历史与适用范围

| 对象 | 身份/用途 |
|---|---|
| 当前正式文件 | `/Users/wangweiyang/GitHub/zenpi/src/render.rs`，60098 B /1706 L /`b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d` |
| 蓝图登记基线 | 30217 B /901 L /`6eaaa712745659ae48f429217c5eb8961dd12e71789f8a2bc70867ee71ba25a7`，蓝图L121与source_manifest第24行；保持原登记。当前相对基线净增29881 B/805 L，精确diff另存 |
| 规则 | 3.1.21，run `zenpi-stage1-20260911`，requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`。捕获blueprint snapshot `d0eb898bdc232d122e08a9b7445c5aa13e8f767fcff0a0ef60752297e03a5088`；冻结时如漂移另记，不改旧捕获 |
| 096 owner | 蓝图L212：L1依赖001，唯一上述报告；回退仅撤本报告。L139要求全部内容、接口/错误/副作用/取消/恢复、测试或可运行判据及owner映射，hash/纯摘要/compile不替代语义 |
| 当前TUI上下文 | 668438 B /16361 L /`e45131dcf190d55068e369e55b8a3cc0dda18234ccee2c2ca335c9cefdd15904`，仅按read-binding有界阅读，不当085整文件 |
| 旧096 | 117文件ready及2文件postfreeze原样复制；19238 B/152 L旧最终报告完整新读。继承报告与其相同；15997 B/105 L旧working报告是精确字节前缀，已在最终报告阅读中覆盖 |
| 已独立复核308 | key_hint.rs 3306 B/112 L/`07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8`；完整复制原149文件包并核与当前源同字节，新读111行报告；不计其源码阅读为096 |
| 已独立复核309 | status_indicator_widget.rs 15810 B/440 L/`e35e3efbd18784b55343366b2001bda08dfc1543f7e4a88700a715562d519cc5`；完整复制原216文件包并核当前源同字节，新读137行报告；7源测试/3快照不计入096 |
| 125局部主控记录 | 已读37行 `Docs/quality/stage1/ZS1-125/master-approval-timer-3.1.21/review.md`。该记录接受审批等待计时/footer局部修复，记195不同Rust测试和最终PTY14检查通过；保留旧二进制mtime误复用与首次PTY额外journal统计失败。都是主控历史证据，不是本次运行，也没有关闭整个125 |

旧基线本次作完整字节比较，不另宣称新读其901行。各上下文完整复制是身份绑定，不自动取得全文阅读信用。旧308/309报告写的是当时候选状态；后续master已接受的通知与原文各保留时序，不能修改原报告来伪造当时接受。权限、项目/会话状态、终端模式及UI布局不在render.rs内，相关映射只以读到的调用点为限。

## 输入输出与模块边界

L1–29依赖Ratatui Line/Span/Style、Unicode scalar宽度及扩展grapheme分割、VecDeque；四常量为输入256KiB、默认8192行、近似4Mi cells、width最大65535。MarkdownBlock拥有Paragraph、Heading(level/text)、Code(language/text)、Quote、ListItem(ordered/marker/text)、Rule六种变体。RenderedTail L265–269保存拥有的lines和omitted_visual_lines；StyledSegment为私有text/style；LinkScan持有两个绝对closer位置及仅测试时存在的scanned_bytes。

全部生产接口纯内存：不写终端、不打开URL、不读文件、不执行代码、不复制到系统剪贴板、不接收任务取消、不保存会话。没有Result或业务错误类型；无效/未知Markdown倾向字面显示，输入与输出上限倾向截断，分配失败/panic不被本模块捕获。parse只返回块，render只返回拥有的静态生命周期Line/Span，不意味着绘制已完成。所有宏测试体、cfg(test) helper与测试计数字段已读，未执行。

| 公开路径 | 输入处理与输出 | 关键边界 |
|---|---|---|
| parse_markdown L57–61 | 先按UTF8字符边界保留前MAX_MARKDOWN_BYTES；sanitize_text再按同上限截；调用bounded解析 | 返回整个接受范围的blocks，无block数量限制或truncated标志；不是完整CommonMark解析器 |
| render_markdown L139–141 | 调with_limit，默认8192行 | width在下层钳1–65535；保留头部，不是最新尾部 |
| render_markdown_with_limit L679–753 | requested至少1，并取4Mi/width的较小值；解析后逐block输出，到行上限停止，空结果补一行 | 显式requested可超过8192，0也返回至少一行；max_lines不约束此前完整parse/inline/wrap的临时工作量 |
| markdown/plain prefixed L150–231 | width钳制；prefix按scalar/cell截至width−1，预留body至少1cell；首视觉行角色前缀、后续同宽空格 | 这两条头部接口不sanitize或byte限制prefix，预算按body宽算，追加prefix后可超过4Mi cells；当前读到的生产caller用固定短/空prefix |
| tail prefixed L241–259 | 委托metadata版本，取.lines | 丢掉偏移信息；只是同一尾部算法的便利wrapper，不额外处理显示提示 |
| tail metadata L273–291 | 分别传markdown=true/false到render_tail_metadata，返回lines+偏移 | 偏移相对于接受输入、当前width及解析布局；width变化或早期Markdown重解析后不能当永久文本锚点 |

plain路径保留`#`、fence、`---`等字面，不主动解释Markdown；Markdown路径隐藏匹配的行内标记和链接target是设计行为，不是原文逐字展示。正文控制字符清理不等于所有Unicode视觉混淆被消除，也不保证任何外部prefix都已安全处理。

## 块解析状态与全部语法分支

parse_markdown_bounded L63–132持有paragraph行列表和可选(fence char、长度、language、code行列表)。逐bounded.lines扫描，先在code状态检查关闭：同字符、长度≥开fence且无language才结束；其它内容逐行保留，随后continue。普通状态依次判断开fence、blank、heading、quote、list、rule，否则加入paragraph；每次转块先flush paragraph。EOF未闭code仍生成Code，最后flush剩余paragraph。空白行不是原样空块，render阶段按block之间插入单空行，因此源空白布局不保证逐字保留；`.lines()`不保留末尾空行，plain wrap则保留最后newline之后的一行。

| helper/范围 | 识别规则 | 保真/复杂度边界 |
|---|---|---|
| flush_paragraph L755–761 | 空则return；否则newline join为Paragraph，再clear | 多行paragraph保内部换行，空段不输出 |
| heading_info L763–777 | trim_start，1–6个ASCII #且后跟whitespace，内容trim | `#word`与七个#不识别；不解析尾部#、嵌套结构或完整CommonMark缩进规范 |
| quote_info L779–783 | trim_start后取一个>，最多去一个ASCII空格 | 剩余>不递归变嵌套quote |
| list_info L785–816 | 无序-/*/+后whitespace；有序ASCII数字串+`.`+whitespace，保留marker字符串并trim正文 | 不把数字parse为整数，不会整数溢出，但marker可很长；无序显示统一-；无嵌套/续行归并 |
| is_rule L818–827 | trim后字节长度≥3，首字为-/*/_，其余仅同字或whitespace | 不是“至少3个标记字符”的完整判据，而且在list之后；带空格的某些横线会先识别为list |
| starts_with_whitespace L829–831 | 首Unicode字符is_whitespace | 不是仅ASCII空格 |
| fence_info L833–859 | trim_start后至少3个连续反引号或~；余文trim后第一个whitespace token为language | token仅做标签，不加载语法高亮；带language的同形行不关闭已有fence；开/关字符及长度严格按此子集 |

inline_segments L968–1052按UTF8字符推进，plain缓存遇成功格式才flush。优先处理词内underscore保护，再backtick、双星、双下划线、单星、单下划线、链接，失败保留当前字符。成功content直接生成指定Style而不递归解析，消费长度用byte；previous_non_underscore基于此前非下划线字符（成功片段trim末尾underscore后再更新）决定词内underscore。独立 `_italic_` 可以识别，标识符中的underscore保持字面；这不是完整转义/嵌套强调语法。

parse_delimited L1054–1066要求开delimiter，找第一个同delimiter作close，空content拒绝，返回拥有的content和已消费byte数；`_marker`形参未使用。code浅黄，bold/italic叠加modifier，link浅蓝且下划线。links返回label，丢弃target，不打开、不验证target的scheme、存在性或可信度；展示文字不构成目标认证。

LinkScan L1068–1107替代旧parse_link重复suffix搜索。closer仅在cached<start时向后find，找不到缓存text.len；parse从index+1查`]`，必须马上接`(`，再查第一个`)`且target非空；label可以空，目标嵌套括号/escape不是完整语法。一个inline_segments只用同一text、单调前进index的同一LinkScan，正是缓存有效的前提。T20为多组未闭合/无目标/缺close/空目标统计两类扫描总byte≤2n；它证明对应closer路径的工作量边界，不是整renderer实测延迟、所有语法的复杂度定理或UI帧率。旧报告“很多未闭合[每次重扫”对应代码已变，不继续把旧定位当当前未修复问题。

## 头部渲染、样式和临时工作

render_markdown_with_limit对每个块先在非首块插空行：Paragraph调用render_inline_text；Heading先heading_style；Code调用render_code；Quote用`| `、DarkGray；List有序保marker、无序统一`- `；Rule生成width个ASCII横线、DarkGray。外层达到max_lines后退出。heading_style L1175–1182对level1 LightCyan、2 Cyan、其余LightBlue并BOLD；不生成终端escape字符串。

render_inline_text L867–882先完整inline_segments，再完整wrap_segments，才逐行push并停。render_prefixed_inline L884–918计算marker/body宽，首行marker BOLD、续行空格，line构造后再push_line；其总行限制来自外层完整width，区别于公开角色prefixed按body求预算。render_code L920–966先可选[language]一行，代码统一LightYellow；空文本至少一行，其余先split并collect所有物理行；每物理行wrap_plain，首视觉行`| `、续行空格，到行上限return。language是普通显示标签，无lexer/执行。

wrap_segments L1109–1147按Unicode scalar遍历，在newline完成一行；单scalar宽于viewport变`?`，下个非零宽字符溢出则换行，同style合并Span；最后始终再push一行。零宽scalar在行首仍保留；它没有将跨style的组合字符拼为grapheme。wrap_plain L1149–1173供已分物理行的code使用；过宽char可先结束已有内容再输出`?`，其余按scalar宽分String，也生成完整Vec。truncate_to_width L1247–1259同样按scalar累加，零宽char不消耗cell，无byte cap或清理。push_line L1184–1188只拒绝追加到满Vec，但入参Line已先构造，因此不是防临时分配的屏障。

这些路径保留原始头部API行为；返回行数有界与中间解析、所有换行结果的分配量有界到同一数值不是同一命题。256KiB正文限制仍约束输入规模，但窄列可临时生成多于8192条Line/String。旧头部prefix问题也仍只适用于这些API：公共prefix可包含零宽/控制字符或很长文本，追加缩进未计最终cell预算；当前TUI的聊天主路径已经改用尾部API，不能把旧头部风险泛化为当前所有对话内容可触发。

## 尾部状态机、视觉行计数与资源约束

render_tail_metadata L304–381先钳width；对prefix先截输入byte再sanitize、换行改空格，tail_prefix_to_width按grapheme裁至width−1，重新计算实际prefix宽；body至少1cell。行limit按最终完整width和默认8192计算，包含每行角色缩进。正文只截接受的前256KiB，然后sanitize_compact保留tab令牌不提前四倍扩张，CRLF/loneCR归一、其它control变`?`。因此合法范围末尾的内容不会被前面的tab扩张挤出，但输入超过256KiB的最后内容仍不被接受；“tail”是接受前缀的视觉后缀，绝不是原始无限流最后256KiB。

Markdown路径解析完整接受范围以保持fence/inline上下文；total是每块block_visual_lines总和，加块间空行，至少1。逆序遍历blocks，把每个block的有限尾部反向push_front；达到limit停止，必要时添加块间空行。Plain路径直接wrap_tail_segments并同时拿total。空输出补一行；omitted_visual_lines=total−保留行数；最后为保留第一行插角色prefix、其余插等宽空格。角色标记只标记本次保留窗口起点，不意味着该行在原文是第一行；内部list/code marker另保原视觉行位置。

| 尾部helper | 完整行为 | 必须保留的边界 |
|---|---|---|
| sanitize_compact L385–400 | CRLF跳CR，loneCR变newline，newline/tab保留，其余control→?，普通Unicode原样 | 不提前扩tab；输入byte已在调用方限制；它不归一化所有Unicode格式符或双向文本 |
| retain_tail L402–407 | VecDeque满limit时pop_front，再push_back | 私有调用依赖limit≥1；不是无限日志或溢出通知通道 |
| wrap_tail_segments L413–434 | 调共享walker，文本事件合并相邻同style Span，None完成一行，经retain_tail只留后limit；返回deque和total | 至多保留limit完成行及一条构造中行，但joined文本、style段、blocks等仍另占空间；不是总堆上限 |
| walk_wrapped_segments L438–485 | 先join各StyledSegment文本，按扩展grapheme索引遍历；每grapheme取首byte所属style，处理跨style VS/组合；tab固定四个单空格，newline完成行；过宽簇或行首零宽簇→`?`，非零簇溢出换行；最后完成一行并返回total+1 | 不是基于当前列的tab stop；整簇不能fit时有意降级，丢失原字形。计数调用忽略emit，但仍扫描/join输入，不是O(保留行数)算法 |
| block_visual_lines L487–532 | Paragraph/Heading用inline+walker计数；Quote/List先按实际内部marker裁宽再扣body宽；Rule恒1；Code可选language行+每个split newline代码行计数 | 与保留路径共用walker，避免计数/渲染不同；不为被丢弃Rule展开横线，但inline段/代码行临时String仍会建立 |
| tail_inline L534–567 | 包装Quote/List，取得total后以total−lines.len判断原首行是否存活；仅保留行装饰marker或续行空格，原首行marker加BOLD | 被裁掉原首行后不重新插有序编号；角色prefix仍由外层加到窗口第一行；巨大marker只装饰保留行 |
| render_block_tail L569–655 | Paragraph/Heading按相应style尾wrap；Quote/List调用tail_inline；Rule只为保留块分配width横线；Code物理行逆序、行内正序wrap，保原`\| `首行和空格续行，够limit立即return；若还留空间才加language标签 | 未闭fence由parser的Code提供；语言标签/旧物理行可合法不在保留窗口，不能重新显示成完整代码块 |
| tail_prefix_to_width L659–676 | 按extended grapheme累加cell，行首零宽簇变?，下一簇会超宽就停止 | prefix不能fit的宽簇直接停止而不是像正文替换?；保UTF8及整簇不等于字体/终端所有宽度实现一致 |

RenderedTail的行索引是当次布局中的`omitted_visual_lines + retained_index`。纯追加不改变前文解析/宽度时，存活视觉行的索引稳定；T22和TUI流式用例核这种条件。更改width、修改早先Markdown造成重解析、改变角色prefix宽或折叠投影会改变布局，调用方必须作缓存/锚点决策。metadata不记录原始byte范围、不携message ID，不持久化锚点、不返回输入byte是否截断，也不自动插“earlier omitted”文案。

## 资源、终端文本与错误合同

bounded_line_limit L1190–1194返回max(requested,1)与max(4Mi/max(width,1),1)的较小值；不无条件再min8192。truncate_bytes L1196–1205只回退到UTF8字符边界，可在扩展grapheme中间截断；它不添加省略号或标记。sanitize_text L1212–1214固定传256KiB；sanitize_text_with_limit L1216–1245逐字符预检查UTF8替换后的byte长度，CRLF保一个newline、loneCR也newline、tab四空格、其它is_control→?，达到上限停止。所以头部parse可能在tab扩张后截掉原先接受范围末尾；尾部compact路径有意避免这个差别。

| 保证层 | 当前结论 | 不能当作已证明 |
|---|---|---|
| body输入 | 进入公开解析/渲染前接受前256KiB，UTF8有效；head扩张后也限256KiB，tail延迟tab | 无完整输入保留、尾字节接受或原文hash认证；byte截断没有结构化flag |
| 保留结果 | 默认8192及width/cells进一步限制；tail按完整角色行宽计预算 | 4Mi不是峰值堆内存/CPU/整个会话预算，Span/Vec/多轮计数有额外开销 |
| head角色prefix | 仅scalar列裁剪，未sanitize/byte cap，最终缩进不计body预算 | 不能把tail的prefix安全和总cell上界套用到head；也不能把任意公共API参数等同已读生产固定prefix |
| tail Unicode | 共有walker按扩展grapheme、跨样式首byte取样式；过宽/行首零宽有意? | 不承诺所有字体、双向格式、Unicode新版本都显示等价；不是原字形无损 |
| 终端control | body和tail prefix使is_control字符不原样进入相应Span；newline为结构、tab变空格 | 不实现全Unicode安全可视化，不处理终端协议协商或恶意链接目标认证；head外部prefix例外已明示 |
| 取消/恢复/错误 | 无取消token、IO恢复或Result，纯计算返回块/行 | 不知道任务是否被取消；长处理没有deadline或主动yield；渲染完成不能当provider完成 |
| 文档/代码 | 未支持的Markdown趋字面，code显示固定样式 | 不执行代码、不加载图片/HTML，不打开链接；本模块没有剪贴板副作用 |

旧报告的巨大prefix内存放大是公开头部API参数公式推论，当前仍可按代码区分，未重新制造大分配或宣称实测卡顿。尾部已修正按完整width限保留行，T18有大width/长prefix的断言源码；其通过与峰值运行结果仍需各自运行记录。T20扫描计数也不替代整条UI延迟测量，本次不运行性能/预算实验。

## TUI、ViewModel及复制的实际调用链

当前TUI L14350–14441的transcript_window从最新消息倒序构建窗口，自己的max_lines按MAX_RENDER_LINES与4Mi/width算。role prefix为固定`● `或Reasoning的`◇ Reasoning: `；Assistant和System走markdown tail metadata，User/Tool/Error/Reasoning走plain tail metadata。message.blocks若为空或已有live block_id不重复投影，否则view_block_text组合到正文后；不能简单说只有provider assistant会被Markdown解析，System也走该分支。

单消息返回后，宿主取剩余窗口能装下的后缀，并将(position message ID, omitted_visual_lines+line_index)附于TranscriptRow。若单消息早行、跨消息旧前缀或剩余不足导致省略，宿主替换窗口首行显示“Earlier rows omitted from this view”，该提示position=None；提示会占一个保留行。它不修改canonical消息队列，也不代表能够在同一有限窗口滚回被renderer丢弃的全部内容。

TUI L6990–7108按inner.width重建cache，保留row position查找阅读锚点，找不到而已落在窗口前方时回首行，否则按scroll anchor回退；End/follow_latest清锚并回最新。若画Goal区使用transcript_lines转换后再truncate且独立pane scroll，这也是TUI职责。L7113–7148的transcript_messages在渲染视图内：Reasoning折叠成byte数提示，Tool折叠计数并加System摘要，其他消息clone；message_offset+index提供ID。L3782–3801和4420–4433修改折叠/跟随状态并清cache或锚点。render.rs没有任何折叠、按键或项目字段，不能把这些UI支持写为096本身已有状态机。

copy_latest_answer L4438–4444从canonical当前消息倒序选最后Assistant text，返回clone，独立于可见tail与Reasoning折叠；不是从屏幕Span拼接，也不自动包含单独blocks。TranscriptBrowser L886–940最多看最新64消息、预算MAX_TUI_PROVIDER_BYTES、最终entries≤128；原text entry与至多32个canonical block投影入表，Assistant另从parse_markdown提取Code。Code entry已历经renderer的输入截断/控制归一/parse；它与原文entry不一定字节相同。L942–948 Enter只是返回Copy(entry.text)，L974–1027浏览显示用空prefix的plain head并按返回lines限scroll，因此显示窗口限额之外内容不因翻页自动恢复；复制text仍是entry自身并另受剪贴板限额。

TUI L14049–14065在明确Copy请求后检查≤64KiB、将原UTF8 byte编码Base64，外包OSC52控制序列写stdout并flush，错误返回String；L12666–12675宿主显示sent bytes或failure。Base64 payload不会把源ESC原样当终端命令；OS/终端是否接受剪贴板没有ACK保证。“Clipboard sent”只表示写入路径成功，render.rs并未写剪贴板。测试能解码回原控制文本并不代表显示文本、Code entry和原始消息完全相同。

审批模态TUI L1297–1331先redact arguments并按preview.truncated加owner提示，再用空prefix plain head显示完整拼接preview；renderer自己的byte/行截断没有metadata传回该入口。owner的truncated标志和渲染截断是不同层，不能从聊天tail的省略提示推导审批preview截断总会可见。该边界归124/125与095输入合同，本次没有改变审批或复跑审批工具。

ViewModel L392–424把MarkdownBlock映射为canonical变体再validate；有序ListItem转为ordered+单item列表，marker数字字符串不带入该canonical列表。from_markdown_text先parse建立全部blocks，之后才判断MAX_VIEW_BLOCKS，超量返回TooLarge；这限制返回模型，不能抹去parse阶段的工作。不能把parse_markdown本身说成携TooLarge错误或限制block数。

## 308/309 对照：借用原则，不移植未实现能力

| 已接受参考 | 源事实与本文件映射 | 有意差异/判据 |
|---|---|---|
| 308 key_hint | 14文本fn、cfg选13，0源测试；KeyBinding精确code/mods匹配Press或Repeat；格式化仅返回DIM Span，平台Alt文字有cfg差别 | 与096一样“生成Span”不是执行。096不识别按键、不注册Copy，不因有提示文字就说明任何焦点状态下动作可用。owner122/125须核实际键路由与提示 |
| 308 样式/宽度 | key_hint formatter不做cell裁剪/换行，部分modifier不显示但匹配仍精确 | 096的Unicode换行保证不能反向赋给源helper；TUI footer/modal另有宽度与优先级，当前e451未保存警告先于隐藏审批提示，局部125已核，不由096绘制 |
| 308 平台/AltGr | Windows helper只是Ctrl+Alt启发式，实际textarea有更早快捷键分支；cfg(test)的⌥不证明非mac生产显示 | 096Unicode输出处理不证明键盘输入/AltGr/PTY协议。该维度无对应源实现，范围排除有明确owner |
| 309 长详情 | details先全量word-wrap再truncate，行上限不是分配上限；末span字符截省略并非grapheme整行约束 | 096头部同样区分临时工作与保留结果；tail已只保留有限完成行但仍需解析/计数。不要因两者有max_lines就称资源合同等价 |
| 309 截断/可见性 | 主行另走列宽截断；details配置溢出加…、area高度不足不一定加提示；header长可遮interrupt hint | 096metadata只给视觉偏移，提示是宿主；没有通用“…完整内容可恢复”承诺。应核真正输出路径、0/1列、长前缀、隐藏内容和可读取来源 |
| 309 状态/暂停/取消 | widget单bool暂停、32ms请求帧与timer分离；interrupt仅发消息、无ACK和任务身份；setter不自动重绘 | 096无计时/调度/取消。当前125用实际审批集合和job终态控制计时，是TUI新增合同，不能说render.rs自己知道等待或借源pause保证停止 |
| 309 测试边界 | 7测试、18assert、3快照为上次源范围；dummy帧、受控Instant不是实际终端/性能 | 本次只复用同identity既有结论并新读报告；095/125及旧reference运行不并入096，当前23个源测试没有执行 |

两份reference完整包和读取账本在history下保存。旧参考报告有关较旧TUI的Super/Ctrl-G或按状态文案计时结论不能未经检查套到e451：输入修复另项122已迁入，计时/footer修复由当前125局部记录说明。本次不重审整个输入/布局/时钟体系。

## 全部23个源测试：实际断言与尚缺证据

两个cfg(test) helper：L294–302的render_tail_prefixed只传markdown bool并丢metadata；L1266–1271的line_text拼接Span内容。二者不计生产API或测试。T01–T11对应旧子集测试，T12–T23是当前尾部/扫描/metadata/Unicode新增覆盖。下表assert为源码宏位置，循环不会膨胀为不同测试；全部executed=false。

| 测试 | 行区间 / assert | 输入与真正证明范围（静态读取） |
|---|---|---|
| T01 parses_blocks_without_swallowing_unknown_text | 1274–1294 /8 | 八块顺序/变体，code language，未知语法Paragraph；不证明CommonMark全语法 |
| T02 renders_inline_styles_and_hides_link_target | 1297–1311 /4 | bold、italic样式，label存在且target隐藏；输入含code但无专门code样式断言 |
| T03 output_is_bounded_for_narrow_unicode_viewports | 1314–1322 /2 | 世界在1列非空且拼接列宽≤1；不证明原字形保真 |
| T04 unclosed_fence_is_visible_as_code | 1325–1330 /1 | 未闭fence的Code language/text精确 |
| T05 line_limit_is_hard | 1333–1336 /1 | max_lines=2返回2；不测0或显式>8192 |
| T06 terminal_control_sequences_are_rendered_as_text | 1339–1344 /2 | body ESC不存在，`?[2J`与四空格tab可见；没放控制prefix |
| T07 crlf_input_does_not_leak_carriage_returns | 1347–1351 /2 | CRLF空行分两Paragraph；不是所有C0/C1组合矩阵 |
| T08 prefixed_rendering_keeps_role_and_body_inside_width | 1354–1363 /3 | ASCII短you前缀宽8及续行缩进；不覆盖长prefix预算或grapheme |
| T09 plain_prefixed_rendering_does_not_interpret_markers | 1366–1371 /2 | #/---字面存在 |
| T10 sanitization_bound_applies_after_tab_expansion | 1374–1377 /1 | sanitize大量tab结果≤256KiB；不证明末尾仍保留 |
| T11 cell_budget_limits_wide_viewport_allocations | 1380–1384 /1 | 无prefix宽65535规则，断言返回行数≤cellbudget/width+1；不是峰值分配证明 |
| T12 tail_retains_latest_wrapped_rows_while_head_stays_at_start | 1387–1405 /6 | 接受范围最后LATEST在1列tail可见，尾len8192；markdown/plain head仍全x；区分head/tail合同 |
| T13 tail_matches_uncapped_head_styles_and_block_spacing | 1408–1433 /2 | 七组小输入×1/3/12/80列，markdown/plain尾与头精确Line比较；小输入无省略，非所有Unicode/超限等价 |
| T14 tail_preserves_unclosed_fence_and_inline_context | 1436–1465 /6 | 万行未闭rust fence尾部literal浅黄、旧language标签不在；跨20000字粗体保持LATEST和BOLD |
| T15 tail_distinguishes_closed_fence_quote_list_and_plain_markers | 1468–1486 /4 | 关闭~fence后heading/quote/list出现正确样式/文本；plain #/---/**保字面 |
| T16 tail_tabs_do_not_consume_the_final_legal_input_bytes | 1489–1510 /3 | 接受范围内大量tab后的END保留、len≤8192；超256KiB的OUTSIDE仍不可见 |
| T17 tail_unicode_prefix_controls_and_cells_are_bounded | 1513–1536 /4 | width0/1/2/8/80/usizeMAX×两mode，中文/心/家庭ZWJ/组合、CRLF/ESC/tab/DEL，prefix含心与control；两种宽度断言、无control，1列心变? |
| T18 tail_wide_rules_and_role_indentation_share_the_cell_budget | 1539–1566 /4 | 60000规则、空/近满前缀与最大width；返回行数、总cell、末LATEST、Span内容byte公式；无allocator峰值或CPU计时 |
| T19 tail_continuation_does_not_reinsert_ordered_marker | 1569–1583 /4 | 长有序列表截尾，len8192，首行角色+续行缩进、不再出现1.、END仍可见 |
| T20 unfinished_link_search_is_linear_and_valid_links_remain_styled | 1586–1618 /6 | 五种未完整链接closer扫描≤2n、parse无成功、tail非空；有效ok下划线、bad/unfinished字面，长词内underscore原样 |
| T21 tail_metadata_matches_full_small_render_and_large_exact_row_counts | 1621–1654 /5 | 10000行加Code/Quote，与显式30000行head的后缀精确比；60000规则总视觉行120001；plain abc newline宽2保ab/c/空行且omitted0 |
| T22 tail_metadata_keeps_surviving_streamed_rows_at_original_indices | 1657–1672 /3 | 10000行plain后追加new newline，omitted+1，存活行按原索引相等；不覆盖width或早期语法变化 |
| T23 tail_grapheme_safety_crosses_style_and_role_boundaries | 1675–1705 /3 | 心+styledVS、keycap、组合/孤立VS ×多前缀×1/2/3/16列，拼接width及control检查；1列跨style心返回?；非完整Unicode字形矩阵 |

额外context `tests/render_markdown.rs` 71行/4测试完整读：literal underscore/emphasis、块顺序code、1列、显式行limit保首行；`tests/tui_markdown.rs`60行/4测试完整读：小Markdown TestBackend显示、1×1无panic（无文本断言）、User/Tool字面、set_input控制清理（是输入层，不是prefix渲染）。它们均不计23正式源测试，也未执行。

`tests/tui_transcript_ux.rs`只读L1–91、118–264、375–487：共10个完整上下文测试定义及3 helper，核Reasoning独立和copy、三消息18000行后latest/队列不删、滚动窗口锚、队列淘汰与项目切换、窄resize省略提示、Code entry复制与草稿、Base64控制和64KiB拒绝、单消息plain/markdown/code的9000行尾、同stream越8192门槛锚、单物理行Unicode窄宽reflow。构造ProviderEvent、直接KeyEvent和TestBackend不能当真实provider/PTY/OS剪贴板ACK；本次运行仍0。未读其余状态/项目/审批测试，不混计完整测试文件。

## 功能结论、owner与可运行判据

下表每项连接正式源码或明确上下文、现有行为与仍需独立运行的判据。所有判据均为审查建议，不在本次新增产品/探针范围。

| 映射 | 源范围与owner | 结论及最小判据 |
|---|---|---|
| M01 模型与解析状态 | L30–138/755–762；096/ViewModel | 测fence开关字符/长度/language、EOF、空输入/连续blank/未知语法，核完整块顺序；不把canonical block上限归给parser |
| M02 头部API与scalar | L139–240/1109–1174/1247–1260；096/124/125 | 头部保前缀、短角色固定；用小受控参数验证prefix control/零宽/grapheme与完整行预算，避免制造巨量分配；保留头部调用方需求 |
| M03 尾部与偏移 | L241–384/402–412；096/125 | T12/16/21/22，合法byte末尾与超限尾区别、omitted+index、empty/末newline；宽度/早期Markdown变化须使锚重新定位 |
| M04 控制与归一化 | L385–401/1207–1246；096/security/TUI | body/head/tail prefix分别核CRLF、loneCR、tab、ESC/DEL/C1、UTF8边界；is_control净化不等于所有Unicode混淆过滤 |
| M05 共享grapheme walker | L413–486/659–678；096/125 | 宽1/2、跨styleVS/组合/ZWJ、孤立零宽簇、role/body边界；核计数与实际完整行宽，明确?降级 |
| M06 块尾部样式与计数 | L487–658；096/125 | T14/15/18/19/21，长code物理行/内部marker/language不重插、list续行无编号、规则只展开保留部分 |
| M07 头部块绘制 | L679–754/867–967/1175–1189；096 | 每变体style、block间单空行、max0/1/显式大于8192；返回limit不冒充parse/wrap临时工作限制 |
| M08 语法token | L763–860；096 | 1–6#、七#、无whitespace、嵌套>、长数字marker、有空格rule优先级、三/四fence和language；明确有限子集而非CommonMark |
| M09 行内/链接扫描 | L861–866/968–1108；096/125 | T02/20：未知字面、词内underscore、空content、空label/target、缺closer、有效链接target隐藏；计数只证明缓存扫描不证明总耗时 |
| M10 输入/行/cell预算 | L1–29/1190–1206；096/125/131 | UTF8 bytecap与grapheme分割不同；最终tail prefix计cell，显式requested非恒8192；峰值/时延应另限量测，当前无131/预算执行 |
| M11 基础测试区域 | L1261–1386；096 | 2helper之一+11测试27assert文本位置；区分输出存在与保真、无prefix宽测与长prefix、set_input与render边界 |
| M12 尾部测试区域 | L293–303/1387–1706；096/125 | 另一helper+12测试50assert位置，现有输入矩阵与缺项明确；不按循环倍增，不用TestBackend当PTY |
| M13 长输出TUI | TUI6990–7108/14350–14441；125 | role选择、独立全窗口budget、omitted提示占行、锚与message ID；多消息与单消息、追加/resize/queue eviction分别验证 |
| M14 折叠/复制 | TUI3782–3801/4420–4444/7113–7148/886–1027/14049–14065；125 | canonical消息不随折叠删除，copy原message/code entry不同、64KiB显式拒绝、OSC52只有用户Copy后写；OS接收需另证 |
| M15 308提示对照 | 独立308同identity；122/125 | Span提示无按键动作，modifier显示不同于匹配，平台字形/输入不由096负责；实际focus可用性另验 |
| M16 309详情/状态对照 | 独立309同identity及125局部记录；125 | 详情裁剪不等于资源上限，pause不等于provider停，源scalar省略不能冒充096grapheme；原失败与局部195/14结果保边界 |
| M17 ViewModel与审批preview | ViewModel392–424/TUI1297–1331；124/125/096 | canonical TooLarge在parse后，ordered marker投影丢数字；owner preview.truncated不是renderer截断flag，需要有界提示/可检查原文合同 |
| M18 历史与差异 | 旧901行报告/308/309包；096/master | 旧link性能定位已被缓存扫描替换；头部prefix/临时分配仍与尾部区分；完整基线diff与新全文证据不能改登记或自动验收 |

没有遗漏的“UI功能分支”：本文件实际不存在按键、折叠、复制、滚动、message/project身份、timer、terminal mode或session恢复。它们在上述调用方中有确定owner，不能为了映射而虚构096已有实现。42个生产函数、全部类型/常量/注释/测试cfg都有连续unit归属，后附函数索引辅助核对。

## 保全、验证与交付约束

旧096 ready117文件、postfreeze2文件、308包149文件、309包216文件共484个普通文件完整复制并保持字节。旧最终19238 B/152L报告全文新读，旧working15997 B精确前缀复用有记录。旧gate-disposition保留G-STAGE exit1、structural_ok=true、master receipt缺失；历史离线exit0也保留，均不重新运行、不改成当前验收。旧报告所有源码行号属于901行基线，本文以当前1706行另立完整身份和diff。

本次准备阶段两个locator失败只有路径不存在：2267下无096 glob；先前309的capture实际在evidence子目录、直接capture.json不存在。已记录无阅读信用，之后使用真实目录/manifest定位。最初source预览仅作定位，正式完整阅读从新捕获的七段ledger开始。没有运行任何旧script、Cargo、产品测试、PTY、HTTP、release、117/131/FIFO/DTrace或预算任务。

新的便携审计在冻结前完整静态阅读及AST检查，避免manifest变量被匹配结果覆盖和identity.lines数量覆盖语义lines区间；冻结后最多一次，仅核包、来源/读取/单元/函数/测试/历史、唯一报告补丁与临时目录正反向应用。raw stdout/stderr/exit/time/hash置包外；任何失败原样保留，不重跑。现有ready与tracked在冻结时重新比对，主库owned报告仍absent才使用创建补丁。最终G-FILE仍需master阅读本文语义，而不是将结构check数量当测试通过数。

回退仅删除本次创建的owned报告；如果主库届时已有另版报告，需要重新核基底，不能强行覆盖。包中的历史、既有工作树report、产品源码、权威、claims和receipt不在补丁范围。

## 完整函数声明索引

下列67个fn按当前源码位置列出，同名方法依行号区分；42生产、2测试helper、23源测试。索引包括每个函数体hash绑定的unit与语义映射，具体输入、状态、边界和测试判据见正文，不以符号清单代替学习。

| 函数 | 行区间 | 分类 | unit / map |
|---|---|---|---|
| `parse_markdown` | 57–61 | production | U03 / M01 |
| `parse_markdown_bounded` | 63–132 | production | U04 / M01 |
| `render_markdown` | 139–141 | production | U05 / M02 |
| `render_markdown_prefixed` | 150–184 | production | U06 / M02 |
| `render_plain_prefixed` | 192–231 | production | U07 / M02 |
| `render_markdown_tail_prefixed` | 241–248 | production | U08 / M03 |
| `render_plain_tail_prefixed` | 252–259 | production | U08 / M03 |
| `render_markdown_tail_prefixed_with_metadata` | 273–280 | production | U09 / M03 |
| `render_plain_tail_prefixed_with_metadata` | 284–291 | production | U09 / M03 |
| `render_tail_prefixed` | 294–302 | test-helper | U10 / M12 |
| `render_tail_metadata` | 304–381 | production | U11 / M03 |
| `sanitize_compact` | 385–400 | production | U12 / M04 |
| `retain_tail` | 402–407 | production | U13 / M03 |
| `wrap_tail_segments` | 413–434 | production | U14 / M05 |
| `walk_wrapped_segments` | 438–485 | production | U15 / M05 |
| `block_visual_lines` | 487–532 | production | U16 / M06 |
| `tail_inline` | 534–567 | production | U17 / M06 |
| `render_block_tail` | 569–655 | production | U18 / M06 |
| `tail_prefix_to_width` | 659–676 | production | U19 / M05 |
| `render_markdown_with_limit` | 679–753 | production | U20 / M07 |
| `flush_paragraph` | 755–761 | production | U21 / M01 |
| `heading_info` | 763–777 | production | U22 / M08 |
| `quote_info` | 779–783 | production | U22 / M08 |
| `list_info` | 785–816 | production | U23 / M08 |
| `is_rule` | 818–827 | production | U23 / M08 |
| `starts_with_whitespace` | 829–831 | production | U23 / M08 |
| `fence_info` | 833–859 | production | U24 / M08 |
| `render_inline_text` | 867–882 | production | U26 / M07 |
| `render_prefixed_inline` | 884–918 | production | U27 / M07 |
| `render_code` | 920–966 | production | U28 / M07 |
| `inline_segments` | 968–1052 | production | U29 / M09 |
| `parse_delimited` | 1054–1066 | production | U30 / M09 |
| `closer` | 1080–1090 | production | U31 / M09 |
| `parse` | 1092–1106 | production | U31 / M09 |
| `wrap_segments` | 1109–1147 | production | U32 / M02 |
| `wrap_plain` | 1149–1173 | production | U33 / M02 |
| `heading_style` | 1175–1182 | production | U34 / M07 |
| `push_line` | 1184–1188 | production | U34 / M07 |
| `bounded_line_limit` | 1190–1194 | production | U35 / M10 |
| `truncate_bytes` | 1196–1205 | production | U35 / M10 |
| `sanitize_text` | 1212–1214 | production | U36 / M04 |
| `sanitize_text_with_limit` | 1216–1245 | production | U36 / M04 |
| `truncate_to_width` | 1247–1259 | production | U37 / M02 |
| `line_text` | 1266–1271 | test-helper | U38 / M11 |
| `parses_blocks_without_swallowing_unknown_text` | 1274–1294 | source-test | U39 / M11 |
| `renders_inline_styles_and_hides_link_target` | 1297–1311 | source-test | U40 / M11 |
| `output_is_bounded_for_narrow_unicode_viewports` | 1314–1322 | source-test | U41 / M11 |
| `unclosed_fence_is_visible_as_code` | 1325–1330 | source-test | U42 / M11 |
| `line_limit_is_hard` | 1333–1336 | source-test | U43 / M11 |
| `terminal_control_sequences_are_rendered_as_text` | 1339–1344 | source-test | U44 / M11 |
| `crlf_input_does_not_leak_carriage_returns` | 1347–1351 | source-test | U45 / M11 |
| `prefixed_rendering_keeps_role_and_body_inside_width` | 1354–1363 | source-test | U46 / M11 |
| `plain_prefixed_rendering_does_not_interpret_markers` | 1366–1371 | source-test | U47 / M11 |
| `sanitization_bound_applies_after_tab_expansion` | 1374–1377 | source-test | U48 / M11 |
| `cell_budget_limits_wide_viewport_allocations` | 1380–1384 | source-test | U49 / M11 |
| `tail_retains_latest_wrapped_rows_while_head_stays_at_start` | 1387–1405 | source-test | U50 / M12 |
| `tail_matches_uncapped_head_styles_and_block_spacing` | 1408–1433 | source-test | U51 / M12 |
| `tail_preserves_unclosed_fence_and_inline_context` | 1436–1465 | source-test | U52 / M12 |
| `tail_distinguishes_closed_fence_quote_list_and_plain_markers` | 1468–1486 | source-test | U53 / M12 |
| `tail_tabs_do_not_consume_the_final_legal_input_bytes` | 1489–1510 | source-test | U54 / M12 |
| `tail_unicode_prefix_controls_and_cells_are_bounded` | 1513–1536 | source-test | U55 / M12 |
| `tail_wide_rules_and_role_indentation_share_the_cell_budget` | 1539–1566 | source-test | U56 / M12 |
| `tail_continuation_does_not_reinsert_ordered_marker` | 1569–1583 | source-test | U57 / M12 |
| `unfinished_link_search_is_linear_and_valid_links_remain_styled` | 1586–1618 | source-test | U58 / M12 |
| `tail_metadata_matches_full_small_render_and_large_exact_row_counts` | 1621–1654 | source-test | U59 / M12 |
| `tail_metadata_keeps_surviving_streamed_rows_at_original_indices` | 1657–1672 | source-test | U60 / M12 |
| `tail_grapheme_safety_crosses_style_and_role_boundaries` | 1675–1705 | source-test | U61 / M12 |


# ZS1-096 主控独立全文理解验收 · 3.1.21

接受 src/render.rs 这一文件的完整现状理解，包含主控随后合入的预览字素换行修复。只接受096文件理解项；124/125完整交互、085整个TUI、117预算和最终阶段交付仍未完成。登记前53/121，requirement保持3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d。

## 独立阅读与当前差异

主控上轮连续读完修复前render60098B/1706L/b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d：1–300 bee85e、301–600 f71bb8、601–900 6503af、901–1200 6e7a9c、1201–1450 7ec500、1451–1706 896348，包括全部23内联测试、2测试helper、42生产函数和EOF。当前59562B/1688L/9e526c42e1394cc2aa888f9627e7e0dfaf5b5e3acd0db3187c65553884d61c0a与该全文只有wrap_segments函数的−536B/18L变化，完整patch已读bccab5，本轮又完整读当前函数7009bd；其余字节通过精确连续匹配对应已读正文。因此不是把旧60098B报告直接套在当前源上，也没有重新宣称本轮从1行开始连续读1688行。

本轮另行完整连续阅读冻结基线30217B/901L/6eaaa712745659ae48f429217c5eb8961dd12e71789f8a2bc70867ee71ba25a7：1–310 ca1fd4、311–620 f7a156、621–901 32e84a，含基线11测试及旧parse_link/underscore反向扫描。当前对基线的变化涵盖头尾接口分离、延迟tab展开、metadata、完整grapheme walker、链接closer缓存及本次头部Span换行复用；基线的source_manifest身份继续冻结，不以现状hash覆盖。

worker280行42574B报告248b6883f7e3910718c1be101151f4c660c738c8f51cf8b66ce4eed29df81d47完整阅读：1–100上轮857a68，101–200本轮61e217，201–280本轮85c552。tests/render_markdown.rs71行4测试、tests/tui_markdown.rs60行4测试本轮从各自1行到EOF读完d53ee6；不把8个外部声明并入23个内联测试，也没有本轮执行它们。

## 模块全部行为判断

六类MarkdownBlock只表示有限终端Markdown子集。解析按code状态优先处理fence关闭，同字符/足够长度/无语言才关闭，EOF未闭仍输出Code；否则按fence、blank、heading、quote、list、rule、paragraph顺序。空白归并、单层quote/list、rule的字节长度与list优先级、language首token和未支持结构均如报告所述，不承诺CommonMark保真。文本解析先接受前256KiB，UTF8字符边界不等于grapheme边界；没有返回input-truncated标志或block数量上限。

头部接口保留首行，width钳1–65535；requested至少1且受4Mi/width限制，显式requested可以超过8192。wrap/inline/parse可能先构造全部中间结果才按行limit保存，返回量不等于峰值内存。公开head role prefix不清控制字符或限bytes，使用body宽求预算又附加缩进，不能套用tail完整width预算结论。实际两个TUI head调用者传空prefix，不能由公开任意prefix推断现实终端注入已经发生。head代码块wrap_plain及truncate_to_width仍按scalar；本次wrap_segments改为grapheme并不覆盖这两个路径。

尾部路径接受的仍是原始前256KiB，在整个接受范围内保持fence和样式语境后选视觉后缀。sanitize_compact不先扩大tab，避免其挤掉合法范围末尾；CRLF/loneCR归一，其他is_control替换?。prefix先限制/清理再grapheme裁宽，body至少一格，行数按最终完整width限cells。它不保存无限流末尾字节，不作全Unicode双向格式安全归一，不自动添加字节截断标记。

walk_wrapped_segments先join文本，扩展字素跨样式边界时取首字节的Style；换行完成一行，tab固定四空格，过宽或行首零宽簇降级为?，最后补一行。计数和保留共用同一状态机，避免行偏移算法漂移；计数虽不创建Line仍需join/扫描。tail保留deque至limit并记录总视觉行，Markdown逆序挑块、code逆序物理行但正序wrap，内部list/code标记只属于原首行。omitted_visual_lines是当前接受内容在当前width布局中的行起点；resize/早期语法变化后不能当持久byte锚点。尾部渲染本身没有message/project/job身份。

行内格式只做单层delimiter、词内underscore保护和链接label。空content失败后保留字面，label可以空，target非空但不校验URL可信性，不打开链接。当前previous_non_underscore避免重复回看；LinkScan两个closer位置含EOF缓存在同一文本/index单调推进前提下有效。T20扫描byte计数只约束该查找部分，不证明整个renderer响应时延或资源峰值。

本文件是纯内存块/Line/Span计算，无IO、任务取消、审批决策、终端模式或会话恢复；分配失败/panic不被此层捕获。copy动作、折叠、滚动、状态时钟、按键匹配应由各宿主owner承担，不能因为生成styled span就宣称它们执行成功。

## 当前宿主接线与仍待验证的差距

本轮完整读TUI transcript_window及邻近14350–14465（5990c5），render_transcript_pane与transcript_messages6990–7165（343fd8）。当前TUI SHA801bffa879a211fc8b1fe3f43463dd38584f7f570b870d91f0c5ba2258d79c02未变。Assistant与System走markdown tail，User/Tool/Error/Reasoning走plain tail；folding只造显示副本，stable message ID加omitted行偏移定位。宿主max_lines约束整窗口，从最新消息取尾，省略时首行提示无position并占一个显示行。阅读锚点优先匹配position，过早则回首，找不到则回退scroll anchor；渲染cache按宽度失效。Goal分支也有独立截断/scroll状态。这些是有界函数接线阅读，不是085整个16376行重新验收。

上轮已读的正文查看器与审批render（e06e5a/9132f8）仍适用；本次实际修复消除了两个空prefix head视图的emoji后字符裁剪，但head行/byte上限仍会截掉较长正文。工具preview.truncated仅反映owner截断，不等于renderer截断，审批UI当前缺少通用渲染省略反馈；保留为124/125下一项真实入口反例验证条件。本轮没有把该静态发现称为已复现或已修复，也没有补测压力样本。

ViewModel的parse再校验128块不能消除parse中间分配，有序marker到canonical List只保ordered+单item。308/309同hash报告/历史保留，按键hint无动作、暂停bool无job身份、详情先wrap后截断等对照限于已有复核结论，未新读其全源或复跑参考测试。

## 验证、证据保全与接受范围

worker immutable564普通文件、其中484历史文件保留，manifest135680B/4b3fc05994c41164db23839fbf1a8fc7c802db26b801c034d890ec8d281e9d48。主控完整静读verify.py（9dbcce），新副本只改PACKAGE为argv并固定manifest hash；原包全部无写权限/无symlink。仅一次新审计6746b9：233结构检查通过、0失败、stderr空、0产品执行。涵盖包/读取段/61连续单元/67函数/23测试77assert/18映射/历史保全/单路径正反patch与sentinel；它只验证已实现的机械一致性，人工语义结论来自上述完整正文。

上一轮125局部修复已有[单独主控记录](../../ZS1-125/master-preview-grapheme-3.1.21/review.md)：原3失败，修复后85Rust（含render23）通过、fmt/clippy/debug0。它用TestBackend并非真实PTY，新debug63e40f29b01a82fd5b0d9f02069d10d466f68f3b707a370e27a44ba2c109714a没有release/预算通过。本轮只绑定保留该记录和确切修复差分，不重新执行、重复计入或把worker的0运行改成85。

worker捕获的旧权威状态、旧096 G-STAGE缺master receipt失败、准备locator错误与旧308/309包原样保留。当前唯一报告此前不存在，接受时创建worker原文加本主控现状限定；不改历史包、源源码或用户数据。096通过后整阶段仅新增一个文件理解项；BentoBox与顶部加号流程未改，124/125/117等整项继续按真实入口和当前构建证据独立闭合。
