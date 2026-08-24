//! 哈希计算 —— xxHash3-64 流式计算（PRD §5.3：默认 xxHash3-64，大文件快）
//!
//! 拷贝时边读边算，回读目标逐文件比对。

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;
use xxhash_rust::xxh3::Xxh3;

/// 流式读取块大小（1 MiB）
pub const CHUNK_SIZE: usize = 1024 * 1024;

/// 对文件计算 xxHash3-64，返回十六进制小写字符串（16 位）。
pub fn hash_file(path: &Path) -> io::Result<String> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let (_, digest) = hash_stream(&mut reader)?;
    Ok(digest)
}

/// 流式计算：返回 (读取字节数, xxHash3-64 hex)。可配合拷贝流边读边算。
pub fn hash_stream<R: Read>(reader: &mut R) -> io::Result<(u64, String)> {
    let mut hasher = Xxh3::new();
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((total, format!("{:016x}", hasher.digest())))
}

/// 对内存字节计算 xxHash3-64 hex（用于小数据与测试）。
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Xxh3::new();
    hasher.update(data);
    format!("{:016x}", hasher.digest())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn hash_known_value() {
        // xxHash3-64("") = 0x2d06800538d394c2 (little-endian 显示为 2d06800538d394c2)
        assert_eq!(hash_bytes(b""), "2d06800538d394c2");
        // 不同内容必不同
        assert_ne!(hash_bytes(b"a"), hash_bytes(b"b"));
        // 与流式一致
        let mut cur = Cursor::new(b"hello world".to_vec());
        let (n, h) = hash_stream(&mut cur).unwrap();
        assert_eq!(n, 11);
        assert_eq!(h, hash_bytes(b"hello world"));
    }

    #[test]
    fn hash_file_matches_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sample.bin");
        let data: Vec<u8> = (0..=255u8).cycle().take(3 * 1024 * 1024 + 17).collect();
        std::fs::write(&p, &data).unwrap();
        let h = hash_file(&p).unwrap();
        assert_eq!(h, hash_bytes(&data));
    }

    #[test]
    fn hash_stream_counts_bytes() {
        let mut cur = Cursor::new(vec![0u8; 5 * CHUNK_SIZE + 3]);
        let (n, _) = hash_stream(&mut cur).unwrap();
        assert_eq!(n, (5 * CHUNK_SIZE + 3) as u64);
    }
}
