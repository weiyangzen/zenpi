# ZS1-300 主控独立验收

主控核对 Codex `chat_composer.rs`：374943B/9785L，SHA `234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da`。五个连续区间 1–2000、2001–4000、4001–6000、6001–8000、8001–9785 的原始字节拼接精确等于捕获源并到 EOF；manifest 15 项身份逐项一致。

报告覆盖 composer 状态机、键盘/popup、slash/file/mention、历史、paste burst、附件/远程图片、队列提交、voice、滚动和上层边界。该文件只作为 Codex TUI 消费者参考，不能推导 zenpi 已实现相同能力；136 个内嵌测试均未运行，跨平台语音、真实终端时序和像素行为未验收。
