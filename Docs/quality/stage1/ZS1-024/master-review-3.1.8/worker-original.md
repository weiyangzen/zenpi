# ZS1-024 — output-accumulator.ts

状态：[_] worker 完整阅读候选，主控未接受。
source_id: SRC-0953
source_path: packages/coding-agent/src/core/tools/output-accumulator.ts
source_hash: c601ddd8e10934be6f3db30696eacba7b9544f2dd1648a5ec3c9935f0d4625c3
source_bytes: 6049
read_ranges: [[0, 6049]]
run_id: zenpi-stage1-20260911

完整阅读 1–222 行；文件小于256KiB。OutputAccumulatorOptions 支持 maxLines/maxBytes/tempFilePrefix；默认 truncate 模块限制，rolling cap 至少1且为maxBytes*2。原始 Buffer chunks 在未溢出前保留，TextDecoder(stream=true)跨chunk处理UTF-8，解码文本独立跟踪totalDecodedBytes、行数、当前行字节；decoded与rawbytes不同，不能混当artifact原始范围。

append在finished后抛错；先增原始字节、追加解码tail，再判断超过原始字节/解码字节/行数任一阈值，ensureTempFile建立tmpdir随机8字节hex路径并写出此前rawChunks，后续直接write(data)。tailBytes超过rolling*2才trim，trim用UTF-8 continuation-byte对齐，不切坏字符，并记录起始是否行边界。snapshot按truncateTail裁剪显示；若tail起于半行且含换行，则跳到下一完整行；一条超长行无换行时仍保留尾部。totalLines=completed+openline，getLastLineBytes提供最后一行大小。

finish幂等、flush TextDecoder残留、必要时spill；它不关闭或验证临时文件。snapshot(persistIfTruncated)可确保临时路径存在，但完整性不能仅由fullOutputPath存在推断。closeTempFile以WriteStream finish/error Promise确认end，不在finish里自动调用，也没有fsync、读回hash、每流身份或持久恢复元数据。write返回的backpressure布尔值没有等待；原始输出没有磁盘单文件/总量/文件数量/TTL配额，也没有目录私有权限/范围读取权限合同。

对应zenpi G06：保存原始stdout/stderr应在read_limited_stream丢弃超过内存cap之前，而不是对既有截断Value再compact。111候选采用host选定私有目录、生成ID而非模型路径、openat/no-follow/单链接、有限单文件/总量/数量/TTL、流式hash和终态读回验证，显示尾部与原始字节范围分离。取消/磁盘错误不声明full；progress经过既有canonical view kind适配，slow consumer有drop marker。与pi的区别须保留：zenpi不会默许worker gate产生未经授予的raw artifact写入。此文件不能替代同目录bash/grep/find等独立阅读和目录报告。
