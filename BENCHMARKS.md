# 压缩 / 加密性能基准

`codec` 模块（压缩 `gzip`/`zstd` + 加密 `AES-256-GCM`/`SM4-GCM`）的性能基准与结论。

## 运行方式

```bash
cargo bench --bench codec            # 完整（100 samples）
cargo bench --bench codec -- --quick  # 快速
```

- 基准文件：`benches/codec.rs`
- 指标：吞吐（`Throughput::Bytes`，criterion 直接报 MiB/s / GiB/s）与压缩比（基准启动时打印）。
- 载荷：1 MiB 日志风格文本（高可压缩）；另附 1 MiB 高熵数据看最坏情况。

## 测试环境

- Apple Silicon（aarch64-apple-darwin），macOS
- 单次 `encode` 的输入为 1 MiB（= 16 × 64 KiB 压缩块，`finish` 尾块为空）
- 数字为**单机单核**吞吐，仅供参考量级；绝对数随 CPU/数据形态波动

## 数据

### 压缩比（1 MiB 日志文本）

| 算法 | 压缩后 | 压缩比 |
| --- | --- | --- |
| gzip-1 | 78,670 B | 7.5% |
| gzip-3 | 68,818 B | 6.6% |
| gzip-6 | 65,157 B | 6.2% |
| gzip-9 | 65,155 B | 6.2% |
| zstd-1 | 42,179 B | 4.0% |
| zstd-3 | 40,153 B | 3.8% |
| zstd-6 | 40,159 B | 3.8% |
| zstd-9 | 37,794 B | 3.6% |
| gzip-6（随机） | 1,049,232 B | 100.1%（不压缩） |
| zstd-3（随机） | 1,048,864 B | 100.0%（不压缩） |

### 压缩吞吐（encode，1 MiB）

| 算法 | 吞吐 |
| --- | --- |
| gzip-1 | ~1.09 GiB/s |
| gzip-3 | ~500 MiB/s |
| gzip-6 | ~255 MiB/s |
| zstd-1 | ~1.25 GiB/s |
| zstd-3 | ~1.34 GiB/s |
| zstd-6 | ~419 MiB/s |

### 加密吞吐（encode，1 MiB）

| 算法 | 吞吐 |
| --- | --- |
| aes-256-gcm | ~5.27 GiB/s |
| sm4-gcm | ~74.7 MiB/s |

### 组合（先压缩后加密）+ 解码

| 组合 | 吞吐 |
| --- | --- |
| zstd-3 + aes-256-gcm | ~1.32 GiB/s |
| zstd-3 + sm4-gcm | ~810 MiB/s |
| 解码（zstd-3 + aes-256-gcm） | ~2.82 GiB/s |

## 结论

1. **SM4-GCM 比 AES-256-GCM 慢约 70 倍（~74.7 MiB/s vs ~5.27 GiB/s）**。
   根因：AES-GCM 走 ring 的 AES-NI / ARM 密码学硬件加速，而 SM4 是纯软件实现（`sm4-gcm` crate
   无硬件加速，GCM 的 GHASH 也在软件里算）。**高吞吐日志链路用 SM4 会是明显瓶颈**（单核 ~75 MiB/s
   上限），选型时需重点权衡「国密合规」与「吞吐」。
2. **zstd 全面优于 gzip**：同压缩率下吞吐更高（zstd-3 比 gzip-3 快 ~2.7× 且体积更小）；
   zstd-1 的吞吐甚至高于 gzip-1，同时体积小一半。默认 `zstd-3` 是合理甜点位。
3. **压缩对日志类数据收益巨大**：典型日志文本可压到 3.8%（zstd-3）；但高熵数据压缩比 ≈100%，
   白付 CPU（可考虑对不可压缩数据跳过压缩，但需额外探测逻辑）。
4. **AES 加密本身不是瓶颈**：组合链路的瓶颈在压缩（zstd-3 ~1.34 GiB/s）而非 AES 加密
   （~5.27 GiB/s）；但若换 SM4，瓶颈立即变成加密。
5. **帧开销**：每个加密块固定开销 = 帧头 8 B + nonce 12 B + GCM tag 16 B = **36 B/块**；
   压缩每块 64 KiB。小消息场景该固定开销占比会上升。
