//! 压缩 / 加密 codec 的性能基准。
//!
//! 运行：`cargo bench --bench codec`
//! 快速：`cargo bench --bench codec -- --quick`
//!
//! 指标说明：
//! - 吞吐：criterion 的 `Throughput::Bytes` 会直接报出 MiB/s / GiB/s。
//! - 压缩比：基准启动时先打印一次（`print_ratios`），供对照。

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use wp_connector_utils::codec::{
    Cipher, CompressConfig, CompressionAlgo, EncryptConfig, build_decoder, build_encoder,
};

/// 单次基准的数据规模（1 MiB，为 64 KiB 压缩块的整数倍，`finish` 尾块为空）。
const PAYLOAD_BYTES: usize = 1024 * 1024;

fn aes_key() -> Vec<u8> {
    (0u8..32).collect()
}

fn sm4_key() -> Vec<u8> {
    (0u8..16).collect()
}

fn encrypt_config(cipher: Cipher) -> EncryptConfig {
    EncryptConfig {
        cipher,
        key: match cipher {
            Cipher::Aes256Gcm => aes_key(),
            Cipher::Sm4Gcm => sm4_key(),
        },
    }
}

fn compress_config(algo: CompressionAlgo, level: i32) -> CompressConfig {
    CompressConfig { algo, level }
}

/// 生成日志风格文本（高度可压缩，贴近真实日志链路）。
fn log_text(bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes);
    let mut i = 0u64;
    while out.len() < bytes {
        let line = format!(
            "2026-09-25T12:00:00.000Z INFO worker-{:02} task=t{:02} status=ok latency_ms={:03} req_id={:08} region=cn-north-1 trace_id={:016x}\n",
            i % 16,
            i % 8,
            i % 100,
            i,
            i
        );
        out.extend_from_slice(line.as_bytes());
        i += 1;
    }
    out.truncate(bytes);
    out
}

/// 生成高熵（不可压缩）数据，用于看压缩的最坏情况。
fn random_text(bytes: usize) -> Vec<u8> {
    let mut out = vec![0u8; bytes];
    // 确定性伪随机（LCG），避免 `rand` 依赖。
    let mut x: u64 = 0x9e3779b97f4a7c15;
    for chunk in out.chunks_mut(8) {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let b = x.to_le_bytes();
        let n = chunk.len().min(8);
        chunk[..n].copy_from_slice(&b[..n]);
    }
    out
}

fn run_encode(
    data: &[u8],
    compression: Option<&CompressConfig>,
    encryption: Option<&EncryptConfig>,
) -> usize {
    let mut encoder = build_encoder(compression, encryption).unwrap();
    let mut out = Vec::new();
    encoder.encode(data, &mut out).unwrap();
    let mut tail = Vec::new();
    encoder.finish(&mut tail).unwrap();
    out.extend_from_slice(&tail);
    out.len()
}

fn run_decode(
    data: &[u8],
    compression: Option<&CompressConfig>,
    encryption: Option<&EncryptConfig>,
) -> usize {
    let mut decoder = build_decoder(compression, encryption).unwrap();
    let mut out = Vec::new();
    decoder.decode(data, &mut out).unwrap();
    let mut tail = Vec::new();
    decoder.finish(&mut tail).unwrap();
    out.extend_from_slice(&tail);
    out.len()
}

/// 打印压缩比（启动时一次）。
fn print_ratios(data: &[u8]) {
    println!("=== compression ratio @ {} bytes log text ===", data.len());
    for (name, algo, level) in [
        ("gzip-1", CompressionAlgo::Gzip, 1),
        ("gzip-3", CompressionAlgo::Gzip, 3),
        ("gzip-6", CompressionAlgo::Gzip, 6),
        ("gzip-9", CompressionAlgo::Gzip, 9),
        ("zstd-1", CompressionAlgo::Zstd, 1),
        ("zstd-3", CompressionAlgo::Zstd, 3),
        ("zstd-6", CompressionAlgo::Zstd, 6),
        ("zstd-9", CompressionAlgo::Zstd, 9),
    ] {
        let cfg = compress_config(algo, level);
        let compressed = run_encode(data, Some(&cfg), None);
        let ratio = compressed as f64 / data.len() as f64;
        println!(
            "  {name:8} {} -> {} bytes  ratio={:.4}  ({:.1}% of original)",
            data.len(),
            compressed,
            ratio,
            ratio * 100.0
        );
    }
    // 高熵数据最坏情况
    let random = random_text(data.len());
    for (name, algo, level) in [
        ("gzip-6/rand", CompressionAlgo::Gzip, 6),
        ("zstd-3/rand", CompressionAlgo::Zstd, 3),
    ] {
        let cfg = compress_config(algo, level);
        let compressed = run_encode(&random, Some(&cfg), None);
        let ratio = compressed as f64 / random.len() as f64;
        println!(
            "  {name:8} {} -> {} bytes  ratio={:.4}  ({:.1}% of original)",
            random.len(),
            compressed,
            ratio,
            ratio * 100.0
        );
    }
}

fn bench_compression(c: &mut Criterion, data: &[u8]) {
    let mut group = c.benchmark_group("compression");
    group.throughput(Throughput::Bytes(data.len() as u64));
    for (name, algo, level) in [
        ("gzip-1", CompressionAlgo::Gzip, 1),
        ("gzip-3", CompressionAlgo::Gzip, 3),
        ("gzip-6", CompressionAlgo::Gzip, 6),
        ("zstd-1", CompressionAlgo::Zstd, 1),
        ("zstd-3", CompressionAlgo::Zstd, 3),
        ("zstd-6", CompressionAlgo::Zstd, 6),
    ] {
        let cfg = compress_config(algo, level);
        group.bench_function(BenchmarkId::new("encode", name), |b| {
            b.iter(|| run_encode(data, Some(&cfg), None));
        });
    }
    group.finish();
}

fn bench_encryption(c: &mut Criterion, data: &[u8]) {
    let mut group = c.benchmark_group("encryption");
    group.throughput(Throughput::Bytes(data.len() as u64));
    for (name, cipher) in [
        ("aes-256-gcm", Cipher::Aes256Gcm),
        ("sm4-gcm", Cipher::Sm4Gcm),
    ] {
        let cfg = encrypt_config(cipher);
        group.bench_function(BenchmarkId::new("encode", name), |b| {
            b.iter(|| run_encode(data, None, Some(&cfg)));
        });
    }
    group.finish();
}

fn bench_combined(c: &mut Criterion, data: &[u8]) {
    let mut group = c.benchmark_group("compress+encrypt");
    group.throughput(Throughput::Bytes(data.len() as u64));
    for (name, cipher) in [
        ("zstd3+aes-256-gcm", Cipher::Aes256Gcm),
        ("zstd3+sm4-gcm", Cipher::Sm4Gcm),
    ] {
        let comp = compress_config(CompressionAlgo::Zstd, 3);
        let enc = encrypt_config(cipher);
        group.bench_function(BenchmarkId::new("encode", name), |b| {
            b.iter(|| run_encode(data, Some(&comp), Some(&enc)));
        });
    }
    group.finish();
}

fn bench_decode(c: &mut Criterion, data: &[u8]) {
    // 先编码一次，得到密文作为解码输入。
    let comp = compress_config(CompressionAlgo::Zstd, 3);
    let enc = encrypt_config(Cipher::Aes256Gcm);
    let mut encoder = build_encoder(Some(&comp), Some(&enc)).unwrap();
    let mut wire = Vec::new();
    encoder.encode(data, &mut wire).unwrap();
    let mut tail = Vec::new();
    encoder.finish(&mut tail).unwrap();
    wire.extend_from_slice(&tail);

    let mut group = c.benchmark_group("decode");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("zstd3+aes-256-gcm", |b| {
        b.iter(|| run_decode(&wire, Some(&comp), Some(&enc)));
    });
    group.finish();
}

fn codec_benches(c: &mut Criterion) {
    let data = log_text(PAYLOAD_BYTES);
    print_ratios(&data);
    bench_compression(c, &data);
    bench_encryption(c, &data);
    bench_combined(c, &data);
    bench_decode(c, &data);
}

criterion_group!(benches, codec_benches);
criterion_main!(benches);
