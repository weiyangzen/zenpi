# ZS1-098 — Cargo.lock

Worker candidate: [_]. Source scope addition authorized by controller and assigned ZS1-098 in blueprint 3.1.0.

source_path: `Cargo.lock`
source_hash: `f822e2d73d49727fdb4898180d26ff75bf95a4f81b638b613cb457e3171a30e1`
source_bytes: 42724
coverage: full bytes [0,42724), all 1664 lines including comments. Baseline HEAD: `6f252a20c628e9b1ede14e2887acc04657c71d7c`. Read before dependency modification.

## Full lockfile review

Version 4 generated lock with 182 package records. Registry records bind exact package versions to crates.io registry and SHA256 checksums; zenpi is the single workspace record and has no registry checksum. Dependencies identify package name, disambiguating explicit version for duplicate getrandom/hashbrown/syn/windows-sys/winnow. The complete package-by-package ledger below was inspected; each row carries the resolved dependency list, including platform-only Windows/WASI entries. Platform entries remain needed for cross-platform resolution even though local tests run arm64 Darwin.

Runtime families: terminal/layout use ratatui/crossterm, parking_lot/signal-hook/mio and Unicode crates; HTTPS uses ureq/rustls/ring/webpki/url/ICU; serialization uses serde/JSON/TOML and proc macros; SHA256 uses digest/generic-array; temp fixtures use tempfile/rustix/getrandom. Macro/build dependencies are compile-time only, not separate agent services. No YAML/ignore crate appears at baseline. No shell commands are encoded in lock records.

There are no functions or runtime state transitions in Cargo.lock. Its behavior is resolution reproducibility: missing package/checksum mismatch/network unavailable/unsupported toolchain produces Cargo failure, not partial product acceptance. Cancel/restart applies to Cargo build cache only, not skill catalogue mutation. New dependencies must be resolved by Cargo without bulk updating these versions. Tests --locked verify the resulting graph is usable; package checksums cannot substitute for runtime behavior evidence or security audit. Master must integrate this extra per-file report into its manifest before acceptance.

## Complete package ledger

| Package/version | Direct dependency references |
|---|---|
| adler2 2.0.1 | none |
| allocator-api2 0.2.21 | none |
| approx 0.5.1 | num-traits |
| autocfg 1.5.1 | none |
| base64 0.23.1 | none |
| bitflags 2.13.1 | none |
| block-buffer 0.10.4 | generic-array |
| by_address 1.2.1 | none |
| bytes 1.12.1 | none |
| castaway 0.2.4 | rustversion |
| cc 1.4.4 | find-msvc-tools, shlex |
| cfg-if 1.0.4 | none |
| compact_str 0.9.1 | castaway, cfg-if, itoa, rustversion, ryu, static_assertions |
| convert_case 0.10.0 | unicode-segmentation |
| cookie 0.18.2 | percent-encoding, time, version_check |
| cookie_store 0.22.0 | cookie, document-features, idna, indexmap, log, serde, serde_derive, serde_json, time, url |
| cpufeatures 0.2.17 | libc |
| crc32fast 1.5.1 | cfg-if |
| crossterm 0.29.0 | bitflags, crossterm_winapi, derive_more, document-features, mio, parking_lot, rustix, signal-hook, signal-hook-mio, winapi |
| crossterm_winapi 0.9.1 | winapi |
| crypto-common 0.1.7 | generic-array, typenum |
| darling 0.20.11 | darling_core, darling_macro |
| darling_core 0.20.11 | fnv, ident_case, proc-macro2, quote, strsim, syn 2.0.119 |
| darling_macro 0.20.11 | darling_core, quote, syn 2.0.119 |
| deranged 0.5.8 | none |
| derive_more 2.1.1 | derive_more-impl |
| derive_more-impl 2.1.1 | convert_case, proc-macro2, quote, rustc_version, syn 2.0.119 |
| digest 0.10.7 | block-buffer, crypto-common |
| displaydoc 0.2.7 | proc-macro2, quote, syn 3.0.4 |
| document-features 0.2.12 | litrs |
| either 1.18.0 | none |
| equivalent 1.0.2 | none |
| errno 0.3.14 | libc, windows-sys 0.61.2 |
| fastrand 2.5.0 | none |
| find-msvc-tools 0.1.11 | none |
| flate2 1.1.10 | crc32fast, miniz_oxide, zlib-rs |
| fnv 1.0.7 | none |
| foldhash 0.2.0 | none |
| form_urlencoded 1.2.2 | percent-encoding |
| generic-array 0.14.7 | typenum, version_check |
| getrandom 0.2.17 | cfg-if, libc, wasi |
| getrandom 0.4.3 | cfg-if, libc, r-efi |
| hashbrown 0.16.1 | allocator-api2, equivalent, foldhash |
| hashbrown 0.17.1 | allocator-api2, equivalent, foldhash |
| heck 0.5.0 | none |
| http 1.5.0 | bytes, itoa |
| httparse 1.10.1 | none |
| httpdate 1.0.3 | none |
| icu_collections 2.1.1 | displaydoc, potential_utf, yoke, zerofrom, zerovec |
| icu_locale_core 2.1.1 | displaydoc, litemap, tinystr, writeable, zerovec |
| icu_normalizer 2.1.1 | icu_collections, icu_normalizer_data, icu_properties, icu_provider, smallvec, zerovec |
| icu_normalizer_data 2.1.1 | none |
| icu_properties 2.1.2 | icu_collections, icu_locale_core, icu_properties_data, icu_provider, zerotrie, zerovec |
| icu_properties_data 2.1.2 | none |
| icu_provider 2.1.1 | displaydoc, icu_locale_core, writeable, yoke, zerofrom, zerotrie, zerovec |
| ident_case 1.0.1 | none |
| idna 1.1.0 | idna_adapter, smallvec, utf8_iter |
| idna_adapter 1.2.1 | icu_normalizer, icu_properties |
| indexmap 2.14.1 | equivalent, hashbrown 0.17.1 |
| indoc 2.0.7 | rustversion |
| instability 0.3.10 | darling, indoc, proc-macro2, quote, syn 2.0.119 |
| itertools 0.14.0 | either |
| itoa 1.0.18 | none |
| kasuari 0.4.12 | hashbrown 0.16.1, portable-atomic, thiserror |
| libc 0.2.189 | none |
| libm 0.2.16 | none |
| line-clipping 0.3.8 | bitflags |
| linux-raw-sys 0.12.1 | none |
| litemap 0.8.3 | none |
| litrs 1.0.0 | none |
| lock_api 0.4.14 | scopeguard |
| log 0.4.34 | none |
| lru 0.18.4 | hashbrown 0.17.1 |
| memchr 2.8.3 | none |
| miniz_oxide 0.9.1 | adler2, simd-adler32 |
| mio 1.2.3 | libc, log, wasi, windows-sys 0.61.2 |
| num-conv 0.2.2 | none |
| num-traits 0.2.19 | autocfg |
| num_threads 0.1.7 | libc |
| once_cell 1.21.4 | none |
| palette 0.7.7 | approx, libm, palette_derive, palette_math |
| palette_derive 0.7.7 | by_address, proc-macro2, quote, syn 2.0.119 |
| palette_math 0.7.7 | libm |
| parking_lot 0.12.5 | lock_api, parking_lot_core |
| parking_lot_core 0.9.12 | cfg-if, libc, redox_syscall, smallvec, windows-link |
| percent-encoding 2.3.2 | none |
| portable-atomic 1.15.0 | none |
| potential_utf 0.1.6 | zerovec |
| powerfmt 0.2.0 | none |
| proc-macro2 1.0.107 | unicode-ident |
| quote 1.0.47 | proc-macro2 |
| r-efi 6.0.0 | none |
| ratatui 0.30.2 | instability, ratatui-core, ratatui-crossterm, ratatui-widgets, serde |
| ratatui-core 0.1.2 | bitflags, compact_str, hashbrown 0.17.1, itertools, kasuari, lru, palette, serde, strum, thiserror, unicode-segmentation, unicode-truncate, unicode-width |
| ratatui-crossterm 0.1.2 | cfg-if, crossterm, instability, ratatui-core |
| ratatui-widgets 0.3.2 | bitflags, hashbrown 0.17.1, indoc, instability, itertools, line-clipping, ratatui-core, serde, strum, time, unicode-segmentation, unicode-width |
| redox_syscall 0.5.18 | bitflags |
| ring 0.17.14 | cc, cfg-if, getrandom 0.2.17, libc, untrusted, windows-sys 0.52.0 |
| rustc_version 0.4.1 | semver |
| rustix 1.1.4 | bitflags, errno, libc, linux-raw-sys, windows-sys 0.61.2 |
| rustls 0.23.43 | log, once_cell, ring, rustls-pki-types, rustls-webpki, subtle, zeroize |
| rustls-pki-types 1.15.1 | zeroize |
| rustls-webpki 0.103.15 | ring, rustls-pki-types, untrusted |
| rustversion 1.0.23 | none |
| ryu 1.0.23 | none |
| scopeguard 1.2.0 | none |
| semver 1.0.28 | none |
| serde 1.0.229 | serde_core, serde_derive |
| serde_core 1.0.229 | serde_derive |
| serde_derive 1.0.229 | proc-macro2, quote, syn 3.0.4 |
| serde_json 1.0.151 | itoa, memchr, serde, serde_core, zmij |
| serde_spanned 1.1.1 | serde_core |
| sha2 0.10.9 | cfg-if, cpufeatures, digest |
| shlex 2.0.1 | none |
| signal-hook 0.3.18 | libc, signal-hook-registry |
| signal-hook-mio 0.2.5 | libc, mio, signal-hook |
| signal-hook-registry 1.4.8 | errno, libc |
| simd-adler32 0.3.10 | none |
| smallvec 1.16.0 | none |
| stable_deref_trait 1.2.1 | none |
| static_assertions 1.1.0 | none |
| strsim 0.11.1 | none |
| strum 0.28.0 | strum_macros |
| strum_macros 0.28.0 | heck, proc-macro2, quote, syn 2.0.119 |
| subtle 2.6.1 | none |
| syn 2.0.119 | proc-macro2, quote, unicode-ident |
| syn 3.0.4 | proc-macro2, quote, unicode-ident |
| synstructure 0.13.2 | proc-macro2, quote, syn 2.0.119 |
| tempfile 3.27.0 | fastrand, getrandom 0.4.3, once_cell, rustix, windows-sys 0.61.2 |
| thiserror 2.0.20 | thiserror-impl |
| thiserror-impl 2.0.20 | proc-macro2, quote, syn 3.0.4 |
| time 0.3.55 | deranged, libc, num-conv, num_threads, powerfmt, serde_core, time-core, time-macros |
| time-core 0.1.9 | none |
| time-macros 0.2.32 | num-conv, time-core |
| tinystr 0.8.4 | displaydoc, zerovec |
| toml 0.9.12+spec-1.1.0 | indexmap, serde_core, serde_spanned, toml_datetime, toml_parser, toml_writer, winnow 0.7.15 |
| toml_datetime 0.7.5+spec-1.1.0 | serde_core |
| toml_parser 1.1.3+spec-1.1.0 | winnow 1.0.4 |
| toml_writer 1.1.2+spec-1.1.0 | none |
| typenum 1.20.1 | none |
| unicode-ident 1.0.24 | none |
| unicode-segmentation 1.13.3 | none |
| unicode-truncate 2.0.1 | itertools, unicode-segmentation, unicode-width |
| unicode-width 0.2.2 | none |
| untrusted 0.9.0 | none |
| ureq 3.4.0 | base64, cookie_store, flate2, log, percent-encoding, rustls, rustls-pki-types, serde, serde_json, ureq-proto, utf8-zero, webpki-roots |
| ureq-proto 0.6.1 | base64, http, httparse, log |
| url 2.5.8 | form_urlencoded, idna, percent-encoding, serde |
| utf8-zero 0.8.1 | none |
| utf8_iter 1.0.4 | none |
| version_check 0.9.5 | none |
| wasi 0.11.1+wasi-snapshot-preview1 | none |
| webpki-roots 1.0.9 | rustls-pki-types |
| winapi 0.3.9 | winapi-i686-pc-windows-gnu, winapi-x86_64-pc-windows-gnu |
| winapi-i686-pc-windows-gnu 0.4.0 | none |
| winapi-x86_64-pc-windows-gnu 0.4.0 | none |
| windows-link 0.2.1 | none |
| windows-sys 0.52.0 | windows-targets |
| windows-sys 0.61.2 | windows-link |
| windows-targets 0.52.6 | windows_aarch64_gnullvm, windows_aarch64_msvc, windows_i686_gnu, windows_i686_gnullvm, windows_i686_msvc, windows_x86_64_gnu, windows_x86_64_gnullvm, windows_x86_64_msvc |
| windows_aarch64_gnullvm 0.52.6 | none |
| windows_aarch64_msvc 0.52.6 | none |
| windows_i686_gnu 0.52.6 | none |
| windows_i686_gnullvm 0.52.6 | none |
| windows_i686_msvc 0.52.6 | none |
| windows_x86_64_gnu 0.52.6 | none |
| windows_x86_64_gnullvm 0.52.6 | none |
| windows_x86_64_msvc 0.52.6 | none |
| winnow 0.7.15 | none |
| winnow 1.0.4 | none |
| writeable 0.6.4 | none |
| yoke 0.8.3 | stable_deref_trait, yoke-derive, zerofrom |
| yoke-derive 0.8.2 | proc-macro2, quote, syn 2.0.119, synstructure |
| zenpi 0.1.0 | base64, crossterm, httpdate, libc, ratatui, serde, serde_json, sha2, tempfile, thiserror, toml, unicode-width, ureq |
| zerofrom 0.1.8 | zerofrom-derive |
| zerofrom-derive 0.1.7 | proc-macro2, quote, syn 2.0.119, synstructure |
| zeroize 1.9.0 | none |
| zerotrie 0.2.5 | displaydoc, yoke, zerofrom |
| zerovec 0.11.8 | yoke, zerofrom, zerovec-derive |
| zerovec-derive 0.11.6 | proc-macro2, quote, syn 3.0.4 |
| zlib-rs 0.6.7 | none |
| zmij 1.0.23 | none |

## ZS1-098 当前 3.1.21 锁文件全文复核（2026-09-13）

本段是唯一 `Cargo.lock` owner 的当前候选 `[_]`。上方旧 3.1.0 报告为原样历史前缀，其 182 包、单一无 source 条目以及“baseline 不含 ignore/YAML”等陈述绑定旧源，不能当作当前锁文件结论。主库目标报告在捕获时不存在，worker 已有报告 10995B / 203L / `638da9ac927084138c9cdde8d8637a024e751f96f64159a202c50dae1f6e92b9`，已完整阅读并保留。只交付 `Docs/learn/stage1_pi_mono/targets/zenpi/files/Cargo.lock_learn.md`，不增加目录、Cargo.toml、vendor 或其他 item credit。

权威捕获为 3.1.21、派发正式进度 54/121，requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，blueprint snapshot `7577131ead9a6c2c77481ee58af8a83db2124e8570b0b6aad7ea54a2afc76c80`。本候选不写 authority、claim 或 master receipt；进度只作为派发时上下文。

### 三份源身份与完整阅读

| 对象 | 字节 / 行 | SHA-256 | 本轮阅读性质 |
| --- | --- | --- | --- |
| 冻结登记基线，git `6f252a20c628e9b1ede14e2887acc04657c71d7c:Cargo.lock` | 42724 / 1664 | f822e2d73d49727fdb4898180d26ff75bf95a4f81b638b613cb457e3171a30e1 | 原字节保存、结构比较；不冒充本轮新全文阅读 |
| worker 现有 Cargo.lock | 46164 / 1804 | d266eb5b8f61d08ea1d8e3aeca0987f97a28384ecb21558a279d0d7d84d80f12 | 独立 preimage 保存、差异阅读，保持不变 |
| 主库当前 Cargo.lock | 46021 / 1802 | 7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348 | 连续完整阅读 1–300、301–600、601–900、901–1200、1201–1500、1501–1802 |

当前全部注释、空行、版本头、196 个 package 记录均已读至 EOF；每块原文、源 hash、行范围和 byte range 位于 `read-binding.json`，六段拼接精确覆盖 [0,46021)。197 个连续语义单元 = 头部 1–4 行 + 196 个包块，块身份存于 `semantic-units.json` 与 `package-ledger.json`。个别阅读块边界落在 dependencies 数组中间，下一块紧接续读；包块绑定独立于阅读块边界。文件末尾 `zmij` checksum 与终止换行均属于最后包块。10 条绑定记录还包括旧报告 1–203、根 Cargo.toml 1–50、vendor 标准 Cargo.toml 1–240 和 Cargo.toml.orig 1–122；后三者只作消费者/配置上下文，不获得正式 owner 全文 credit。

### 锁格式、source 与 checksum

1–4 行标明 Cargo 生成文件、不可手工编辑的提示及 lock format version=4。该 4 是锁格式号，不是项目版本、Rust edition 或执行协议号。本次离线 TOML 读取发现顶层为 version/package；每个包仅出现 name、version、source、checksum、dependencies 五类键，未出现 git source、replace、patch.unused 或独立 metadata 块。文件没有函数、测试声明、执行命令、scheduler、runtime 状态、环境读取或路径扫描逻辑。

当前 196 包对应 191 个不同名字，其中 194 包携带相同 source 字符串 `registry+https://github.com/rust-lang/crates.io-index` 和 64 位小写十六进制 checksum。这个字符串在此作为 registry source 身份，不证明本机实际传输使用 Git 还是 sparse、镜像、缓存或 source replacement；本轮没有读取用户/祖先 Cargo 配置或环境，没有发起网络请求。checksum 的语法与记录位置已经绑定，没有拿下载的 `.crate` 原始档案重算比较；因此既不证明 registry 内容真实，也不证明无已知漏洞、许可证合规、无恶意代码或已验证构建产物。

两个无 source/checksum 条目分别为 `crossterm 0.29.0`（204–219）与本项目 `zenpi 0.1.0`（1710–1731）。后者是根项目；前者由根 Cargo.toml:48–50 的 `[patch.crates-io] crossterm={path="vendor/crossterm"}` 提供本地来源。锁记录本身没有 path 字段，也没有冻结 vendor 源树的内容摘要；即使名称/版本/lock hash 都不变，本地 patch 源码仍可能变化，源码完整性须另由 vendor owner/仓库证据约束。不能给当前 crossterm 补回旧 registry checksum 并声称本地代码受到它保护。

### 每条依赖引用与同名多版本

196 个包块包含 389 条依赖引用，361 条仅有名字，28 条另带版本；当前没有带括号 source 的 dependency reference。`dependency-edges.json` 按父包、原始引用、目标 Pxxx 保存全部解析映射。对这个文件的实际语法逐一匹配，389 条均唯一命中，未出现缺失或歧义引用；以 zenpi 为根，忽略平台/feature/依赖类型的结构遍历可达全部 196 包。该遍历不是 Cargo resolver，不能说明全部包会在某个 target/feature 构建中编译，也不能证明 semver 兼容、feature 统一或 target cfg 激活正确。

| 同名包 | 锁定版本 | 明确引用者示例 |
| --- | --- | --- |
| getrandom | 0.2.17、0.4.3 | ring→0.2.17；tempfile→0.4.3 |
| hashbrown | 0.16.1、0.17.1 | kasuari→0.16.1；indexmap/lru/ratatui-core/widgets→0.17.1 |
| syn | 2.0.119、3.0.4 | darling_core/macro、derive_more-impl、instability、palette_derive、strum_macros、synstructure、yoke-derive、zerofrom-derive→2.0.119；displaydoc、serde_derive、thiserror-impl、zerovec-derive→3.0.4 |
| windows-sys | 0.52.0、0.61.2 | ring→0.52.0；errno/mio/rustix/tempfile/winapi-util→0.61.2 |
| winnow | 0.7.15、1.0.4 | toml→0.7.15；toml_parser→1.0.4 |

同名多版本是不同记录，不能按名字去重后把引用合并。版本中的 build metadata 也按原串保留，例如 `toml 0.9.12+spec-1.1.0`、`wasi 0.11.1+wasi-snapshot-preview1`；锁定 exact version 与根清单里的 version requirement 不相同。包排列基本按名字分组，不能当作编译或执行的拓扑顺序。

### 根 Cargo.toml 与当前图的对应

根 Cargo.toml:1–50 将项目声明为 zenpi 0.1.0、edition=2024、rust-version=1.88；这些不存于 Cargo.lock。features 为 default=[] 和 dev-fixtures=[]；release 的 opt-level/lto/strip/codegen-units/panic 与 example 路径也不由 lock 表达。根的 16 个引用 = 14 个普通 dependency + Unix 的 libc + dev 的 tempfile，全部名称在 zenpi dependencies 中出现。`manifest-direct-bindings.json` 保留每项清单原 spec 和锁版本。

普通项为 base64 0.23→0.23.1、crossterm 0.29→0.29.0（bracketed-paste）、httpdate 1.0→1.0.3、ignore 0.4→0.4.33、ratatui 0.30→0.30.2（关闭默认特性，开启 crossterm）、serde 1.0→1.0.229（derive）、serde_json 1.0→1.0.151、sha2 0.10→0.10.9、thiserror 2.0→2.0.20、toml 0.9→0.9.12+spec-1.1.0、unicode-width 0.2→0.2.2、unicode-segmentation **=1.13.3**→1.13.3、ureq 3.4→3.4.0（默认特性开启并选 json）、yaml_serde 0.10→0.10.7。Unix libc 0.2→0.2.189，dev tempfile 3.23→3.27.0。箭头表示清单声明与当前锁记录的对应，不表示本轮调用 Cargo 验证了约束或所有 features。

依赖簇的文件内关系完整保留：ratatui→core/crossterm/widgets，core→kasuari/lru/palette/Unicode；ureq→proto、rustls、cookie_store、flate2、webpki-roots，cookie_store→url/idna/ICU，rustls/webpki→ring；sha2→digest→block-buffer/crypto-common；serde_derive、thiserror-impl、displaydoc 等指向 proc-macro2/quote/syn。锁文件没有 runtime/build/dev 分类，旧报告“macro/build dependencies compile-time only”应限定为包职责背景，不能由锁条目推导每一条边的构建阶段。下载或编译依赖可能涉及 crate 自身的构建行为，只有 lock 不足以审计这些行为。

新增 ignore 路径在当前为 ignore→crossbeam-deque/globset/log/memchr/regex-automata/same-file/walkdir/winapi-util；globset→aho-corasick/bstr/log/regex-automata/regex-syntax；crossbeam-deque→epoch/utils；same-file/walkdir→winapi-util→windows-sys 0.61.2。yaml_serde→indexmap/itoa/libyaml-rs/ryu/serde，libyaml-rs 0.3.0 在该锁条目没有 dependencies。这里不凭名字推出 parser 安全、忽略策略正确、FFI 实现或许可证事实。

### 平台、feature 与 vendor patch 的边界

Windows 相关项在 1537–1655 行完整保留：winapi 两个 GNU 架构包；windows-sys 0.52.0→windows-targets 0.52.6→8 个 aarch64/i686/x86_64 与 GNU/GNULLVM/MSVC 包；windows-sys 0.61.2→windows-link 0.2.1。其他平台相关引用包括 getrandom 0.2.17/mio→wasi、getrandom 0.4.3→r-efi、rustix→linux-raw-sys、parking_lot_core→redox_syscall。lock 数组不带 cfg 条件，不能以本机 macOS 为由删掉这些记录，也不能据其出现宣称已经在 Windows/WASI/UEFI/Redox 构建过。

作为消费者上下文的 vendor/crossterm/Cargo.toml:12–43 明确 name/version、edition=2021、rust-version=1.63.0、build=false、lib path=src/lib.rs；它是标准化 manifest，开头说明与 Cargo.toml.orig 的差别。48–78 默认 features 包含 bracketed-paste、events、windows、derive-more；根直接 dependency 没有关闭 crossterm 默认特性，而 ratatui 自身的 default-features=false 不自动关闭独立 crossterm 声明的默认特性。198–240 行给出 Unix mio/rustix/signal-hook 与 Windows winapi/crossterm_winapi 的目标条件，解释为何锁中同时包含两组依赖。

其 event-stream、osc52、serde、use-dev-tty 为可选声明；当前 crossterm 锁条目只列 bitflags/crossterm_winapi/derive_more/document-features/mio/parking_lot/rustix/signal-hook/signal-hook-mio/winapi。不能把根图已有的 base64 0.23.1 当作 vendor osc52 所需 base64 0.22；也不能把任意 optional/dev 声明都要求出现在该根 lock 的 crossterm dependencies。vendor 中的 async-std/futures/tokio 等 dev-dependencies 和 required-features examples 并非 root 的 dev-dependencies。vendor 自己有 Cargo.lock 文件这一目录观察不代表根锁改为消费那份锁；本轮没有读或运行它。仓库根 `.cargo/config` 与 `.cargo/config.toml` 均不存在，但未检查用户/祖先配置、环境或 CLI override，结论仅限这两个路径。未读取 vendor 源码，不认领 ZS1-131 的 reset 修复或任何运行结果。

### 冻结基线和 worker 差异

与登记基线比较，当前净增 3297B / 138L，182→196 包；按 `(name,version)` 计新增 14 个、删除 0 个。新增清单为 aho-corasick 1.1.5、bstr 1.13.1、crossbeam-deque 0.8.8、crossbeam-epoch 0.9.21、crossbeam-utils 0.8.23、globset 0.4.20、ignore 0.4.33、libyaml-rs 0.3.0、regex-automata 0.4.18、regex-syntax 0.8.11、same-file 1.0.6、walkdir 2.5.0、winapi-util 0.1.11、yaml_serde 0.10.7。共同 `(name,version)` 中只有 crossterm、zenpi 的记录改变：crossterm 移除 registry source 和旧 checksum，版本与十条依赖引用不变；zenpi 增加 ignore、unicode-segmentation、yaml_serde 三个直接引用。unicode-segmentation 之前已经是传递依赖，因此不计新增包。其余 180 个共同记录在 TOML 值层面不变。

上述“删除 0”仅按 name/version 比较；如果把 source 纳入 package 身份，crossterm 已由 registry 来源替换为本地来源，不能描述成来源也未改变。保留完整 `baseline-to-current.diff` 与记录级比较，不靠净行数代替阅读。worker 现有锁与主库相比仅 crossterm 的两行 source/checksum 被移除，当前净少 143B / 2L，包名版本、依赖边均无其他差异；`worker-to-current.diff` 单独保存，worker 锁没有被修改。

### 验证、失败记录与交付含义

当前离线分析使用本地 Python TOML 解析和逐块绑定，不调用 Cargo、测试、产品、PTY、HTTP、release、旧 runner 或网络。389 个引用唯一匹配、196 包全部结构可达、194 checksum 语法正确只支持文件内一致性。未验证依赖档案内容、构建脚本、实际 feature/target 解析、rust-version 全图要求、可重现构建、漏洞、许可证或产品行为。历史报告没有本 owner 的可定位运行日志或测试通过记录，本轮不据旧 prose 增补测试通过。

新准备阶段保留一次 `capture.py` exit 1：新目录 history 尚未创建，复制旧报告时 FileNotFoundError；`finish-capture.py` 从已保留捕获继续，并新建该目录，原失败脚本未改动或重跑。`capture-failure.log` 按原工具输出保存异常文本，`preparation-issues.json` 标明这是准备错误，非冻结 verifier 执行。初次 read-only rg 搜索可选 .cargo 目录返回 exit 2，原因是目录不存在；发现的 vendor manifest 和错误另存 `discovery-observation.json`，未掩盖或归作 Cargo 失败。全部 110 个既有 ready / 33592 个 regular 文件、145 个 tracked 文件及旧报告均保持原身份。没有删除或重跑任何旧失败材料。

交付包含主库报告缺失 preimage、worker 报告 exact preimage、主库正反补丁、worker 追加/回退补丁。候选只新增一份最终报告，old prefix 原样保留；冻结后的 verifier 完整静态阅读与 AST 检查之后最多执行一次，日志与回执在包外。该 verifier 的报告补丁只在 TemporaryDirectory 内应用/撤销，检查完整清单、hash、原始阅读块及记录/边绑定，不能给出 master `[x]` 或构建、安全审计通过。目录汇总依赖未来的独立接受文件，不在本轮修改。

## 当前完整 package ledger

R = registry+https://github.com/rust-lang/crates.io-index；L = 本地来源、无 registry checksum。依赖引用逐字保留；块 hash 及所有 389 条目标绑定在 package-ledger.json / dependency-edges.json。

| ID / 行 | 包名 / 版本 | 来源 / checksum | 全部依赖引用 |
| --- | --- | --- | --- |
| P001 / 5–10 | adler2 / 2.0.1 | R / 320119579fcad9c21884f5c4861d16174d0e06250625266f50fe6898340abefa | 无 |
| P002 / 11–19 | aho-corasick / 1.1.5 | R / c982642fa9e8606056828ee9a8505737230110bb1099153c79efe865c59d12ba | memchr |
| P003 / 20–25 | allocator-api2 / 0.2.21 | R / 683d7910e743518b0e34f1186f92494becacb047c7b6bf616c96772180fef923 | 无 |
| P004 / 26–34 | approx / 0.5.1 | R / cab112f0a86d568ea0e627cc1d6be74a1e9cd55214684db5561995f6dad897c6 | num-traits |
| P005 / 35–40 | autocfg / 1.5.1 | R / f2032f911046de80f0a198e0901378627c33f59ea0ac00e363d481118bd70a53 | 无 |
| P006 / 41–46 | base64 / 0.23.1 | R / ac07cdecf99051d9a5238b80f35af32cdeba5b336e55d957b318b50137e18da5 | 无 |
| P007 / 47–52 | bitflags / 2.13.1 | R / b588b76d00fde79687d7646a9b5bdf3cc0f655e0bbd080335a95d7e96f3587da | 无 |
| P008 / 53–61 | block-buffer / 0.10.4 | R / 3078c7629b62d3f0439517fa394996acacc5cbc91c5a20d8c658e77abd503a71 | generic-array |
| P009 / 62–71 | bstr / 1.13.1 | R / 6bb31b46c14244e20ee9984b11bf5c992b91fb6939fea616e3512c8baecdbe5f | memchr, serde_core |
| P010 / 72–77 | by_address / 1.2.1 | R / 64fa3c856b712db6612c019f14756e64e4bcea13337a6b33b696333a9eaa2d06 | 无 |
| P011 / 78–83 | bytes / 1.12.1 | R / fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04 | 无 |
| P012 / 84–92 | castaway / 0.2.4 | R / dec551ab6e7578819132c713a93c022a05d60159dc86e7a7050223577484c55a | rustversion |
| P013 / 93–102 | cc / 1.4.4 | R / 0ad534f4357a5264cce5019c989cf66a4f0dc4e0d1b1d15f8aacec0ff7360273 | find-msvc-tools, shlex |
| P014 / 103–108 | cfg-if / 1.0.4 | R / 9330f8b2ff13f34540b44e946ef35111825727b38d33286ef986142615121801 | 无 |
| P015 / 109–122 | compact_str / 0.9.1 | R / 9dfdd1c2274d9aa354115b09dc9a901d6c5576818cdf70d14cae2bdb47df00ab | castaway, cfg-if, itoa, rustversion, ryu, static_assertions |
| P016 / 123–131 | convert_case / 0.10.0 | R / 633458d4ef8c78b72454de2d54fd6ab2e60f9e02be22f3c6104cdc8a4e0fceb9 | unicode-segmentation |
| P017 / 132–142 | cookie / 0.18.2 | R / 1a373e3602691c3cdea496d2f0ee5935151e6168fe87739483c463db1b2f2f87 | percent-encoding, time, version_check |
| P018 / 143–160 | cookie_store / 0.22.0 | R / 3fc4bff745c9b4c7fb1e97b25d13153da2bc7796260141df62378998d070207f | cookie, document-features, idna, indexmap, log, serde, serde_derive, serde_json, time, url |
| P019 / 161–169 | cpufeatures / 0.2.17 | R / 59ed5838eebb26a2bb2e58f6d5b5316989ae9d08bab10e0e6d103e656d1b0280 | libc |
| P020 / 170–178 | crc32fast / 1.5.1 | R / 8498c871161e1742aaa9d52551b2d6ebdd4c3d45a3be423e3728f33b955be550 | cfg-if |
| P021 / 179–188 | crossbeam-deque / 0.8.8 | R / 622f3fc73690be383c7214310406f28a90e6edeadc3cea882f9d71e495b9711a | crossbeam-epoch, crossbeam-utils |
| P022 / 189–197 | crossbeam-epoch / 0.9.21 | R / dc74980687109a3b14c72fd458107bf0baa1da1a1a805e178d15501ba9b86d9d | crossbeam-utils |
| P023 / 198–203 | crossbeam-utils / 0.8.23 | R / a31eee39dddec8330830986fcd7625edb5a24ec90ea038215273bbc3adb08ac6 | 无 |
| P024 / 204–219 | crossterm / 0.29.0 | L / 无 | bitflags, crossterm_winapi, derive_more, document-features, mio, parking_lot, rustix, signal-hook, signal-hook-mio, winapi |
| P025 / 220–228 | crossterm_winapi / 0.9.1 | R / acdd7c62a3665c7f6830a51635d9ac9b23ed385797f70a83bb8bafe9c572ab2b | winapi |
| P026 / 229–238 | crypto-common / 0.1.7 | R / 78c8292055d1c1df0cce5d180393dc8cce0abec0a7102adb6c7b1eef6016d60a | generic-array, typenum |
| P027 / 239–248 | darling / 0.20.11 | R / fc7f46116c46ff9ab3eb1597a45688b6715c6e628b5c133e288e709a29bcb4ee | darling_core, darling_macro |
| P028 / 249–262 | darling_core / 0.20.11 | R / 0d00b9596d185e565c2207a0b01f8bd1a135483d02d9b7b0a54b11da8d53412e | fnv, ident_case, proc-macro2, quote, strsim, syn 2.0.119 |
| P029 / 263–273 | darling_macro / 0.20.11 | R / fc34b93ccb385b40dc71c6fceac4b2ad23662c7eeb248cf10d529b7e055b6ead | darling_core, quote, syn 2.0.119 |
| P030 / 274–279 | deranged / 0.5.8 | R / 7cd812cc2bc1d69d4764bd80df88b4317eaef9e773c75226407d9bc0876b211c | 无 |
| P031 / 280–288 | derive_more / 2.1.1 | R / d751e9e49156b02b44f9c1815bcb94b984cdcc4396ecc32521c739452808b134 | derive_more-impl |
| P032 / 289–301 | derive_more-impl / 2.1.1 | R / 799a97264921d8623a957f6c3b9011f3b5492f557bbb7a5a19b7fa6d06ba8dcb | convert_case, proc-macro2, quote, rustc_version, syn 2.0.119 |
| P033 / 302–311 | digest / 0.10.7 | R / 9ed9a281f7bc9b7576e61468ba615a66a5c8cfdff42420a70aa82701a3b1e292 | block-buffer, crypto-common |
| P034 / 312–322 | displaydoc / 0.2.7 | R / c6232dd377dcc64799954cbd3a9bb882e9cdc1308ccd87b1c098f1fb2eaf82a8 | proc-macro2, quote, syn 3.0.4 |
| P035 / 323–331 | document-features / 0.2.12 | R / d4b8a88685455ed29a21542a33abd9cb6510b6b129abadabdcef0f4c55bc8f61 | litrs |
| P036 / 332–337 | either / 1.18.0 | R / 252afb9ae5eaa683babdc6a068b3f5726eb19e05070c731f9b2a23a7c3e8ed34 | 无 |
| P037 / 338–343 | equivalent / 1.0.2 | R / 877a4ace8713b0bcf2a4e7eec82529c029f1d0619886d18145fea96c3ffe5c0f | 无 |
| P038 / 344–353 | errno / 0.3.14 | R / 39cab71617ae0d63f51a36d69f866391735b51691dbda63cf6f96d042b63efeb | libc, windows-sys 0.61.2 |
| P039 / 354–359 | fastrand / 2.5.0 | R / da7c62ceae207dd37ea5b845da6a0696c799f85e97da1ab5b7910be3c1c80223 | 无 |
| P040 / 360–365 | find-msvc-tools / 0.1.11 | R / d45db016d36b838f563236e9193d0ee6ce38f3f68b6c94e914b4929c96bbb890 | 无 |
| P041 / 366–376 | flate2 / 1.1.10 | R / 6e634e2e0ebac1ee034020da1ca582e17ffe4e0f5e985823721e168928136dcb | crc32fast, miniz_oxide, zlib-rs |
| P042 / 377–382 | fnv / 1.0.7 | R / 3f9eec918d3f24069decb9af1554cad7c880e2da24a9afd88aca000531ab82c1 | 无 |
| P043 / 383–388 | foldhash / 0.2.0 | R / 77ce24cb58228fbb8aa041425bb1050850ac19177686ea6e0f41a70416f56fdb | 无 |
| P044 / 389–397 | form_urlencoded / 1.2.2 | R / cb4cb245038516f5f85277875cdaa4f7d2c9a0fa0468de06ed190163b1581fcf | percent-encoding |
| P045 / 398–407 | generic-array / 0.14.7 | R / 85649ca51fd72272d7821adaf274ad91c288277713d9c18820d8499a7ff69e9a | typenum, version_check |
| P046 / 408–418 | getrandom / 0.2.17 | R / ff2abc00be7fca6ebc474524697ae276ad847ad0a6b3faa4bcb027e9a4614ad0 | cfg-if, libc, wasi |
| P047 / 419–429 | getrandom / 0.4.3 | R / 300e883d756b2e4ec94e02791f39b04b522276138852cfc41d9fb7e904106099 | cfg-if, libc, r-efi |
| P048 / 430–442 | globset / 0.4.20 | R / 07c34a9410465b45bd9787443bc7370f37735bad04b0f0cd57ff1a3186c98988 | aho-corasick, bstr, log, regex-automata, regex-syntax |
| P049 / 443–453 | hashbrown / 0.16.1 | R / 841d1cc9bed7f9236f321df977030373f4a4163ae1a7dbfe1a51a2c1a51d9100 | allocator-api2, equivalent, foldhash |
| P050 / 454–464 | hashbrown / 0.17.1 | R / ed5909b6e89a2db4456e54cd5f673791d7eca6732202bbf2a9cc504fe2f9b84a | allocator-api2, equivalent, foldhash |
| P051 / 465–470 | heck / 0.5.0 | R / 2304e00983f87ffb38b55b444b5e3b60a884b5d30c0fca7d82fe33449bbe55ea | 无 |
| P052 / 471–480 | http / 1.5.0 | R / 918d3568bebf352712bc2ef3d46a8bcf1a75b373be6539de198e9105cbbf9ce0 | bytes, itoa |
| P053 / 481–486 | httparse / 1.10.1 | R / 6dbf3de79e51f3d586ab4cb9d5c3e2c14aa28ed23d180cf89b4df0454a69cc87 | 无 |
| P054 / 487–492 | httpdate / 1.0.3 | R / df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9 | 无 |
| P055 / 493–505 | icu_collections / 2.1.1 | R / 4c6b649701667bbe825c3b7e6388cb521c23d88644678e83c0c4d0a621a34b43 | displaydoc, potential_utf, yoke, zerofrom, zerovec |
| P056 / 506–518 | icu_locale_core / 2.1.1 | R / edba7861004dd3714265b4db54a3c390e880ab658fec5f7db895fae2046b5bb6 | displaydoc, litemap, tinystr, writeable, zerovec |
| P057 / 519–532 | icu_normalizer / 2.1.1 | R / 5f6c8828b67bf8908d82127b2054ea1b4427ff0230ee9141c54251934ab1b599 | icu_collections, icu_normalizer_data, icu_properties, icu_provider, smallvec, zerovec |
| P058 / 533–538 | icu_normalizer_data / 2.1.1 | R / 7aedcccd01fc5fe81e6b489c15b247b8b0690feb23304303a9e560f37efc560a | 无 |
| P059 / 539–552 | icu_properties / 2.1.2 | R / 020bfc02fe870ec3a66d93e677ccca0562506e5872c650f893269e08615d74ec | icu_collections, icu_locale_core, icu_properties_data, icu_provider, zerotrie, zerovec |
| P060 / 553–558 | icu_properties_data / 2.1.2 | R / 616c294cf8d725c6afcd8f55abc17c56464ef6211f9ed59cccffe534129c77af | 无 |
| P061 / 559–573 | icu_provider / 2.1.1 | R / 85962cf0ce02e1e0a629cc34e7ca3e373ce20dda4c4d7294bbd0bf1fdb59e614 | displaydoc, icu_locale_core, writeable, yoke, zerofrom, zerotrie, zerovec |
| P062 / 574–579 | ident_case / 1.0.1 | R / b9e0384b61958566e926dc50660321d12159025e767c18e043daf26b70104c39 | 无 |
| P063 / 580–590 | idna / 1.1.0 | R / 3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de | idna_adapter, smallvec, utf8_iter |
| P064 / 591–600 | idna_adapter / 1.2.1 | R / 3acae9609540aa318d1bc588455225fb2085b9ed0c4f6bd0d9d5bcd86f1a0344 | icu_normalizer, icu_properties |
| P065 / 601–616 | ignore / 0.4.33 | R / 00b69833ed729dc5aa7d19541d96d6cf8e9137194207a04916d658e43168402f | crossbeam-deque, globset, log, memchr, regex-automata, same-file, walkdir, winapi-util |
| P066 / 617–626 | indexmap / 2.14.1 | R / 07aa2048142242915a31d35844fb311e0e53fcca590c3a0a40dcf1b841fa09eb | equivalent, hashbrown 0.17.1 |
| P067 / 627–635 | indoc / 2.0.7 | R / 79cf5c93f93228cf8efb3ba362535fb11199ac548a09ce117c9b1adc3030d706 | rustversion |
| P068 / 636–648 | instability / 0.3.10 | R / 6778b0196eefee7df739db78758e5cf9b37412268bfa5650bfeed028aed20d9c | darling, indoc, proc-macro2, quote, syn 2.0.119 |
| P069 / 649–657 | itertools / 0.14.0 | R / 2b192c782037fadd9cfa75548310488aabdbf3d2da73885b31bd0abd03351285 | either |
| P070 / 658–663 | itoa / 1.0.18 | R / 8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682 | 无 |
| P071 / 664–674 | kasuari / 0.4.12 | R / bde5057d6143cc94e861d90f591b9303d6716c6b9602309150bd068853c10899 | hashbrown 0.16.1, portable-atomic, thiserror |
| P072 / 675–680 | libc / 0.2.189 | R / 3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2 | 无 |
| P073 / 681–686 | libm / 0.2.16 | R / b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981 | 无 |
| P074 / 687–692 | libyaml-rs / 0.3.0 | R / 2e126dda6f34391ab7b444f9922055facc83c07a910da3eb16f1e4d9c45dc777 | 无 |
| P075 / 693–701 | line-clipping / 0.3.8 | R / e752191d037c44ad111a8caa762921926658402f01cc1253f7bef2020ece4f5e | bitflags |
| P076 / 702–707 | linux-raw-sys / 0.12.1 | R / 32a66949e030da00e8c7d4434b251670a91556f4144941d37452769c25d58a53 | 无 |
| P077 / 708–713 | litemap / 0.8.3 | R / 47d9d19d1d6efa0109d2f65ff4c85cddd50bd572e5a00127ab10987290bcefae | 无 |
| P078 / 714–719 | litrs / 1.0.0 | R / 11d3d7f243d5c5a8b9bb5d6dd2b1602c0cb0b9db1621bafc7ed66e35ff9fe092 | 无 |
| P079 / 720–728 | lock_api / 0.4.14 | R / 224399e74b87b5f3557511d98dff8b14089b3dadafcab6bb93eab67d3aace965 | scopeguard |
| P080 / 729–734 | log / 0.4.34 | R / f9f8bd3e56ce4dfc153cf470fffbfa98c7620958b312ca5c3a4b8d5181fd13c6 | 无 |
| P081 / 735–743 | lru / 0.18.4 | R / ff9840bcc50b71349309900da0ce7279aa336ae71d73250b07998932c7d97c25 | hashbrown 0.17.1 |
| P082 / 744–749 | memchr / 2.8.3 | R / cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98 | 无 |
| P083 / 750–759 | miniz_oxide / 0.9.1 | R / b63fbc4a50860e98e7b2aa7804ded1db5cbc3aff9193adaff57a6931bf7c4b4c | adler2, simd-adler32 |
| P084 / 760–771 | mio / 1.2.3 | R / 4b18443e9c262bfe8fa82f51666e2642c53393f7e5c27b3e1aeab922cff5b9d8 | libc, log, wasi, windows-sys 0.61.2 |
| P085 / 772–777 | num-conv / 0.2.2 | R / 521739c6d2bac4aa25192232afe6841231376b2b26d4d9fae5ecf8ca5772e441 | 无 |
| P086 / 778–786 | num-traits / 0.2.19 | R / 071dfc062690e90b734c0b2273ce72ad0ffa95f0c74596bc250dcfd960262841 | autocfg |
| P087 / 787–795 | num_threads / 0.1.7 | R / 5c7398b9c8b70908f6371f47ed36737907c87c52af34c268fed0bf0ceb92ead9 | libc |
| P088 / 796–801 | once_cell / 1.21.4 | R / 9f7c3e4beb33f85d45ae3e3a1792185706c8e16d043238c593331cc7cd313b50 | 无 |
| P089 / 802–813 | palette / 0.7.7 | R / ddeed8580d347d2abf3dcf06a5f0b3dc020258338526b277847cd4248a70fc64 | approx, libm, palette_derive, palette_math |
| P090 / 814–825 | palette_derive / 0.7.7 | R / 88537020289b719d81be994ccf1bbf4990f477e2f69ee52fe3e45f43a02e56be | by_address, proc-macro2, quote, syn 2.0.119 |
| P091 / 826–834 | palette_math / 0.7.7 | R / 6e6eb142958d64335fb0e345c5b9ead2ecd6fc438c307e9d7d3c4fd428dbaf12 | libm |
| P092 / 835–844 | parking_lot / 0.12.5 | R / 93857453250e3077bd71ff98b6a65ea6621a19bb0f559a85248955ac12c45a1a | lock_api, parking_lot_core |
| P093 / 845–857 | parking_lot_core / 0.9.12 | R / 2621685985a2ebf1c516881c026032ac7deafcda1a2c9b7850dc81e3dfcb64c1 | cfg-if, libc, redox_syscall, smallvec, windows-link |
| P094 / 858–863 | percent-encoding / 2.3.2 | R / 9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220 | 无 |
| P095 / 864–869 | portable-atomic / 1.15.0 | R / 05c8b63e8d9609db387f0324918f81d68fe27748f084ef092fb35954d0539a85 | 无 |
| P096 / 870–878 | potential_utf / 0.1.6 | R / d83eb9bc6d8e5cf568e7a1101d60ee05e81ed50ea106026f3d18deeb046d7661 | zerovec |
| P097 / 879–884 | powerfmt / 0.2.0 | R / 439ee305def115ba05938db6eb1644ff94165c5ab5e9420d1c1bcedbba909391 | 无 |
| P098 / 885–893 | proc-macro2 / 1.0.107 | R / 985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9 | unicode-ident |
| P099 / 894–902 | quote / 1.0.47 | R / 1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001 | proc-macro2 |
| P100 / 903–908 | r-efi / 6.0.0 | R / f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf | 无 |
| P101 / 909–921 | ratatui / 0.30.2 | R / 3274ba0a2c5e1bcad2a2005d20f4dc59dad26b2eb0940fb094500dba4099d57d | instability, ratatui-core, ratatui-crossterm, ratatui-widgets, serde |
| P102 / 922–942 | ratatui-core / 0.1.2 | R / cbb175c433c8e28a809d1f5773a2ae96e68c0ce40db865cbab1020bf33ae479c | bitflags, compact_str, hashbrown 0.17.1, itertools, kasuari, lru, palette, serde, strum, thiserror, unicode-segmentation, unicode-truncate, unicode-width |
| P103 / 943–954 | ratatui-crossterm / 0.1.2 | R / 567584a3b0e6a8203c23de40b4861497266725eb5363dbfd18a1edd603cca9f0 | cfg-if, crossterm, instability, ratatui-core |
| P104 / 955–974 | ratatui-widgets / 0.3.2 | R / 66e3d19bcc9130ca376277d93b60767ff121ace3be06f5f95f81dd68956407d1 | bitflags, hashbrown 0.17.1, indoc, instability, itertools, line-clipping, ratatui-core, serde, strum, time, unicode-segmentation, unicode-width |
| P105 / 975–983 | redox_syscall / 0.5.18 | R / ed2bf2547551a7053d6fdfafda3f938979645c44812fbfcda098faae3f1a362d | bitflags |
| P106 / 984–994 | regex-automata / 0.4.18 | R / ad8553b9b26413251cbf30e620595c7a41b3887f03da04579c0e6b0d6a06b4b2 | aho-corasick, memchr, regex-syntax |
| P107 / 995–1000 | regex-syntax / 0.8.11 | R / d6f6ff9a378485b298a5286656da665ba74413d36db0979633275d2e708145d4 | 无 |
| P108 / 1001–1014 | ring / 0.17.14 | R / a4689e6c2294d81e88dc6261c768b63bc4fcdb852be6d1352498b114f61383b7 | cc, cfg-if, getrandom 0.2.17, libc, untrusted, windows-sys 0.52.0 |
| P109 / 1015–1023 | rustc_version / 0.4.1 | R / cfcb3a22ef46e85b45de6ee7e79d063319ebb6594faafcf1c225ea92ab6e9b92 | semver |
| P110 / 1024–1036 | rustix / 1.1.4 | R / b6fe4565b9518b83ef4f91bb47ce29620ca828bd32cb7e408f0062e9930ba190 | bitflags, errno, libc, linux-raw-sys, windows-sys 0.61.2 |
| P111 / 1037–1051 | rustls / 0.23.43 | R / 0283386ce02abc0151e1761d08802dfe86c173b0b494af5cbc086574e453da06 | log, once_cell, ring, rustls-pki-types, rustls-webpki, subtle, zeroize |
| P112 / 1052–1060 | rustls-pki-types / 1.15.1 | R / 2f4925028c7eb5d1fcdaf196971378ed9d2c1c4efc7dc5d011256f76c99c0a96 | zeroize |
| P113 / 1061–1071 | rustls-webpki / 0.103.15 | R / f3c3cf1d8b1e7d4927e2d154c3fcb02979afb9939629c62cd9048d4f07b60ac2 | ring, rustls-pki-types, untrusted |
| P114 / 1072–1077 | rustversion / 1.0.23 | R / cf54715a573b99ac80df0bc206da022bcd442c974952c7b9720069370852e21f | 无 |
| P115 / 1078–1083 | ryu / 1.0.23 | R / 9774ba4a74de5f7b1c1451ed6cd5285a32eddb5cccb8cc655a4e50009e06477f | 无 |
| P116 / 1084–1092 | same-file / 1.0.6 | R / 93fc1dc3aaa9bfed95e02e6eadabb4baf7e3078b0bd1b4d7b6b0b68378900502 | winapi-util |
| P117 / 1093–1098 | scopeguard / 1.2.0 | R / 94143f37725109f92c262ed2cf5e59bce7498c01bcc1502d7b9afe439a4e9f49 | 无 |
| P118 / 1099–1104 | semver / 1.0.28 | R / 8a7852d02fc848982e0c167ef163aaff9cd91dc640ba85e263cb1ce46fae51cd | 无 |
| P119 / 1105–1114 | serde / 1.0.229 | R / 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba | serde_core, serde_derive |
| P120 / 1115–1123 | serde_core / 1.0.229 | R / 67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48 | serde_derive |
| P121 / 1124–1134 | serde_derive / 1.0.229 | R / e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348 | proc-macro2, quote, syn 3.0.4 |
| P122 / 1135–1147 | serde_json / 1.0.151 | R / c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14 | itoa, memchr, serde, serde_core, zmij |
| P123 / 1148–1156 | serde_spanned / 1.1.1 | R / 6662b5879511e06e8999a8a235d848113e942c9124f211511b16466ee2995f26 | serde_core |
| P124 / 1157–1167 | sha2 / 0.10.9 | R / a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283 | cfg-if, cpufeatures, digest |
| P125 / 1168–1173 | shlex / 2.0.1 | R / f8fadd59c855ef2080decdef8ff161eb6661b86933c9d82e5ba29dc602a55aba | 无 |
| P126 / 1174–1183 | signal-hook / 0.3.18 | R / d881a16cf4426aa584979d30bd82cb33429027e42122b169753d6ef1085ed6e2 | libc, signal-hook-registry |
| P127 / 1184–1194 | signal-hook-mio / 0.2.5 | R / b75a19a7a740b25bc7944bdee6172368f988763b744e3d4dfe753f6b4ece40cc | libc, mio, signal-hook |
| P128 / 1195–1204 | signal-hook-registry / 1.4.8 | R / c4db69cba1110affc0e9f7bcd48bbf87b3f4fc7c61fc9155afd4c469eb3d6c1b | errno, libc |
| P129 / 1205–1210 | simd-adler32 / 0.3.10 | R / 3a219298ac11a56ea9a6d2120044824d6f01aeb034955e7af7bc16858527deea | 无 |
| P130 / 1211–1216 | smallvec / 1.16.0 | R / b9be42f50aa861c555654aa3a37f52f4b1074bacf4e48fe0ef7fa584e80f1f0f | 无 |
| P131 / 1217–1222 | stable_deref_trait / 1.2.1 | R / 6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596 | 无 |
| P132 / 1223–1228 | static_assertions / 1.1.0 | R / a2eb9349b6444b326872e140eb1cf5e7c522154d69e7a0ffb0fb81c06b37543f | 无 |
| P133 / 1229–1234 | strsim / 0.11.1 | R / 7da8b5736845d9f2fcb837ea5d9e2628564b3b043a70948a3f0b778838c5fb4f | 无 |
| P134 / 1235–1243 | strum / 0.28.0 | R / 9628de9b8791db39ceda2b119bbe13134770b56c138ec1d3af810d045c04f9bd | strum_macros |
| P135 / 1244–1255 | strum_macros / 0.28.0 | R / ab85eea0270ee17587ed4156089e10b9e6880ee688791d45a905f5b1ca36f664 | heck, proc-macro2, quote, syn 2.0.119 |
| P136 / 1256–1261 | subtle / 2.6.1 | R / 13c2bddecc57b384dee18652358fb23172facb8a2c51ccc10d74c157bdea3292 | 无 |
| P137 / 1262–1272 | syn / 2.0.119 | R / 872831b642d1a07999a962a351ed35b955ea2cfc8f3862091e2a240a84f17297 | proc-macro2, quote, unicode-ident |
| P138 / 1273–1283 | syn / 3.0.4 | R / e6275cddf4610d1775e6d1fe9469b2e77d0f39fd98fb7450901b821e0c53649f | proc-macro2, quote, unicode-ident |
| P139 / 1284–1294 | synstructure / 0.13.2 | R / 728a70f3dbaf5bab7f0c4b1ac8d7ae5ea60a4b5549c8a5914361c99147a709d2 | proc-macro2, quote, syn 2.0.119 |
| P140 / 1295–1307 | tempfile / 3.27.0 | R / 32497e9a4c7b38532efcdebeef879707aa9f794296a4f0244f6f69e9bc8574bd | fastrand, getrandom 0.4.3, once_cell, rustix, windows-sys 0.61.2 |
| P141 / 1308–1316 | thiserror / 2.0.20 | R / ec86235f5fcc2a73650310756d2ac5b138a5780bbbdfae3eeccec992c435ba4f | thiserror-impl |
| P142 / 1317–1327 | thiserror-impl / 2.0.20 | R / bc04cd3e1236dd4a98afca4569f2deb3f120e5422a4023be2cb683f8486292af | proc-macro2, quote, syn 3.0.4 |
| P143 / 1328–1343 | time / 0.3.55 | R / cdb87b95ec50ddfa440816d227a17b2ccbdda963a316a727fda0fc4334f7d134 | deranged, libc, num-conv, num_threads, powerfmt, serde_core, time-core, time-macros |
| P144 / 1344–1349 | time-core / 0.1.9 | R / 9e1c906769ad99c88eaa54e728060edef082f8e358ff32030cb7c7d315e81109 | 无 |
| P145 / 1350–1359 | time-macros / 0.2.32 | R / 7e689342a48d2ea927c87ea50cabf8594854bf940e9310208848d680d668ed85 | num-conv, time-core |
| P146 / 1360–1369 | tinystr / 0.8.4 | R / b1e27c91459209c2986af3dcf603a5a74a4368754ce37414f59acc971167f643 | displaydoc, zerovec |
| P147 / 1370–1384 | toml / 0.9.12+spec-1.1.0 | R / cf92845e79fc2e2def6a5d828f0801e29a2f8acc037becc5ab08595c7d5e9863 | indexmap, serde_core, serde_spanned, toml_datetime, toml_parser, toml_writer, winnow 0.7.15 |
| P148 / 1385–1393 | toml_datetime / 0.7.5+spec-1.1.0 | R / 92e1cfed4a3038bc5a127e35a2d360f145e1f4b971b551a2ba5fd7aedf7e1347 | serde_core |
| P149 / 1394–1402 | toml_parser / 1.1.3+spec-1.1.0 | R / 1d38ac1cf9b95face32296c0a3ede1fdc270627c9d9c02a7274dd6d960dc4d56 | winnow 1.0.4 |
| P150 / 1403–1408 | toml_writer / 1.1.2+spec-1.1.0 | R / 7d56353a2a665ad0f41a421187180aab746c8c325620617ad883a99a1cbe66d2 | 无 |
| P151 / 1409–1414 | typenum / 1.20.1 | R / b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20 | 无 |
| P152 / 1415–1420 | unicode-ident / 1.0.24 | R / e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75 | 无 |
| P153 / 1421–1426 | unicode-segmentation / 1.13.3 | R / c6f5d3c3b1bf09027a88a6bc961fc00497d651009560b5463668dc81b0fa87a8 | 无 |
| P154 / 1427–1437 | unicode-truncate / 2.0.1 | R / 16b380a1238663e5f8a691f9039c73e1cdae598a30e9855f541d29b08b53e9a5 | itertools, unicode-segmentation, unicode-width |
| P155 / 1438–1443 | unicode-width / 0.2.2 | R / b4ac048d71ede7ee76d585517add45da530660ef4390e49b098733c6e897f254 | 无 |
| P156 / 1444–1449 | untrusted / 0.9.0 | R / 8ecb6da28b8a351d773b68d5825ac39017e680750f980f3a1a85cd8dd28a47c1 | 无 |
| P157 / 1450–1469 | ureq / 3.4.0 | R / 972d7902c8735f2695410b8aed7df6ed12a47394aa1c8d7af49f0497b731a94d | base64, cookie_store, flate2, log, percent-encoding, rustls, rustls-pki-types, serde, serde_json, ureq-proto, utf8-zero, webpki-roots |
| P158 / 1470–1481 | ureq-proto / 0.6.1 | R / da5f78b09e6941e1a0f2e30e695e4b120377b54d5e0aec11b594bb57b3971613 | base64, http, httparse, log |
| P159 / 1482–1493 | url / 2.5.8 | R / ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed | form_urlencoded, idna, percent-encoding, serde |
| P160 / 1494–1499 | utf8-zero / 0.8.1 | R / b8c0a043c9540bae7c578c88f91dda8bd82e59ae27c21baca69c8b191aaf5a6e | 无 |
| P161 / 1500–1505 | utf8_iter / 1.0.4 | R / b6c140620e7ffbb22c2dee59cafe6084a59b5ffc27a8859a5f0d494b5d52b6be | 无 |
| P162 / 1506–1511 | version_check / 0.9.5 | R / 0b928f33d975fc6ad9f86c8f283853ad26bdd5b10b7f1542aa2fa15e2289105a | 无 |
| P163 / 1512–1521 | walkdir / 2.5.0 | R / 29790946404f91d9c5d06f9874efddea1dc06c5efe94541a7d6863108e3a5e4b | same-file, winapi-util |
| P164 / 1522–1527 | wasi / 0.11.1+wasi-snapshot-preview1 | R / ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b | 无 |
| P165 / 1528–1536 | webpki-roots / 1.0.9 | R / 7dcd9d09a39985f5344844e66b0c530a33843579125f23e21e9f0f220850f22a | rustls-pki-types |
| P166 / 1537–1546 | winapi / 0.3.9 | R / 5c839a674fcd7a98952e593242ea400abe93992746761e38641405d28b00f419 | winapi-i686-pc-windows-gnu, winapi-x86_64-pc-windows-gnu |
| P167 / 1547–1552 | winapi-i686-pc-windows-gnu / 0.4.0 | R / ac3b87c63620426dd9b991e5ce0329eff545bccbbb34f3be09ff6fb6ab51b7b6 | 无 |
| P168 / 1553–1561 | winapi-util / 0.1.11 | R / c2a7b1c03c876122aa43f3020e6c3c3ee5c05081c9a00739faf7503aeba10d22 | windows-sys 0.61.2 |
| P169 / 1562–1567 | winapi-x86_64-pc-windows-gnu / 0.4.0 | R / 712e227841d057c1ee1cd2fb22fa7e5a5461ae8e48fa2ca79ec42cfc1931183f | 无 |
| P170 / 1568–1573 | windows-link / 0.2.1 | R / f0805222e57f7521d6a62e36fa9163bc891acd422f971defe97d64e70d0a4fe5 | 无 |
| P171 / 1574–1582 | windows-sys / 0.52.0 | R / 282be5f36a8ce781fad8c8ae18fa3f9beff57ec1b52cb3de0789201425d9a33d | windows-targets |
| P172 / 1583–1591 | windows-sys / 0.61.2 | R / ae137229bcbd6cdf0f7b80a31df61766145077ddf49416a728b02cb3921ff3fc | windows-link |
| P173 / 1592–1607 | windows-targets / 0.52.6 | R / 9b724f72796e036ab90c1021d4780d4d3d648aca59e491e6b98e725b84e99973 | windows_aarch64_gnullvm, windows_aarch64_msvc, windows_i686_gnu, windows_i686_gnullvm, windows_i686_msvc, windows_x86_64_gnu, windows_x86_64_gnullvm, windows_x86_64_msvc |
| P174 / 1608–1613 | windows_aarch64_gnullvm / 0.52.6 | R / 32a4622180e7a0ec044bb555404c800bc9fd9ec262ec147edd5989ccd0c02cd3 | 无 |
| P175 / 1614–1619 | windows_aarch64_msvc / 0.52.6 | R / 09ec2a7bb152e2252b53fa7803150007879548bc709c039df7627cabbd05d469 | 无 |
| P176 / 1620–1625 | windows_i686_gnu / 0.52.6 | R / 8e9b5ad5ab802e97eb8e295ac6720e509ee4c243f69d781394014ebfe8bbfa0b | 无 |
| P177 / 1626–1631 | windows_i686_gnullvm / 0.52.6 | R / 0eee52d38c090b3caa76c563b86c3a4bd71ef1a819287c19d586d7334ae8ed66 | 无 |
| P178 / 1632–1637 | windows_i686_msvc / 0.52.6 | R / 240948bc05c5e7c6dabba28bf89d89ffce3e303022809e73deaefe4f6ec56c66 | 无 |
| P179 / 1638–1643 | windows_x86_64_gnu / 0.52.6 | R / 147a5c80aabfbf0c7d901cb5895d1de30ef2907eb21fbbab29ca94c5b08b1a78 | 无 |
| P180 / 1644–1649 | windows_x86_64_gnullvm / 0.52.6 | R / 24d5b23dc417412679681396f2b49f3de8c1473deb516bd34410872eff51ed0d | 无 |
| P181 / 1650–1655 | windows_x86_64_msvc / 0.52.6 | R / 589f6da84c646204747d1270a2a5661ea66ed1cced2631d546fdfb155959f9ec | 无 |
| P182 / 1656–1661 | winnow / 0.7.15 | R / df79d97927682d2fd8adb29682d1140b343be4ac0f08fd68b7765d9c059d3945 | 无 |
| P183 / 1662–1667 | winnow / 1.0.4 | R / 23b97319f7b8343df12cc98938e5c3eb436064524c8d2b4e30a1d3a36eecdf81 | 无 |
| P184 / 1668–1673 | writeable / 0.6.4 | R / 3ad82d2a33cdc9674dc7465672f271e096168fcdbe0f799d9e6db8c5892679dc | 无 |
| P185 / 1674–1686 | yaml_serde / 0.10.7 | R / 33b729a08a9a6be689bbad3e2bf8015926db54b6622cc89c3a5f7dc174b9e918 | indexmap, itoa, libyaml-rs, ryu, serde |
| P186 / 1687–1697 | yoke / 0.8.3 | R / 709fe23a0424b6a435d82152b1bd3fdfb0833487d5fa90d05d42762a9891fef5 | stable_deref_trait, yoke-derive, zerofrom |
| P187 / 1698–1709 | yoke-derive / 0.8.2 | R / de844c262c8848816172cef550288e7dc6c7b7814b4ee56b3e1553f275f1858e | proc-macro2, quote, syn 2.0.119, synstructure |
| P188 / 1710–1731 | zenpi / 0.1.0 | L / 无 | base64, crossterm, httpdate, ignore, libc, ratatui, serde, serde_json, sha2, tempfile, thiserror, toml, unicode-segmentation, unicode-width, ureq, yaml_serde |
| P189 / 1732–1740 | zerofrom / 0.1.8 | R / 0ec05a11813ea801ff6d75110ad09cd0824ddba17dfe17128ea0d5f68e6c5272 | zerofrom-derive |
| P190 / 1741–1752 | zerofrom-derive / 0.1.7 | R / 11532158c46691caf0f2593ea8358fed6bbf68a0315e80aae9bd41fbade684a1 | proc-macro2, quote, syn 2.0.119, synstructure |
| P191 / 1753–1758 | zeroize / 1.9.0 | R / e13c156562582aa81c60cb29407084cdb54c4164760106ab78e6c5b0858cf64e | 无 |
| P192 / 1759–1769 | zerotrie / 0.2.5 | R / 4ea269c3bd32f0a32c321907a2ae912ba6f4649bb0fc764a15627e99a7095a3f | displaydoc, yoke, zerofrom |
| P193 / 1770–1780 | zerovec / 0.11.8 | R / bb0464e17806c1d976d5cba29399c7f08e516e279e2ba493f63123b5fca67dd8 | yoke, zerofrom, zerovec-derive |
| P194 / 1781–1791 | zerovec-derive / 0.11.6 | R / 34df6fc39dbd26ddc9c10e6a2984476e13acce22e64e4487636ef494369225da | proc-macro2, quote, syn 3.0.4 |
| P195 / 1792–1797 | zlib-rs / 0.6.7 | R / 34b31d188d9d685a4f9c7b46d6e36631b07058d2cfe190267adce54dc230bf12 | 无 |
| P196 / 1798–1802 | zmij / 1.0.23 | R / 29666d0abbfad1e3dc4dcf6144730dd3a3ab225bbbdac83319345b1b44ccfc1b | 无 |

当前全部 196 个包已映射；checksum 仅做离线字段绑定，未与 crate 档案比对。

冻结前原捕获与当前源身份比较见freeze-origin-identities.json；source不变。
`capture/zenpi/Docs/execution/active_requirement.json`发生变化，另存`freeze-drift/capture/zenpi/Docs/execution/active_requirement.json`，9853B/1L/7d6bf98fe20e80c939020a4891efb2fe73d6e8db64dbe670c96469f40266fb21，不增加全文credit。

`capture/zenpi/Docs/learn/stage1_pi_mono/targets/zenpi/file_learn_index.tsv`发生变化，另存`freeze-drift/capture/zenpi/Docs/learn/stage1_pi_mono/targets/zenpi/file_learn_index.tsv`，4553B/28L/b8ac2b2e216c643e6cd2237a419a3068defbb1660ffde8f91e29756098795488，不增加全文credit。

`capture/zenpi/Docs/learn/stage1_pi_mono/targets/zenpi/source_manifest.tsv`发生变化，另存`freeze-drift/capture/zenpi/Docs/learn/stage1_pi_mono/targets/zenpi/source_manifest.tsv`，7862B/28L/1693f341317741369b4012dec632fa2320960686822f84a9af752f8dee515aae，不增加全文credit。

`capture/zenpi/Docs/stage_1_v3_pi_mono_blueprint.md`发生变化，另存`freeze-drift/capture/zenpi/Docs/stage_1_v3_pi_mono_blueprint.md`，153896B/632L/3c090412edc68e1df4c53d4be809c89438e3dd6ad511f3c0b571910a6c5baa5b，不增加全文credit。
