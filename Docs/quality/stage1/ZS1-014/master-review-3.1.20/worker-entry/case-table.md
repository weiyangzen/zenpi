| Source line | Actual test title | Real owner | Assertion boundary |
|---|---|---|---|
| 85 | uses the configured default when a legacy caller omits streamFn | runAgentLoop + getDefaultStreamFn | Reflect.apply省略streamFn；仅断言fallback被调用一次，finally恢复全局注册 |
| 119 | should emit events with AgentMessage types | runAgentLoop/event lifecycle | user+assistant两条结果与生命周期事件存在；不是精确全序断言 |
| 166 | should handle custom message types via convertToLlm | streamAssistantResponse.convertToLlm | 过滤notification后仅user传给转换结果；不验证真实provider角色规则 |
| 221 | should apply transformContext before convertToLlm | streamAssistantResponse transform/convert | 转换前后数组长度均2，原context五条；只覆盖裁剪接线，不覆盖异步竞态 |
| 274 | should handle tool calls and results | executeToolCalls/finalizeExecutedToolCall | echo回调实执行hello，start/end成功；after hook usage替换进入toolResult |
| 371 | should not execute tool calls from a length-truncated assistant message | failToolCallsFromTruncatedMessage | 单个length工具零回调、错误含output token limit，第二次模型调用允许恢复 |
| 444 | should execute mutated beforeToolCall args without revalidation | prepareToolCall + before hook | schema string参数经before hook变123，execute实际收到123；刻意证明不二次校验 |
| 506 | should prepare tool arguments for validation | prepareToolCallArguments + validator | oldText/newText改写edits后真实TypeBox校验，回调接收一组替换 |
| 586 | should emit tool_execution_end in completion order but persist tool results in source order | executeToolCallsParallel | Promise+20ms gate观察回调重叠，end=2,1，而message_end和turn_end结果=1,2 |
| 681 | should inject queued messages after all tool calls complete | runLoop steering boundary | 串行两回调和两result均先于interrupt，下一stream context含interrupt |
| 787 | should force sequential execution when a tool has executionMode=sequential even with default parallel config | executeToolCalls batch mode | 单个sequential工具被调用两次，parallelObserved=false且结果源序 |
| 870 | should force sequential execution when one of multiple tools has executionMode=sequential | executeToolCalls mixed mode | 弱断言：executionOrder首项slow且包含fast；未直接断言fast晚于slow完成 |
| 957 | should allow parallel execution when all tools have executionMode=parallel | executeToolCallsParallel | 全parallel工具两次调用，第一未完成时第二可执行，parallelObserved=true |
| 1031 | should use prepareNextTurn snapshot before continuing | runLoop.prepareNextTurn | 只验证systemPrompt替换、prepare一次、stream两次；无长prepare新steer/去重断言 |
| 1107 | should stop after the current turn when shouldStopAfterTurn returns true | runLoop.shouldStopAfterTurn | 一次stream/echo，steer只初poll一次、followUp零次；callback context和完整事件全序 |
| 1204 | should stop after a tool batch when every tool result sets terminate=true | shouldTerminateToolBatch | 仅一个工具terminate=true，stream一次、一次turn_end；不独立证明多工具every谓词 |
| 1256 | should stop after a blocked tool call when beforeToolCall sets terminate=true | beforeToolCall block + terminate | 单工具不执行、stream一次，错误toolResult含Blocked by policy |
| 1315 | should continue after a mixed batch with one terminating blocked call | prepareToolCall + mixed terminate | first被block/terminate，second实际echo，stream两次 |
| 1374 | should continue after parallel tool calls when not all tool results terminate | shouldTerminateToolBatch parallel | 两个回调仅first terminate，stream两次且结果角色序含两toolResult |
| 1439 | should allow afterToolCall to mark a tool batch as terminating | finalizeExecutedToolCall after hook | 单工具after hook terminate导致stream一次；非多工具全批证据 |
| 1489 | should throw when context has no messages | agentLoopContinue guard | 空context同步抛错，stream不得被调用 |
| 1508 | should continue from existing context without emitting user message events | runAgentLoopContinue | 已有user不重发，只有新assistant结果及message_end |
| 1550 | should allow custom message types as last message (caller responsibility) | convertToLlm custom continuation | custom尾部由fixture转换成user，返回一assistant；不是实际provider请求 |
