

## 主控目录独立复核 — 3.1.20

主控逐项核对当前物理五文件、零子目录，正式子项严格为 ZS1-022 types.ts 与 ZS1-023 runner.ts。两份当前 canonical、master receipt 与当前源码逐字节一致，先前完整逐文件人工阅读仍绑定同一源 hash；R01–R06沿用该完整阅读，未声称重新全读。R07–R23共17段连接上下文在本次独立读取，全部23段另与真实源码范围/hash匹配。index/wrapper虽全文作为连接背景读取，仍不新增正式覆盖。

本目录理解确认 types/loader/Runner/AgentSession/wrapper 的注册、绑定、分派、结果和撤销关系；保留 hook 后参数复验、失败 reload 保旧等目标有意差异。直接读取54行历史组合 probe：9项是手工 host/runtime/callback 夹具调用真实源 helpers，theme mocked，没有执行 AgentSession/loader，没有实际子进程回收保证。本次不运行旧 probe，不合计旧12/20/9或新13计数。

加载 commit/discard 与 runtime invalidate 均可能因回调或退订抛出而部分完成；错误监听器也能中断分派。工具包装器只转发signal，不强制abort；失效发生在await期间时，拒绝结果不能撤销工具副作用。上述边界与当前源码相符，不能把目录接受理解为全Pi插件API兼容、115运行验收或父目录接受。整项接受仅在安装本唯一报告、独立目录回执和实际G-STAGE通过后成立。
