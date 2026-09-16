# ZS1-402 — vendor/crossterm/src/event 独立目录理解候选

本报告仅对应 `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/event/current_folder_learn.md`，understand 候选，产品实现 LOC 为 0。权威为主库蓝图3.1.21与唯一 active selector，requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。开始时400、401均[ ]且master receipt不存在，402也未接受；本轮依授权先研究和制作候选，不宣称 G-DIR 已满足。

蓝图402只 Depends ZS1-401。G-DIR要求冻结直属文件与直接子目录先独立接受，再审阅本目录调用、所有权、错误、取消、持久化及闭包。这里的完整代码读取与库存提供可复核基础，目录理解依靠下述逐连接分析；候选或哈希校验不代替主控接受。

## 直属物理清点与冻结集合

本轮对 `vendor/crossterm/src/event` 直接枚举，包含隐藏项，实际为六个普通文件、两个普通子目录，无符号链接、隐藏额外项或设备项。父层 `vendor/crossterm/src/event.rs` 是同名模块根文件，物理上不在 event/ 内，不能算作第七个直属文件。sys 的孙项没有提升为本目录直属项。

| 直属项 | 字节/行 | 冻结角色与本轮处理 |
|---|---|---|
| filter.rs | 3786 B / 115 行 | context-only；完整精确复用400读链，连接公开事件与Unix内部查询响应 |
| read.rs | 14606 B / 436 行 | context-only；完整精确复用，包含全部FakeSource测试，负责source、过滤与队列 |
| source.rs | 925 B / 27 行 | context-only；完整精确复用，trait及目标平台模块选择 |
| stream.rs | 5092 B / 146 行 | context-only；完整精确复用，可选async等待线程和双waker连接 |
| sys.rs | 248 B / 9 行 | context-only；完整精确复用，平台sys模块及event-stream Waker导出 |
| timeout.rs | 2662 B / 92 行 | context-only；完整精确复用，Instant预算计算及源测试 |
| source/ | 普通目录 | 唯一冻结直接子目录ZS1-401，封包时接受状态单独记录 |
| sys/ | 普通目录 | context-only，无正式目录项；直属unix.rs/windows.rs及unix/、windows/子目录逐项清点，只为接线研究 |

file index的本目录冻结直属文件数为0；folder index直接子项只有401，没有sys目录项。401内部unix/为400，mio文件为099；099已独立接受，不能绕过400/401直接完成402，也不将6个context文件或sys/变成隐含已接受项。闭包仍是099→400→401→402→403(src)→404(crossterm)→405(vendor)→091(root)，本包只新增402报告。

## 完整读取与精确复用

复用完整401 ready，manifest `b29b2f278ee0bfc956b6498f8e04bb5aef6e6d7b20c0449972435a3d0556d99d`，其中原400 ready与099 accepted receipt/read chain保持原字节。401的15份旧完整context和5份Windows新完整context，本轮逐字核对当前源码全相等，共20份；每份复用路径、完整[0,EOF)范围与SHA在 reused-context-read.json。既有读取事实不依赖401是否接受，语义接受则必须有独立凭据；不能混为一谈。

当前mio仍25801 B / 655行 / `41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f`，与099六段连续读链完全一致。099冻结source_hash仍为原始57f4，原229行、1afa477行及178行增量读取证据完整保留，不改写为当前实现hash或再接受一次。

本轮必要的新完整context共五份：父event.rs1777行、sys/unix/parse.rs1506行、vendor/src/lib.rs263行、cursor/sys/unix.rs56行、terminal/sys/unix.rs326行，合计3928行。先保存只读快照，再连续分块逐行读至EOF；包括event类型、Command实现、全部内置测试、完整Unix解析表及测试，而非只读函数签名。read-blocks.json保存每一连续块的字节范围与hash，fresh-context-read.json记录全文件。lib.rs用于确认events总门控与Windows编译约束；两个查询调用方用于确认reset需停哪些读者、raw恢复与超时边界。它们全部仍是context，不新增正式文件或目录验收。

## 模块和接口接线

lib.rs仅在feature events时声明public event模块；没有events则整个event.rs入口不在该构建中。event.rs声明本目录filter/read/source/sys/timeout，stream另受event-stream控制并对外re-export EventStream。source.rs以cfg(unix/windows)选平台实现，sys.rs以同样目标平台选择sys模块，并在event-stream下导出对应Waker。Unix source与Unix waker在use-dev-tty条件上再次成对切换。根Cargo默认接线仍走本地vendor的默认Unix mio，不把Cargo feature名windows视作在macOS执行Windows代码。

| 消费者→提供者 | 数据/状态转移 | 必须维持的约束 |
|---|---|---|
| public poll→poll_internal→InternalEventReader.poll | 公开EventFilter、剩余Duration | 等锁时间扣入本次poll预算；source初始化和其它慢调用不是可抢占操作 |
| public read→read_internal→reader.read | 从匹配队列取InternalEvent::Event，移交拥有的Event | public poll/read文档要求同一线程，不与EventStream混用；锁不使不受支持的交错变成受支持 |
| reader→Box<dyn EventSource> | mut try_read返回至多一项InternalEvent | 只选一个后端；new失败经.ok变None，没有mio→tty或Unix→Windows回退 |
| Unix source Parser→sys/unix/parse | 连续字节前缀和input_available提示→Some/None/Err | Parser拥有累积内存，parse helper无独立流游标；None保留、完成或错误按source规则清理 |
| sys/parse→InternalEvent→Filter | 公开Event与CursorPosition/增强flags/PrimaryDeviceAttributes分路 | 终端协议响应不应直接作为公开键/鼠标事件交付 |
| cursor/terminal查询→poll_internal/read_internal | 主动写查询，等待专用InternalEvent | 查询也使用共享reader，是reset外部停读约束的一部分 |
| EventStream→公共内部poll与source Waker | 后台等待输入、唤醒executor或取消底层等待 | executor waker与OS poll waker用途不同；Drop未join，不等于工作全部结束 |

事件数据模型定义在父event.rs。Event有Focus、Key、Mouse、Resize，bracketed-paste开启才有拥有String的Paste（这时Event不再derive Copy）；as_paste_event借用&str，其它小型as_*按值返回。KeyEvent区分code/modifiers/kind/state，new默认Press，Repeat/Release需看kind；is_key_press不会接受Repeat。KeyEvent的Eq与Hash按ASCII大小写/SHIFT归一化比较，不会在事件进入队列时自动改写字段，也不等于任意Unicode/平台输入都规范一致。Display只是平台化显示文本，不是协议序列化。serde派生可选，不代表目录拥有会话持久化格式。

## 共享reader、队列与过滤的具体所有权

event.rs的static `Mutex<Option<InternalEventReader>>`是进程内共享reader入口，初值None。lock内部用get_or_insert_with懒建reader；映射guard使调用方在整个reader.poll/read期间持锁。reader持events VecDeque、skipped_events Vec和一个可选source。new的具体错误由.ok丢弃，后续poll报通用初始化失败，waker对无source expect panic；不是自动重建重试通道。

有超时的poll_internal先创建PollTimeout，再try_lock_for(timeout)，拿不到锁返回false，拿到锁传leftover给reader；None超时路径则无期限lock。reader自己的PollTimeout继续扣除过滤/读取耗时。因创建source发生于映射guard内，以及source系统调用、size查询等可慢于预算，不能将注释“maximum waiting time”提升为任意环境下的严格整次墙钟上限。read_internal直接持锁读匹配项，可无限等待；public poll与随后read不是同一guard持有的原子事务，其不阻塞承诺建立在文档允许的消费模型上。

reader.poll先查已有events中的匹配项，不必先访问source；再从source消费，不匹配项放skipped_events。匹配或正常超时时把暂存事件恢复，匹配项push_front，以便下一次read取出。reader.read暂存在局部VecDeque的不匹配项，找到目标才push_back恢复。这些队列与mio pending_tokens/Parser.events均是不同owner，不能将多个层次说成一个统一FIFO，更不能认为各种filter交替消费保持原始总序。匹配事件会有意被优先取走。

异常分支也不同：poll收到Interrupted直接false，其它Err直接上抛，都可能早于skipped_events drain；read的局部暂存若随后poll Err，会在函数退出时销毁。正常过滤不丢其它类型，不足以证明所有错误均保留其它类型。reader源码完整测试含队列、匹配/跳过、不存在source、FakeSource超时/错误等，未覆盖真实fd/信号与全部排列，其waker未实现。本轮只读测试，没有执行。

Unix EventFilter仅匹配InternalEvent::Event；CursorPositionFilter只匹配坐标；KeyboardEnhancementFlagsFilter同时接受flags和PrimaryDeviceAttributes，以后者代表未响应增强能力；PrimaryDeviceAttributesFilter单独处理残余DA响应。Windows InternalEvent没有这些Unix变体，EventFilter全true。过滤是按类型选择，不是网络请求ID相关匹配，不能用于证明并发多个终端查询各自收到自己的应答。

## Unix字节解析与公开事件的连接

完整parse.rs的顶层约定是Some完成、None需更多字节、Err解析失败，具体buffer处理由两个Unix Parser承担。parse_event空输入None，单ESC根据input_available决定继续等待或Esc；ESC O处理箭头/Home/End/F1–F4，ESC [交给CSI，双ESC生成Esc，其它ESC前缀递归解析并只对Key加ALT。CR是Enter，LF是否Enter依赖terminal的raw-mode登记；raw模式下LF落入Ctrl-J。Tab/DEL/控制字符/NUL分别映射，普通字符经UTF-8验证并为大写添加SHIFT。

因此input_available是调用端关于当前字节可用性的提示，不是parse自己做IO，更不是消息边界。已接受099中41321e只在TTY正常排空后针对恰好一个ESC调用完成逻辑，不会把UTF-8/CSI/paste半序列任意冲洗成事件。旧1afa socket ESC反例和公共PTY前后均通过的反证保持；本次完整读parse不是新行为实验。

CSI按早期字符和末字节分派：箭头/功能键、BackTab、Focus、X10/RXVT/SGR鼠标、数字~键、CSI-u键、光标R、增强flags/DA响应。数字路径仅在末字节属于64–126时尝试完成；bracketed-paste开启且匹配200~前缀时继续等待201~终止符，将内部字节按from_utf8_lossy生成String。paste中其它转义不被解释为独立命令；没有硬长度上限或完整paste超时，缺结束标记会持续保留内存，invalid UTF-8则有损替换。source预分配容量不等于内存cap。

UTF-8 helper在有效串中只取首字符；invalid时根据首字节和续字节形状判断需更多还是Err。它依赖source逐字节推进并在完成时清当前序列，不是任意整段混合输入的消耗长度API。parse_csi_*含前缀/后缀assert，调用链保证其基本前置条件，但这不等于所有malformed payload安全：坐标/RXVT/SGR解析存在u16减1，X10先saturating_sub再减1，0坐标缺显式拒绝；调试溢出与发布行为不应被概括成统一io Err。本轮未注入这些输入，不修context代码，也不声称穷尽的畸形协议健壮性。

CSI-u读取base codepoint、可选modifier:event-kind并映射functional/keypad/media/modifier表；kind未知值回退Press，部分modifier解析失败也回退默认；可选SHIFT alternate codepoint可覆盖keycode并去SHIFT，剩余associated text未作为文本事件实现。state组合keypad/Caps/Num。普通KeyEvent Eq测试的大小写等价不能反推parse输出字面字段完全相同。增强flags解析实际直接检查buffer[3]的位，未对整个十进制数字串做parse；DA是仅识别响应的stub，不保留属性列表。源码过时“no tests”注释不能作为事实，后面已有大量具体解析测试；相应“全部增强协议已支持”也不成立。

鼠标CB解析低位/高位按钮与拖拽、SHIFT/ALT/CONTROL；不支持按钮为Err，部分release以Left占位。SGR以末尾小写m将Down改Up，坐标统一减1；RXVT/X10编码有限，与真实平台手势能否完整上报是另一件事。parse完整测试覆盖ESC/Alt、UTF-8合法和部分非法、CSI分派、鼠标格式、含转义的partial paste、CSI-u按键/状态/alternate/type等具体断言；不覆盖全终端、全feature、全错误边界，本轮没有把其数量登记为通过。

## 查询、raw mode和reset：跨目录边界

cursor/sys/unix.rs完整读取确认，position在库认为已raw时直接查询，否则先enable_raw_mode，保存查询结果后disable_raw_mode，再返回原结果；disable失败可覆盖查询结果。查询向stdout写ESC[6n并flush，循环poll CursorPositionFilter每次2000ms；poll false才超时报错，poll Err被忽略后重试，poll true后read失败也重试。没有整次全局deadline，不能把每次2000ms写成函数一定两秒内返回；其它reader占锁也能导致超时。

terminal/sys/unix.rs的增强查询在非raw分支同样暂开/恢复raw。向/dev/tty尝试写query，失败回退stdout；先请求flags再请求DA，用增强filter等待。当flags先来，随后调用无超时的read_internal(PrimaryDeviceAttributesFilter)并忽略其Result来清残余DA；若DA迟迟不到，这一步仍可能阻塞，不能凭前面2000ms poll保证整体有界。先收到DA返回None；poll错误也会循环重试。此处清DA是reader事件消费，不是tcflush或清空OS队列。

raw mode是terminal模块另一把Mutex<Option<Termios>>管理的登记：重复enable看到Some便返回，不是引用计数；只有系统设置成功才保存旧模式，disable成功才清登记。is_raw_mode_enabled检查库中Option，不查询外部进程是否改过termios。reset不会改这个Option，raw模式也不会自动清reader。window_size从/dev/tty或stdout取尺寸；size失败后调用tput cols/lines，无本地超时/退出码成功检查，其输出数字折叠还有输入限制。因mio的SIGWINCH接到size，这也是source等待时限之外的慢路径。

定制reset_event_reader在event.rs仅Unix且not event-stream可用。try_lock失败立即WouldBlock；成功后reader.take并drop，使Option变None，下次poll/read再懒建source。它会销毁完整预读事件、skipped_events、Parser部分序列及source内部readiness，且在持reader锁期间执行Drop；不flush OS输入，不disable_raw_mode，不恢复鼠标/焦点/paste模式，不关闭其它线程或保存草稿。没有“有锁就等待到安全”的分支，调用方必须先停所有读者及cursor查询；实际键盘增强查询同样共享reader，应纳入外部停读协调。正常Resize不应该触发reset。

reset内置测试实际持锁调用，断言WouldBlock且小于100ms，释放锁后再reset成功。这证明该测试意图的锁冲突路径，不单独证明PTY外部editor交接、所有Drop耗时或跨线程生命周期；历史vendor默认检查含它，当前只读没有重跑。event-stream配置根本没有该API，不能因源码里有reset就向该组合承诺可用。

父event.rs的Enable/DisableMouse、Focus、BracketedPaste、Push/PopKeyboardEnhancementFlags为Command，实现ANSI写法或Windows分支；创建/销毁source不会自动执行这些命令。Windows legacy paste enable/增强命令可能Unsupported，focus禁用分支是空成功；平台能力边界不能因定义了枚举就视为实际启用。lib.rs明确Windows目标禁用windows feature会compile_error，不能将未events/未windows的任意feature组合说成通过编译。

## 取消、释放与持久化界限

source/的401候选已逐后端分析：Unix默认mio、可选tty、Windows console分别保持不同状态。mio EOF是UnexpectedEof，tty把WouldBlock与EOF折为0，Windows空记录数不是EOF；waker实际使try_read返回Interrupted，再由reader转false。底层trait注释称Ok(None)不能覆盖真实返回分支。Unix fd Borrowed/Owned、Signals与tty注册差异、Windows外部Handle/Semaphore生命周期均沿401的准确范围复用，Windows/use-dev-tty无新增产品验证。

EventStream直接文件的完整读链说明两种wake：后台任务用executor waker唤醒Future调度，Drop用OS waker解阻source.poll。Default创建sync_channel(1)并spawn线程，Task携带两个AtomicBool和executor waker；poll_next先零超时poll，有事件则read，false时compare_exchange保证最多一个已登记等待task。发送失败被忽略，executed不在该失败处分支显式恢复；任务等待中保存最初送入的waker，并非每次Pending都替换。后台poll成功或shutdown检查后清executed再wake executor，退出后需channel关闭使recv结束。

Drop只设置shutdown、尝试wake且忽略错误，没有JoinHandle或join。后台循环忽略poll错误再检查shutdown；source wake失败、初始化失败panic、通知发送失败等都不能由“容量1”推出统一有界结束。poll_next代码没有返回Ready(None)的正常终止分支；source EOF通常可表现为重复Err而非自动fused stream。公共文档不允许将同步read/poll与EventStream混用，Mutex存在不意味着多个EventStream/其它读者混用契约已验证。这些是静态上下文限制，不运行新线程实验，也不将input wake等同于项目runtime shutdown。

本目录无project/session ID，无WAL/JSONL writer、HTTP、close hook、持久化事务、编辑草稿或kill buffer。Event::Paste(String)及parser/cache都是进程内瞬态数据，serde支持只是数据编码能力。正常输入经TUI owner改变布局/草稿/发送，持久化由其它模块处理；source/reader销毁或reset不把未消费输入转换为草稿，也不保证崩溃后重放。回滚本候选只撤回402报告，不覆盖产品和用户会话。

## 证据时点与剩余门禁

完整复制的prior401及其prior400保持原时点：099独立接受，400/401尚待主控。旧944049+41321e的144 Rust和vendor8/9、公开31ea+41321e的6891 release/8预算/项目PTY12/kill-yank29均保持原版本归属；8/9重叠不能累加为17个独立后端证明，旧公共ESC PTY前后都通过的反证不改写。401记录的headless从31ea变为521c也原样保留，不拿6891结果给521c盖章。

本轮授权消息说明主控正在对headless521c执行新的固定首三样本预算及版本选择CLI检查。这里不等待或推测其结果，不运行任何旧runner/build/PTY/预算，不将运行中消息写成成功回执；包内current-related-identities仅记录当前headless/mio及必要上下文身份，源目录未变化时不因无关Gantt文档漂移重封旧包。headless源或新release的完整产品语义不属于402，既有行为证据仅辅助接口理解，不代替本目录审阅。

准备期间主控已独立接受400：新receipt complete=true、manual_review.decision=accepted，9项artifact全部按字节/hash核对；canonical报告24660 B，精确为原worker20939 B加两个换行及3719 B主控review（SHA `81f077cf11b0c115d9210c1d0b7be5eb7107dadcde078ad832df9253dae76862`）。该主控review本轮完整新读，确认15份context均独立审阅、队列异常边界及tty/stream限制保留；接受仅400。主控通知32/121、snapshot `4106f72bc041cb6c3c04470cd1006ff5b93ef403688fe2a752ec9c10b2b8a3ab`，requirement/baseline不变。初始400 receipt缺失及旧401候选历史仍原样保存；401仍[ ]且receipt缺失，不能因间接子项400接受而跳过直接依赖401。

封包途中401也已由主控独立接受，原“401仍未接受”的打包断言退出1，保留未封包attempt1；该次紧接的verify因文件尚未生成退出2，没有运行任何旧验证器。随后完整新读401 master review3949 B / SHA `e8a634d6e73b22f9f506008903a0cc4733e4c42b44cabde6f9a1d7ce32cb9430`，核对receipt及全部10项artifact，canonical27397 B精确为原worker23446 B加两个换行及主控追加。401 master以真实400凭据闭合依赖，独立新读5份Windows context并复用400的15份，明确只接受401。新snapshot `f6051bcd7a92627bc410d7ac9c0197adfc8501d5bbe6c7cbb9a82fc605e82eff`；旧401候选及本轮initial-authority缺失证据不改写。主控401 review另记载521c/92f95的新80 Rust、55 CLI、8预算，但这些只作为该review的版本背景，本包没有将它们算作worker执行或重新审阅的新产品证据。

402保持accepted=false，直接依赖401已接受，等待402自身的独立目录审阅。新离线只读verify仅检查直接库存、冻结映射、完整新读块、精确复用、历史manifest、实际依赖状态及唯一报告patch；不会执行历史验证器、产品、G-STAGE，也不会伪造master receipt。401现已接受也只是402前提之一，仍需本目录独立主控审阅；403–405/root不随本包接受。

所有新输出限于私有.ops，主库/authority/旧ready/构建输入只读。保留worker HEAD与94条non-.ops dirty。封包后停止，不新建任务或subagent，不扩大Windows/sys/terminal等context为产品或目录验收。主控集成时应按当时相关源码、依赖凭据判断是否需要增量，不以旧时点状态替代当前审阅。

## 封包时点与完整读取索引

最后封包核对 2026-09-12T17:35:46.517713+00:00：400和401均有正式接受receipt，402仍[ ]；snapshot `f6051bcd7a92627bc410d7ac9c0197adfc8501d5bbe6c7cbb9a82fc605e82eff`。本包直接依赖已闭合，但402仍需主控独立审阅；初始缺失状态和旧401历史不倒改。当前headless身份为 `521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0`，仅作版本观察，不附新产品成功结论。完整源码/上下文均与所读字节一致，正式402报告尚不存在。

|方式|完整文件|行数|字节半开范围|SHA256|
|---|---|---|---|---|
|新读|vendor/crossterm/src/event.rs|1777|[0,63629)|`989c472a11a6b471e663e191fd64337593423de2aa4774344cb246fb975bbd42`|
|新读|vendor/crossterm/src/event/sys/unix/parse.rs|1506|[0,55081)|`689b765d67be62703f7935abfa1475d98994987461e413836b54c7936966ce2b`|
|新读|vendor/crossterm/src/lib.rs|263|[0,10739)|`ec896da229e4b9b02da3cf444bf1d89e0bc8dc900a29ded2b2a1df086d92b6aa`|
|新读|vendor/crossterm/src/cursor/sys/unix.rs|56|[0,1711)|`e048206557e41c2090437d2b2b30c0485d17db9bb90e726e44d2e8d3ec5d5d63`|
|新读|vendor/crossterm/src/terminal/sys/unix.rs|326|[0,10684)|`db066f11aa75d6c06515fd600ee38336f230d0f8f9d307663debb6ba835b334b`|
|精确复用|vendor/crossterm/src/event/source/unix/tty.rs|278|[0,10162)|`453e854ca82cd2d37e57494d9766a9eea728883a8a1c5fec186f7c6b83dbd64e`|
|精确复用|vendor/crossterm/src/event/source.rs|27|[0,925)|`f606a124266b6d86b58969348bb713f7f89fb09886f51bfb6fe16b766e113bb7`|
|精确复用|vendor/crossterm/src/event/source/unix.rs|11|[0,294)|`253ed074fbe63e7270fc19e5c85186407c3a91454f43291c29cf5717575c74a6`|
|精确复用|vendor/crossterm/src/event/sys.rs|9|[0,248)|`3ec336b2d4c36cfc895fc7cf586e92f7763473130ca99e8168bead1ba3918d1a`|
|精确复用|vendor/crossterm/src/event/sys/unix.rs|5|[0,110)|`d651928295213cd12a4df7edeb0c8be5a4e4487345fc9f2c2ff59d6411047cf2`|
|精确复用|vendor/crossterm/src/event/sys/unix/waker.rs|11|[0,258)|`fe7a1e77718306e9f84b77337f4e9ea9990bed4cf533036228508316363add0e`|
|精确复用|vendor/crossterm/src/event/sys/unix/waker/tty.rs|28|[0,685)|`49132cd71d20b7cc27b79d28a490743da77241bed24a178e22d9ad6e5e98e185`|
|精确复用|vendor/crossterm/src/event/sys/unix/waker/mio.rs|34|[0,1026)|`cbc89c146a004ad689db7f17e3cfc9ce6198c115f31e519d25c68a441db3ecb4`|
|精确复用|vendor/crossterm/src/event/read.rs|436|[0,14606)|`704eb054bff7408e7591e7b5869cf4606794a43b69dc7a1edef67fa7e26ccb8a`|
|精确复用|vendor/crossterm/src/event/timeout.rs|92|[0,2662)|`cbe518ad2cb666b588f593b4b84704eb4480e49d9bd3f28c72d696dae6e7b5e7`|
|精确复用|vendor/crossterm/src/terminal/sys/file_descriptor.rs|154|[0,4249)|`d215ff60394ec2150c2df6340343d84a3922570c1feba813663742d0eb85d13f`|
|精确复用|vendor/crossterm/src/event/filter.rs|115|[0,3786)|`092515c0939799af80768e7b87d2ea4ffc5b1bcda021982f6f02922378809df5`|
|精确复用|vendor/crossterm/src/event/stream.rs|146|[0,5092)|`1df7f35a1842d54b6a8d2b82c96661305d7bd87bbdf123ce6670c62fec5f3cb1`|
|精确复用|Cargo.toml|50|[0,1228)|`d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884`|
|精确复用|vendor/crossterm/Cargo.toml|240|[0,4455)|`78b5e8b5cd6660067195979533e61d4b33c375c74613f24180efd94c86f7d295`|
|精确复用|vendor/crossterm/src/event/source/windows.rs|100|[0,3656)|`e25bdd13432dfda96401a76f3e2d6aa2a404fbaa4f1b99b55bb0bdec2ac75336`|
|精确复用|vendor/crossterm/src/event/sys/windows.rs|48|[0,1707)|`130d7da62a0af8b1cf7c55bd8bf78fa0272a9a44803704dac6416db993d30d81`|
|精确复用|vendor/crossterm/src/event/sys/windows/poll.rs|86|[0,2545)|`232c69f9c9aa058189a418cdaee82134feaf6ef91f89cea8eaaeb5b4cfd722fc`|
|精确复用|vendor/crossterm/src/event/sys/windows/waker.rs|40|[0,1156)|`a145d8ccf6b7747eb4e9499bdade2616a997539da4f4691f4c14bd4c4d6a472b`|
|精确复用|vendor/crossterm/src/event/sys/windows/parse.rs|378|[0,15566)|`15afbd3b84edfa2a72f9587f591ce9f7dfe99f1cb85db8b45037c0b70c0346ae`|


## Controller independent directory acceptance — 3.1.21

The controller read this complete25390-byte candidate and its entire portable verifier. It newly read every line of event.rs1777, Unix parse.rs1506, lib.rs263, cursor/sys/unix.rs56 and terminal/sys/unix.rs326:3928 lines of actual module roots, parser functions, query callers and all included tests. These five context files are not new formal leaf acceptances. The complete400 and401 master reviews were read again. Their previously independently read20 complete context files and accepted099 baseline/current655-line chain are reused by exact current source and receipt-artifact identity, without executing archived programs or claiming fresh rereads of those20 files.

An independently enumerated physical event/ has six regular files and two direct directories, no symlink or additional hidden entry. event.rs is its parent's file. There are zero frozen direct files and only401/source as formal child; sys/ and all six files remain context. Both400 and401 are individually accepted, their exact9/10 artifact chains checked. Their acceptance is a prerequisite, not automatic402 acceptance. Current source matches all25 fresh/reused context identities and41321e mio. The root ran the new copied package's read-only verifier once with external logs:438 payloads verified, including original failures, old state transitions,14 continuous new read blocks and20 reused full contexts. Hash coverage is supporting evidence; the separate semantic reading is the basis of this decision.

The actual module root gates event on events and EventStream on event-stream; source/sys and waker selectors must agree on target/use-dev-tty. The public reader is one process-global Mutex<Option<InternalEventReader>>. Lazy initialization occurs while acquiring the mapped guard. poll subtracts lock-acquisition time, but source construction, size lookup and arbitrary blocking operations are not made preemptible by that duration. read can wait indefinitely, and poll followed by read is not an atomic multi-reader transaction. Public documentation excludes cross-thread synchronous readers or mixing EventStream. Source parser/readiness, reader events/skipped state, async executor waker and OS waker are distinct owners. The previously reviewed constructor .ok error erasure, filtered queue priority, error-time skipped-event loss and no automatic alternate backend fallback remain explicit.

The full parser read confirms incremental byte parsing and input_available's special standalone-Esc role.099's already accepted normal-drain fix targets only a single Esc; it does not flush arbitrary partial UTF8, CSI or paste. CR and raw-mode LF differ, Unicode uppercase detection affects parsed SHIFT, whereas KeyEvent Eq/Hash use ASCII case normalization and do not mutate the received event. Repeat and Release remain separate kinds. Native paste accumulates until its end marker with lossy UTF8 conversion and no explicit total cap in this parser. CSI-u handles codepoint, modifiers/kind, shifted alternative, keypad/media/modifier and lock state, but does not emit the associated-text component. The actual tests verify specific examples and equality can conceal literal field differences; their presence is not a new execution result.

Two parser limitations are confirmed statically and stay open for product follow-up: cursor/RXVT/SGR coordinates subtract1 from parsed u16 without rejecting zero; X10 saturates the byte offset then subtracts1. This can panic with overflow checks or wrap in release. Enhancement flag parsing inspects the first ASCII digit's bits, so multi-digit flags are not parsed as the intended whole decimal value. No malformed-input experiment was run for402 and neither defect is declared repaired. No exhaustive platform, parser robustness or TUI acceptance follows from understanding this directory.

Cursor position query writes/flushed stdout, can temporarily enable raw mode and returns restoration failure ahead of a stored query result. Each poll has2000ms but error branches retry; there is no single function-wide deadline. Keyboard enhancement query tries /dev/tty then stdout, waits for flags or DA, and after flags uses unbounded read_internal to consume DA. Repeated poll errors and the second DA wait therefore are not covered by a blanket two-second guarantee. Raw mode has a separate saved-Termios mutex and is not reference counted or reconciled with external termios changes. Resize may reach tput, whose process wait has no local timeout and whose numeric fold does not validate arbitrary output/overflow or child exit success. These are actual query/size ownership limits, not new tested failures.

reset_event_reader uses try_lock and drops the reader under that guard. It clears in-process completed/partial/readiness state but does not flush OS input, alter raw/mouse/focus/paste modes, join other readers, persist a draft or coordinate queries. Both cursor and keyboard enhancement queries must be stopped externally. The reset API is absent with event-stream. Its existing100ms lock-contention test does not prove every destructor or terminal handoff is bounded. EventStream Drop signals/wakes without join and ignores some failures; public synchronous/async mixing restrictions remain. Windows command stubs/Unsupported paths and actual ANSI enable/disable commands do not imply constructing/destroying a source changes terminal modes.

This directory has no session/project correlation, WAL, provider lifecycle, persistent input queue or draft owner. Historical144 Rust and overlapping8/9 vendor tests,6891 projectPTY12/kill-yank29 and later92f95 product results remain bound to their own revisions. No runtime, build, provider, PTY, budget or old probe was executed by this402 acceptance. Headless pending-input repair remains a separate active product candidate, not integrated by this review. Original worker packing failures when401 became accepted and a verifier was not yet generated remain archived; the controller's initial read used an inapplicable files/ prefix and failed before correctly reading this package's direct Docs/ report layout.

Accept402 only after independently accepted401.403/404/405/root, six context-only files, sys directory,129/131 and the full goal remain separately open. This records complete understanding of the frozen directory and its real limits, not complete product parity. Rollback withdraws402 report/receipts/status only, preserving all child acceptance, product source, original failures and user state.
