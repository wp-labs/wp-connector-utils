//! 压缩 / 加密编解码（字节级、同步、无 tokio 依赖）。
//!
//! 供 sink 侧（压缩 + 加密）与 source 侧（解压 + 解密）复用：
//! - 压缩：`gzip` / `zstd`（按块流式，块阈值 64 KiB）。
//! - 加密：`AES-256-GCM` / `SM4-GCM`（国密，每块独立随机 nonce，输出 `nonce(12) || ciphertext+tag`）。
//! - 组合：**先压缩后加密**（密文是伪随机、不可压缩，压缩放加密前）。
//!
//! 线格式（自描述帧）：每个块都是
//! `magic(2 "WP") | version(1) | kind(1) | length(4 大端) | payload` ——
//! `kind` 标识内容（gzip/zstd/aes-256-gcm/sm4-gcm），拿到字节流即可判断「是否密文、用了哪个算法」。
//! `Decoder::decode` 假定输入是 0 或多个**完整**块；源端读满一个块再喂给 decoder。
//!
//! 可观测性：`Encoder::stats()` 返回 `CodecStats`（`nonces_issued` / `sealed_bytes`），
//! `nonces_issued > 0` 即证明数据确实经过了加密层。
//!
//! ```text
//! encode: 明文 → [compress 分块] → [encrypt 每块] → 自描述帧流
//! decode: 帧流 → [decrypt] → [decompress] → 明文
//! ```

use std::fmt;
use std::io::{Read, Write};

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CodecError {
    Config(String),
    Compress(String),
    Encrypt(String),
    Decode(String),
    Io(std::io::Error),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Config(m) => write!(f, "codec config error: {m}"),
            CodecError::Compress(m) => write!(f, "compress error: {m}"),
            CodecError::Encrypt(m) => write!(f, "encrypt error: {m}"),
            CodecError::Decode(m) => write!(f, "decode error: {m}"),
            CodecError::Io(e) => write!(f, "codec io error: {e}"),
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CodecError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CodecError {
    fn from(e: std::io::Error) -> Self {
        CodecError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgo {
    Gzip,
    Zstd,
}

#[derive(Debug, Clone)]
pub struct CompressConfig {
    pub algo: CompressionAlgo,
    /// 算法相关压缩级别：gzip `0..=9`（0=不压缩）；zstd `0..=22`（0=默认）。
    pub level: i32,
}

impl Default for CompressConfig {
    fn default() -> Self {
        Self {
            algo: CompressionAlgo::Zstd,
            level: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cipher {
    Aes256Gcm,
    /// 国密 SM4（128-bit 分组）的 GCM 模式。
    Sm4Gcm,
}

#[derive(Debug, Clone)]
pub struct EncryptConfig {
    pub cipher: Cipher,
    /// 对称密钥：`Aes256Gcm` 需 32 字节；`Sm4Gcm` 需 16 字节。
    pub key: Vec<u8>,
}

// ---------------------------------------------------------------------------
// 编解码器 trait
// ---------------------------------------------------------------------------

/// 编解码统计：用于观测「加密/压缩确实发生」。`nonces_issued == 0` 表示未经过加密层。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodecStats {
    /// 加密层发出的 nonce 数（= 加密块数）。
    pub nonces_issued: u64,
    /// 加密层处理过的明文字节数。
    pub sealed_bytes: u64,
}

/// 有状态编码器：可多次 `encode`，最后 `finish` 冲刷尾部。
pub trait Encoder: Send {
    fn encode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError>;
    fn finish(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError>;
    /// 运行期统计。
    fn stats(&self) -> CodecStats {
        CodecStats::default()
    }
}

/// 有状态解码器：`decode` 消费 0+ 完整长度前缀块，`finish` 冲刷尾部。
pub trait Decoder: Send {
    fn decode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError>;
    fn finish(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError>;
}

impl<T: Encoder + ?Sized> Encoder for Box<T> {
    fn encode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        (**self).encode(input, output)
    }

    fn finish(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError> {
        (**self).finish(output)
    }

    fn stats(&self) -> CodecStats {
        (**self).stats()
    }
}

impl<T: Decoder + ?Sized> Decoder for Box<T> {
    fn decode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        (**self).decode(input, output)
    }

    fn finish(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError> {
        (**self).finish(output)
    }
}

// ---------------------------------------------------------------------------
// 块帧：magic(2) | version(1) | kind(1) | length(4 BE) | payload
// ---------------------------------------------------------------------------

const FRAME_MAGIC: [u8; 2] = [0x57, 0x50]; // "WP"
const FRAME_VERSION: u8 = 1;

/// 帧类型：标识每个块的内容（压缩/加密算法），让字节流自描述——拿到流即可判断是否密文、用了哪个算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Gzip = 0x01,
    Zstd = 0x02,
    Aes256Gcm = 0x03,
    Sm4Gcm = 0x04,
}

impl FrameKind {
    fn from_u8(v: u8) -> Result<Self, CodecError> {
        match v {
            0x01 => Ok(FrameKind::Gzip),
            0x02 => Ok(FrameKind::Zstd),
            0x03 => Ok(FrameKind::Aes256Gcm),
            0x04 => Ok(FrameKind::Sm4Gcm),
            other => Err(CodecError::Decode(format!(
                "unknown frame kind 0x{other:02x}"
            ))),
        }
    }
}

fn put_block(out: &mut Vec<u8>, kind: FrameKind, payload: &[u8]) -> Result<(), CodecError> {
    let len = u32::try_from(payload.len())
        .map_err(|_| CodecError::Compress(format!("block too large: {} bytes", payload.len())))?;
    out.extend_from_slice(&FRAME_MAGIC);
    out.push(FRAME_VERSION);
    out.push(kind as u8);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

fn take_block<'a>(input: &mut &'a [u8]) -> Result<(FrameKind, &'a [u8]), CodecError> {
    if input.len() < 8 {
        return Err(CodecError::Decode("truncated block header".into()));
    }
    if input[0] != FRAME_MAGIC[0] || input[1] != FRAME_MAGIC[1] {
        return Err(CodecError::Decode("bad frame magic".into()));
    }
    if input[2] != FRAME_VERSION {
        return Err(CodecError::Decode("unsupported frame version".into()));
    }
    let kind = FrameKind::from_u8(input[3])?;
    let len = u32::from_be_bytes([input[4], input[5], input[6], input[7]]) as usize;
    *input = &input[8..];
    if input.len() < len {
        return Err(CodecError::Decode(format!(
            "truncated block payload: need {len}, have {}",
            input.len()
        )));
    }
    let (payload, rest) = input.split_at(len);
    *input = rest;
    Ok((kind, payload))
}

// ---------------------------------------------------------------------------
// 压缩
// ---------------------------------------------------------------------------

/// 压缩块阈值：达到该大小即产出一个独立压缩块。
const COMPRESS_BLOCK_SIZE: usize = 64 * 1024;

type CompressFn = fn(&[u8], i32) -> Result<Vec<u8>, CodecError>;
type DecompressFn = fn(&[u8]) -> Result<Vec<u8>, CodecError>;

fn gzip_compress(data: &[u8], level: i32) -> Result<Vec<u8>, CodecError> {
    let level = level.clamp(0, 9) as u32;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
    enc.write_all(data)
        .map_err(|e| CodecError::Compress(e.to_string()))?;
    enc.finish()
        .map_err(|e| CodecError::Compress(e.to_string()))
}

fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut dec = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| CodecError::Decode(e.to_string()))?;
    Ok(out)
}

fn zstd_compress(data: &[u8], level: i32) -> Result<Vec<u8>, CodecError> {
    zstd::bulk::compress(data, level).map_err(|e| CodecError::Compress(e.to_string()))
}

fn zstd_decompress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    zstd::stream::decode_all(data).map_err(|e| CodecError::Decode(e.to_string()))
}

/// 按块压缩：把输入累计到阈值再压缩，保证小消息也能拿到可接受的压缩率。
struct BlockCompressor {
    buffer: Vec<u8>,
    level: i32,
    kind: FrameKind,
    compress: CompressFn,
}

impl BlockCompressor {
    fn flush(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError> {
        while self.buffer.len() >= COMPRESS_BLOCK_SIZE {
            let block: Vec<u8> = self.buffer.drain(..COMPRESS_BLOCK_SIZE).collect();
            put_block(output, self.kind, &(self.compress)(&block, self.level)?)?;
        }
        Ok(())
    }
}

impl Encoder for BlockCompressor {
    fn encode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        self.buffer.extend_from_slice(input);
        self.flush(output)
    }

    fn finish(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError> {
        if !self.buffer.is_empty() {
            let block = std::mem::take(&mut self.buffer);
            put_block(output, self.kind, &(self.compress)(&block, self.level)?)?;
        }
        Ok(())
    }
}

struct BlockDecompressor {
    kind: FrameKind,
    decompress: DecompressFn,
}

impl Decoder for BlockDecompressor {
    fn decode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        let mut rest = input;
        while !rest.is_empty() {
            let (kind, payload) = take_block(&mut rest)?;
            if kind != self.kind {
                return Err(CodecError::Decode(format!(
                    "unexpected frame kind {kind:?}, expected {:?}",
                    self.kind
                )));
            }
            output.extend_from_slice(&(self.decompress)(payload)?);
        }
        Ok(())
    }

    fn finish(&mut self, _output: &mut Vec<u8>) -> Result<(), CodecError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 加密（AEAD：AES-256-GCM / SM4-GCM）
// ---------------------------------------------------------------------------

const NONCE_LEN: usize = 12;

/// 生成 96-bit 随机 nonce。GCM 要求「同一密钥下 nonce 不可重复」——随机 nonce 在该位数下碰撞概率可忽略
/// （生日界约 2^48 个 nonce），但同一密钥不应跨多个长期独立实例共享（否则碰撞界被放大）。
fn new_nonce() -> Result<[u8; NONCE_LEN], CodecError> {
    use ring::rand::SecureRandom;
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; NONCE_LEN];
    rng.fill(&mut bytes)
        .map_err(|_| CodecError::Encrypt("nonce generation failed".into()))?;
    Ok(bytes)
}

/// AEAD 后端抽象：屏蔽 ring（AES-256-GCM）与 sm4-gcm（SM4-GCM）两套 API 差异。
/// `aad` 为附加认证数据（这里只放帧的 `kind` 字节，把「帧类型」与密文绑定，防帧头被重新标记）。
trait AeadCipher: Send {
    fn seal(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CodecError>;
    fn open(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CodecError>;
}

struct RingAes256Gcm(ring::aead::LessSafeKey);

impl AeadCipher for RingAes256Gcm {
    fn seal(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CodecError> {
        let nonce = ring::aead::Nonce::assume_unique_for_key(*nonce);
        let mut sealed = plaintext.to_vec();
        self.0
            .seal_in_place_append_tag(nonce, ring::aead::Aad::from(aad), &mut sealed)
            .map_err(|_| CodecError::Encrypt("aes-gcm seal failed".into()))?;
        Ok(sealed)
    }

    fn open(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CodecError> {
        let nonce = ring::aead::Nonce::try_assume_unique_for_key(nonce)
            .map_err(|_| CodecError::Decode("bad nonce".into()))?;
        let mut data = ciphertext.to_vec();
        let plaintext = self
            .0
            .open_in_place(nonce, ring::aead::Aad::from(aad), &mut data)
            .map_err(|_| CodecError::Decode("open failed (tampered data?)".into()))?;
        Ok(plaintext.to_vec())
    }
}

struct Sm4GcmCipher(sm4_gcm::Sm4Key);

impl AeadCipher for Sm4GcmCipher {
    fn seal(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CodecError> {
        Ok(sm4_gcm::sm4_gcm_aad_encrypt(&self.0, nonce, aad, plaintext))
    }

    fn open(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CodecError> {
        sm4_gcm::sm4_gcm_aad_decrypt(&self.0, nonce, aad, ciphertext)
            .map_err(|e| CodecError::Decode(format!("sm4-gcm open failed: {e}")))
    }
}

struct AeadEncoder {
    cipher: Box<dyn AeadCipher>,
    kind: FrameKind,
    nonces_issued: u64,
    sealed_bytes: u64,
}

impl Encoder for AeadEncoder {
    /// 单次 `encode` 把整段输入作为一个 AEAD 块（一个 nonce）加密；块大小由调用方（sink 批大小）限定，
    /// 远低于 GCM 的 ~64GB 明文上限。
    fn encode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        if input.is_empty() {
            return Ok(());
        }
        let nonce = new_nonce()?;
        let aad = [self.kind as u8];
        let sealed = self.cipher.seal(&nonce, &aad, input)?;
        let mut payload = Vec::with_capacity(NONCE_LEN + sealed.len());
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&sealed);
        put_block(output, self.kind, &payload)?;
        self.nonces_issued += 1;
        self.sealed_bytes += input.len() as u64;
        Ok(())
    }

    fn finish(&mut self, _output: &mut Vec<u8>) -> Result<(), CodecError> {
        Ok(())
    }

    fn stats(&self) -> CodecStats {
        CodecStats {
            nonces_issued: self.nonces_issued,
            sealed_bytes: self.sealed_bytes,
        }
    }
}

struct AeadDecoder {
    cipher: Box<dyn AeadCipher>,
    kind: FrameKind,
}

impl Decoder for AeadDecoder {
    fn decode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        let mut rest = input;
        while !rest.is_empty() {
            let (kind, payload) = take_block(&mut rest)?;
            if kind != self.kind {
                return Err(CodecError::Decode(format!(
                    "unexpected frame kind {kind:?}, expected {:?}",
                    self.kind
                )));
            }
            if payload.len() < NONCE_LEN {
                return Err(CodecError::Decode("ciphertext block too short".into()));
            }
            let (nonce, sealed) = payload.split_at(NONCE_LEN);
            let nonce: [u8; NONCE_LEN] = nonce
                .try_into()
                .map_err(|_| CodecError::Decode("bad nonce length".into()))?;
            let aad = [kind as u8];
            let plaintext = self.cipher.open(&nonce, &aad, sealed)?;
            output.extend_from_slice(&plaintext);
        }
        Ok(())
    }

    fn finish(&mut self, _output: &mut Vec<u8>) -> Result<(), CodecError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 链式组合：先压缩后加密
// ---------------------------------------------------------------------------

struct ChainEncoder {
    compress: Option<Box<dyn Encoder>>,
    encrypt: Option<Box<dyn Encoder>>,
}

impl Encoder for ChainEncoder {
    fn encode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        match (&mut self.compress, &mut self.encrypt) {
            (Some(c), Some(e)) => {
                let mut mid = Vec::new();
                c.encode(input, &mut mid)?;
                if !mid.is_empty() {
                    e.encode(&mid, output)?;
                }
            }
            (Some(c), None) => c.encode(input, output)?,
            (None, Some(e)) => e.encode(input, output)?,
            (None, None) => output.extend_from_slice(input),
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError> {
        match (&mut self.compress, &mut self.encrypt) {
            (Some(c), Some(e)) => {
                let mut mid = Vec::new();
                c.finish(&mut mid)?;
                if !mid.is_empty() {
                    e.encode(&mid, output)?;
                }
                e.finish(output)?;
            }
            (Some(c), None) => c.finish(output)?,
            (None, Some(e)) => e.finish(output)?,
            (None, None) => {}
        }
        Ok(())
    }

    fn stats(&self) -> CodecStats {
        match &self.encrypt {
            Some(e) => e.stats(),
            None => CodecStats::default(),
        }
    }
}

struct ChainDecoder {
    decrypt: Option<Box<dyn Decoder>>,
    decompress: Option<Box<dyn Decoder>>,
}

impl Decoder for ChainDecoder {
    fn decode(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), CodecError> {
        match (&mut self.decrypt, &mut self.decompress) {
            (Some(d), Some(c)) => {
                let mut mid = Vec::new();
                d.decode(input, &mut mid)?;
                c.decode(&mid, output)?;
            }
            (Some(d), None) => d.decode(input, output)?,
            (None, Some(c)) => c.decode(input, output)?,
            (None, None) => output.extend_from_slice(input),
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut Vec<u8>) -> Result<(), CodecError> {
        match (&mut self.decrypt, &mut self.decompress) {
            (Some(d), Some(c)) => {
                let mut mid = Vec::new();
                d.finish(&mut mid)?;
                if !mid.is_empty() {
                    c.decode(&mid, output)?;
                }
                c.finish(output)?;
            }
            (Some(d), None) => d.finish(output)?,
            (None, Some(c)) => c.finish(output)?,
            (None, None) => {}
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 构建入口
// ---------------------------------------------------------------------------

fn validate_compress_level(cfg: &CompressConfig) -> Result<(), CodecError> {
    let (lo, hi, name) = match cfg.algo {
        CompressionAlgo::Gzip => (0, 9, "gzip"),
        CompressionAlgo::Zstd => (-7, 22, "zstd"),
    };
    if !(lo..=hi).contains(&cfg.level) {
        return Err(CodecError::Config(format!(
            "{name} level must be {lo}..={hi}, got {}",
            cfg.level
        )));
    }
    Ok(())
}

fn build_compressor(cfg: &CompressConfig) -> Box<dyn Encoder> {
    let (compress, kind): (CompressFn, FrameKind) = match cfg.algo {
        CompressionAlgo::Gzip => (gzip_compress, FrameKind::Gzip),
        CompressionAlgo::Zstd => (zstd_compress, FrameKind::Zstd),
    };
    Box::new(BlockCompressor {
        buffer: Vec::new(),
        level: cfg.level,
        kind,
        compress,
    })
}

fn build_decompressor(cfg: &CompressConfig) -> Box<dyn Decoder> {
    let (decompress, kind): (DecompressFn, FrameKind) = match cfg.algo {
        CompressionAlgo::Gzip => (gzip_decompress, FrameKind::Gzip),
        CompressionAlgo::Zstd => (zstd_decompress, FrameKind::Zstd),
    };
    Box::new(BlockDecompressor { kind, decompress })
}

fn build_aead_cipher(cfg: &EncryptConfig) -> Result<Box<dyn AeadCipher>, CodecError> {
    match cfg.cipher {
        Cipher::Aes256Gcm => {
            const KEY_LEN: usize = 32;
            if cfg.key.len() != KEY_LEN {
                return Err(CodecError::Config(format!(
                    "aes-256-gcm key must be {KEY_LEN} bytes, got {}",
                    cfg.key.len()
                )));
            }
            let unbound = ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &cfg.key)
                .map_err(|_| CodecError::Config("invalid aes-256-gcm key".into()))?;
            Ok(Box::new(RingAes256Gcm(ring::aead::LessSafeKey::new(
                unbound,
            ))))
        }
        Cipher::Sm4Gcm => {
            const KEY_LEN: usize = 16;
            if cfg.key.len() != KEY_LEN {
                return Err(CodecError::Config(format!(
                    "sm4-gcm key must be {KEY_LEN} bytes, got {}",
                    cfg.key.len()
                )));
            }
            let key_bytes: [u8; KEY_LEN] = cfg
                .key
                .as_slice()
                .try_into()
                .map_err(|_| CodecError::Config("sm4-gcm key must be 16 bytes".into()))?;
            Ok(Box::new(Sm4GcmCipher(sm4_gcm::Sm4Key(key_bytes))))
        }
    }
}

fn cipher_frame_kind(cipher: Cipher) -> FrameKind {
    match cipher {
        Cipher::Aes256Gcm => FrameKind::Aes256Gcm,
        Cipher::Sm4Gcm => FrameKind::Sm4Gcm,
    }
}

fn build_encryptor(cfg: &EncryptConfig) -> Result<Box<dyn Encoder>, CodecError> {
    Ok(Box::new(AeadEncoder {
        cipher: build_aead_cipher(cfg)?,
        kind: cipher_frame_kind(cfg.cipher),
        nonces_issued: 0,
        sealed_bytes: 0,
    }))
}

fn build_decryptor(cfg: &EncryptConfig) -> Result<Box<dyn Decoder>, CodecError> {
    Ok(Box::new(AeadDecoder {
        cipher: build_aead_cipher(cfg)?,
        kind: cipher_frame_kind(cfg.cipher),
    }))
}

/// 构建编码器（sink 侧）。`compression` / `encryption` 传 `None` 表示该层不启用；
/// 两者都 `None` 时退化为透传。
pub fn build_encoder(
    compression: Option<&CompressConfig>,
    encryption: Option<&EncryptConfig>,
) -> Result<Box<dyn Encoder>, CodecError> {
    if let Some(c) = compression {
        validate_compress_level(c)?;
    }
    Ok(Box::new(ChainEncoder {
        compress: compression.map(build_compressor),
        encrypt: encryption.map(build_encryptor).transpose()?,
    }))
}

/// 构建解码器（source 侧 / 测试）。顺序与 `build_encoder` 相反。
pub fn build_decoder(
    compression: Option<&CompressConfig>,
    encryption: Option<&EncryptConfig>,
) -> Result<Box<dyn Decoder>, CodecError> {
    if let Some(c) = compression {
        validate_compress_level(c)?;
    }
    Ok(Box::new(ChainDecoder {
        decrypt: encryption.map(build_decryptor).transpose()?,
        decompress: compression.map(build_decompressor),
    }))
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> Vec<u8> {
        vec![0xAB; 32]
    }

    fn sm4_test_key() -> Vec<u8> {
        vec![0xCD; 16]
    }

    fn encode_all(enc: &mut dyn Encoder, chunks: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for c in chunks {
            enc.encode(c, &mut out).unwrap();
        }
        enc.finish(&mut out).unwrap();
        out
    }

    fn decode_all(dec: &mut dyn Decoder, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        dec.decode(input, &mut out).unwrap();
        dec.finish(&mut out).unwrap();
        out
    }

    fn big_payload() -> Vec<u8> {
        // 超过多个压缩块阈值，跨块切分
        (0..(COMPRESS_BLOCK_SIZE * 3 + 123))
            .map(|i| (i % 251) as u8)
            .collect()
    }

    #[test]
    fn gzip_roundtrip() {
        let cfg = CompressConfig {
            algo: CompressionAlgo::Gzip,
            level: 6,
        };
        let mut enc = build_encoder(Some(&cfg), None).unwrap();
        let mut dec = build_decoder(Some(&cfg), None).unwrap();

        let payload = big_payload();
        let encoded = encode_all(&mut enc, &[&payload]);
        let decoded = decode_all(&mut dec, &encoded);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn zstd_roundtrip() {
        let cfg = CompressConfig {
            algo: CompressionAlgo::Zstd,
            level: 3,
        };
        let mut enc = build_encoder(Some(&cfg), None).unwrap();
        let mut dec = build_decoder(Some(&cfg), None).unwrap();

        let payload = big_payload();
        let encoded = encode_all(&mut enc, &[&payload]);
        let decoded = decode_all(&mut dec, &encoded);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn compression_is_smaller_on_redundant_data() {
        let cfg = CompressConfig::default();
        let mut enc = build_encoder(Some(&cfg), None).unwrap();
        let payload = vec![0x5A; COMPRESS_BLOCK_SIZE * 2];
        let encoded = encode_all(&mut enc, &[&payload]);
        assert!(
            encoded.len() < payload.len(),
            "redundant data should compress"
        );
    }

    #[test]
    fn aes_gcm_roundtrip() {
        let cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        let mut dec = build_decoder(None, Some(&cfg)).unwrap();

        let payload = big_payload();
        let encoded = encode_all(&mut enc, &[&payload]);
        let decoded = decode_all(&mut dec, &encoded);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn aes_gcm_tamper_detection() {
        let cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        let mut dec = build_decoder(None, Some(&cfg)).unwrap();

        let payload = b"attack at dawn".to_vec();
        let mut encoded = encode_all(&mut enc, &[&payload]);
        // 翻转密文最后一个字节（tag 内）
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;

        let mut out = Vec::new();
        assert!(
            dec.decode(&encoded, &mut out).is_err(),
            "tampered ciphertext must fail to decrypt"
        );
    }

    #[test]
    fn aes_gcm_nonce_is_unique() {
        let cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        let payload = b"same plaintext".to_vec();
        let mut a = Vec::new();
        let mut b = Vec::new();
        enc.encode(&payload, &mut a).unwrap();
        enc.encode(&payload, &mut b).unwrap();
        assert_ne!(
            a, b,
            "same plaintext should yield different ciphertext (fresh nonce)"
        );
    }

    #[test]
    fn sm4_gcm_roundtrip() {
        let cfg = EncryptConfig {
            cipher: Cipher::Sm4Gcm,
            key: sm4_test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        let mut dec = build_decoder(None, Some(&cfg)).unwrap();

        let payload = big_payload();
        let encoded = encode_all(&mut enc, &[&payload]);
        let decoded = decode_all(&mut dec, &encoded);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn sm4_gcm_tamper_detection() {
        let cfg = EncryptConfig {
            cipher: Cipher::Sm4Gcm,
            key: sm4_test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        let mut dec = build_decoder(None, Some(&cfg)).unwrap();

        let payload = b"attack at dawn".to_vec();
        let mut encoded = encode_all(&mut enc, &[&payload]);
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;

        let mut out = Vec::new();
        assert!(
            dec.decode(&encoded, &mut out).is_err(),
            "tampered sm4-gcm ciphertext must fail to decrypt"
        );
    }

    #[test]
    fn chain_roundtrip_sm4_compress_then_encrypt() {
        let comp = CompressConfig::default();
        let enc_cfg = EncryptConfig {
            cipher: Cipher::Sm4Gcm,
            key: sm4_test_key(),
        };
        let mut enc = build_encoder(Some(&comp), Some(&enc_cfg)).unwrap();
        let mut dec = build_decoder(Some(&comp), Some(&enc_cfg)).unwrap();

        let payload = big_payload();
        let encoded = encode_all(&mut enc, &[&payload]);
        let decoded = decode_all(&mut dec, &encoded);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn chain_roundtrip_compress_then_encrypt() {
        let comp = CompressConfig::default();
        let enc_cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: test_key(),
        };
        let mut enc = build_encoder(Some(&comp), Some(&enc_cfg)).unwrap();
        let mut dec = build_decoder(Some(&comp), Some(&enc_cfg)).unwrap();

        let payload = big_payload();
        let encoded = encode_all(&mut enc, &[&payload]);
        let decoded = decode_all(&mut dec, &encoded);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn passthrough_when_no_codec() {
        let mut enc = build_encoder(None, None).unwrap();
        let mut dec = build_decoder(None, None).unwrap();
        let payload = b"raw bytes".to_vec();
        let encoded = encode_all(&mut enc, &[&payload]);
        assert_eq!(encoded, payload);
        let decoded = decode_all(&mut dec, &encoded);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn empty_input_produces_no_blocks() {
        let comp = CompressConfig::default();
        let mut enc = build_encoder(Some(&comp), None).unwrap();
        let mut out = Vec::new();
        enc.encode(&[], &mut out).unwrap();
        enc.finish(&mut out).unwrap();
        assert!(out.is_empty(), "empty input should produce no blocks");
    }

    #[test]
    fn invalid_key_length_rejected() {
        for (cipher, key_len) in [
            (Cipher::Aes256Gcm, 16), // AES-256 需 32，给 16 错
            (Cipher::Aes256Gcm, 31),
            (Cipher::Sm4Gcm, 32), // SM4 需 16，给 32 错
            (Cipher::Sm4Gcm, 15),
        ] {
            let cfg = EncryptConfig {
                cipher,
                key: vec![0x01; key_len],
            };
            assert!(
                build_encoder(None, Some(&cfg)).is_err(),
                "cipher={cipher:?} key_len={key_len} should be rejected"
            );
            assert!(
                build_decoder(None, Some(&cfg)).is_err(),
                "cipher={cipher:?} key_len={key_len} should be rejected on decode"
            );
        }
    }

    #[test]
    fn truncated_block_rejected() {
        let comp = CompressConfig::default();
        let mut enc = build_encoder(Some(&comp), None).unwrap();
        let encoded = encode_all(&mut enc, &[&big_payload()]);
        // 截断最后 3 字节，制造不完整块
        let truncated = &encoded[..encoded.len() - 3];
        let mut dec = build_decoder(Some(&comp), None).unwrap();
        assert!(dec.decode(truncated, &mut Vec::new()).is_err());
    }

    #[test]
    fn streaming_multiple_chunks_roundtrip() {
        // 多次小块 encode + 一次 finish，覆盖缓冲/分块/冲刷路径
        let comp = CompressConfig::default();
        let mut enc = build_encoder(Some(&comp), None).unwrap();
        let mut dec = build_decoder(Some(&comp), None).unwrap();

        let chunk: Vec<u8> = (0..1000).map(|i| (i % 7) as u8).collect();
        let mut encoded = Vec::new();
        for _ in 0..200 {
            enc.encode(&chunk, &mut encoded).unwrap();
        }
        enc.finish(&mut encoded).unwrap();

        let mut decoded = Vec::new();
        dec.decode(&encoded, &mut decoded).unwrap();
        dec.finish(&mut decoded).unwrap();

        let expected: Vec<u8> = chunk
            .iter()
            .cycle()
            .take(chunk.len() * 200)
            .copied()
            .collect();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn gzip_level_boundaries_roundtrip() {
        for level in [0, 9] {
            let cfg = CompressConfig {
                algo: CompressionAlgo::Gzip,
                level,
            };
            let mut enc = build_encoder(Some(&cfg), None).unwrap();
            let mut dec = build_decoder(Some(&cfg), None).unwrap();
            let payload = big_payload();
            let encoded = encode_all(&mut enc, &[&payload]);
            assert_eq!(
                decode_all(&mut dec, &encoded),
                payload,
                "gzip level {level}"
            );
        }
    }

    #[test]
    fn zstd_level_boundaries_roundtrip() {
        for level in [-7, 22] {
            let cfg = CompressConfig {
                algo: CompressionAlgo::Zstd,
                level,
            };
            let mut enc = build_encoder(Some(&cfg), None).unwrap();
            let mut dec = build_decoder(Some(&cfg), None).unwrap();
            let payload = big_payload();
            let encoded = encode_all(&mut enc, &[&payload]);
            assert_eq!(
                decode_all(&mut dec, &encoded),
                payload,
                "zstd level {level}"
            );
        }
    }

    #[test]
    fn invalid_compress_level_rejected() {
        for (algo, level) in [
            (CompressionAlgo::Gzip, 10),
            (CompressionAlgo::Gzip, -1),
            (CompressionAlgo::Zstd, 23),
            (CompressionAlgo::Zstd, -8),
        ] {
            let cfg = CompressConfig { algo, level };
            assert!(
                build_encoder(Some(&cfg), None).is_err(),
                "algo={algo:?} level={level} should be rejected"
            );
            assert!(
                build_decoder(Some(&cfg), None).is_err(),
                "algo={algo:?} level={level} should be rejected on decode too"
            );
        }
    }

    fn hex_decode(s: &str) -> Vec<u8> {
        let s = s.trim();
        assert!(
            s.len().is_multiple_of(2),
            "hex string must have even length"
        );
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// 金标准测试向量：零 key/IV + 空明文的 AES-256-GCM 输出必须逐字节等于 NIST 规定值。
    #[test]
    fn aes_256_gcm_kat_zero_vector() {
        let cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: vec![0u8; 32],
        };
        let cipher = build_aead_cipher(&cfg).unwrap();
        let sealed = cipher.seal(&[0u8; 12], &[], b"").unwrap();
        let expected = hex_decode("530f8afbc74536b9a963b4f1c4cb738b");
        assert_eq!(sealed, expected, "AES-256-GCM zero vector must match NIST");
    }

    /// 金标准测试向量：SM4-GCM 输出必须逐字节等于 Bouncy Castle 向量。
    #[test]
    fn sm4_gcm_kat_bc_vector() {
        let cfg = EncryptConfig {
            cipher: Cipher::Sm4Gcm,
            key: vec![0u8; 16],
        };
        let cipher = build_aead_cipher(&cfg).unwrap();
        let sealed = cipher.seal(&[0u8; 12], &[], b"hello world").unwrap();
        let expected = hex_decode("1587c6137e306fed6a6a5f49539b6dd6fe2b7872c3279636db07c2");
        assert_eq!(sealed, expected, "SM4-GCM must match Bouncy Castle vector");
    }

    /// 附加认证数据（AAD）把帧 `kind` 与密文绑定：换 AAD 解不开。
    #[test]
    fn aad_binds_frame_kind() {
        for cipher in [Cipher::Aes256Gcm, Cipher::Sm4Gcm] {
            let cfg = EncryptConfig {
                cipher,
                key: match cipher {
                    Cipher::Aes256Gcm => test_key(),
                    Cipher::Sm4Gcm => sm4_test_key(),
                },
            };
            let c = build_aead_cipher(&cfg).unwrap();
            let nonce = [0x12u8; 12];
            let sealed = c.seal(&nonce, &[FrameKind::Sm4Gcm as u8], b"data").unwrap();

            // 用错误 AAD（另一个 kind）解 → 必须失败
            assert!(
                c.open(&nonce, &[FrameKind::Aes256Gcm as u8], &sealed)
                    .is_err(),
                "{cipher:?}: wrong AAD must fail to open"
            );
            // 用正确 AAD 解 → 成功
            assert_eq!(
                c.open(&nonce, &[FrameKind::Sm4Gcm as u8], &sealed).unwrap(),
                b"data",
                "{cipher:?}: correct AAD must open"
            );
        }
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        let plaintext = b"the quick brown fox jumps over the lazy dog 0123456789".to_vec();
        let encoded = encode_all(&mut enc, &[&plaintext]);
        assert!(
            !encoded
                .windows(plaintext.len())
                .any(|w| w == plaintext.as_slice()),
            "ciphertext must not contain the plaintext as a contiguous substring"
        );
    }

    #[test]
    fn ciphertext_is_incompressible() {
        let cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        // 高冗余明文：本身可被大幅压缩，但加密后不可再压缩。
        let plaintext = vec![0x41u8; 256 * 1024];
        let encoded = encode_all(&mut enc, &[&plaintext]);
        assert!(
            encoded.len() >= plaintext.len(),
            "ciphertext should be at least as large as plaintext"
        );

        let recompressed = zstd_compress(&encoded, 3).unwrap();
        assert!(
            recompressed.len() * 100 >= encoded.len() * 99,
            "encrypted output should be incompressible ({} -> {})",
            encoded.len(),
            recompressed.len()
        );

        // 对照：明文本身高度可压缩
        let plain_compressed = zstd_compress(&plaintext, 3).unwrap();
        assert!(
            plain_compressed.len() < plaintext.len() / 10,
            "redundant plaintext should compress well ({} -> {})",
            plaintext.len(),
            plain_compressed.len()
        );
    }

    #[test]
    fn frame_has_self_describing_header() {
        let cfg = EncryptConfig {
            cipher: Cipher::Sm4Gcm,
            key: sm4_test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        let mut out = Vec::new();
        enc.encode(b"data", &mut out).unwrap();
        assert_eq!(&out[0..2], &FRAME_MAGIC, "frame must start with magic");
        assert_eq!(out[2], FRAME_VERSION, "frame version");
        assert_eq!(
            out[3],
            FrameKind::Sm4Gcm as u8,
            "frame kind must identify SM4-GCM"
        );
    }

    #[test]
    fn stats_reports_encryption() {
        let cfg = EncryptConfig {
            cipher: Cipher::Aes256Gcm,
            key: test_key(),
        };
        let mut enc = build_encoder(None, Some(&cfg)).unwrap();
        enc.encode(b"hello", &mut Vec::new()).unwrap();
        enc.encode(b"world", &mut Vec::new()).unwrap();
        let stats = enc.stats();
        assert_eq!(stats.nonces_issued, 2, "two encodes → two nonces");
        assert_eq!(stats.sealed_bytes, 10, "5 + 5 plaintext bytes sealed");

        // 压缩-only（无加密）时 stats 应为 0，证明「没有经过加密层」
        let mut comp = build_encoder(Some(&CompressConfig::default()), None).unwrap();
        comp.encode(b"hello", &mut Vec::new()).unwrap();
        assert_eq!(comp.stats().nonces_issued, 0, "no encryption → zero nonces");
    }

    #[test]
    fn decoder_rejects_wrong_frame_kind() {
        // 用 gzip 编码的流，用 zstd 解码器去解，应因 frame kind 不匹配而报错
        let gzip_cfg = CompressConfig {
            algo: CompressionAlgo::Gzip,
            level: 6,
        };
        let zstd_cfg = CompressConfig {
            algo: CompressionAlgo::Zstd,
            level: 3,
        };
        let mut enc = build_encoder(Some(&gzip_cfg), None).unwrap();
        let encoded = encode_all(&mut enc, &[&big_payload()]);
        let mut dec = build_decoder(Some(&zstd_cfg), None).unwrap();
        assert!(dec.decode(&encoded, &mut Vec::new()).is_err());
    }
}
