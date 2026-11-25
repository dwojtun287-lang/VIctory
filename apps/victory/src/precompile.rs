//! Pre-launch translation into the runtime shader cache.
use anyhow::Result;
use std::path::PathBuf;

#[cfg(windows)]
pub fn run(paths: Vec<PathBuf>) -> Result<()> {
    use anyhow::ensure;
    use std::collections::HashSet;
    use victory_gpu::shader_cache;
    let paths = crate::default_paths(paths)?;
    let cfg = victory_core::Config::load()?;
    shader_cache::prepare_precompile(cfg.cache_subdir("shaders")?).map_err(anyhow::Error::msg)?;
    let mut seen = HashSet::new();
    let mut failed = 0usize;
    let mut files = 0usize;
    println!(
        "Precompiling discoverable shaders. Compressed/encrypted archives may hide additional shaders."
    );
    for root in paths {
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            files += 1;
            println!("Scanning {}", entry.path().display());
            let mut compile = |bytes: &[u8]| {
                if !seen.insert(*blake3::hash(bytes).as_bytes()) {
                    return;
                }
                if let Err(error) = shader_cache::precompile(bytes) {
                    failed += 1;
                    eprintln!(
                        "Shader {} in {}: {error}",
                        seen.len(),
                        entry.path().display()
                    );
                }
                if seen.len() % 100 == 0 {
                    println!(
                        "Shaders: {}, hits/translations: {:?}, failures: {failed}",
                        seen.len(),
                        shader_cache::counts()
                    );
                }
            };
            scan_raw(entry.path(), &mut compile)?;
        }
    }
    let (hits, translated) = shader_cache::counts();
    println!(
        "Precompile complete: {files} files, {} unique shaders, {hits} cached, {translated} translated, {failed} failed.",
        seen.len()
    );
    ensure!(
        !seen.is_empty(),
        "no shaders discovered; game launch stopped"
    );
    ensure!(failed == 0, "{failed} shaders failed; game launch stopped");
    println!(
        "Discoverable shaders ready. Runtime-only shaders and driver pipelines may still compile during play."
    );
    Ok(())
}

#[cfg(not(windows))]
pub fn run(_: Vec<PathBuf>) -> Result<()> {
    anyhow::bail!("shader precompilation requires Windows")
}

fn canonical(bytes: &[u8]) -> Option<&[u8]> {
    let len = u32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?) as usize;
    let bytes = bytes.get(..len)?;
    victory_shader::Container::parse(bytes).ok()?;
    Some(bytes)
}

/// Bounded-memory scan preserving magic split across chunk boundaries.
fn scan_raw(path: &std::path::Path, visit: &mut impl FnMut(&[u8])) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut stream = std::fs::File::open(path)?;
    let mut reader = std::fs::File::open(path)?;
    let size = reader.metadata()?.len();
    let mut buffer = vec![0u8; 1024 * 1024 + 3];
    let mut carry = 0;
    let mut offset = 0u64;
    loop {
        let n = stream.read(&mut buffer[carry..])?;
        if n == 0 {
            break;
        }
        let end = carry + n;
        for pos in memchr::memmem::find_iter(&buffer[..end], b"DXBC") {
            let location = offset - carry as u64 + pos as u64;
            if size - location < 32 {
                continue;
            }
            reader.seek(SeekFrom::Start(location))?;
            let mut header = [0u8; 32];
            reader.read_exact(&mut header)?;
            let len = u32::from_le_bytes(header[24..28].try_into().unwrap()) as usize;
            if !(32..=64 * 1024 * 1024).contains(&len) || len as u64 > size - location {
                continue;
            }
            let mut bytes = vec![0; len];
            bytes[..32].copy_from_slice(&header);
            reader.read_exact(&mut bytes[32..])?;
            if let Some(bytes) = canonical(&bytes) {
                visit(bytes);
            }
        }
        // SGD5 records can also be embedded in large engine archives.
        for pos in memchr::memmem::find_iter(&buffer[..end], &0x000D_DCE7u32.to_le_bytes()) {
            let location = offset - carry as u64 + pos as u64;
            if location < 6 || size - location < 12 {
                continue;
            }
            reader.seek(SeekFrom::Start(location - 6))?;
            let mut header = [0u8; 18];
            reader.read_exact(&mut header)?;
            let n = u32::from_le_bytes(header[2..6].try_into().unwrap()) as usize;
            let repeated = u32::from_le_bytes(header[10..14].try_into().unwrap()) as usize;
            let unpacked = u32::from_le_bytes(header[14..18].try_into().unwrap()) as usize;
            if n != repeated
                || !(12..=64 * 1024 * 1024).contains(&n)
                || unpacked > 64 * 1024 * 1024
                || n as u64 > size - location
            {
                continue;
            }
            let mut record = vec![0; n + 6];
            record[..18].copy_from_slice(&header);
            reader.read_exact(&mut record[18..])?;
            if let Some(record) = victory_game::sgd5::records(&record).next()
                && let Some(bytes) = record.unpack()
                && let Some(bytes) = canonical(&bytes)
            {
                visit(bytes);
            }
        }
        carry = end.min(3);
        buffer.copy_within(end - carry..end, 0);
        offset += n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_unpacks_embedded_sgd5_record_across_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.bin");
        let mut container = vec![0; 32];
        container[..4].copy_from_slice(b"DXBC");
        container[24..28].copy_from_slice(&32u32.to_le_bytes());
        // A raw DEFLATE stored block, avoiding a fixture containing game data.
        let mut packed = vec![1, 32, 0, 223, 255];
        packed.extend(&container);
        let n = (packed.len() + 12) as u32;
        let mut data = vec![0; 1024 * 1024 - 5];
        data.extend([0, 0x32]);
        data.extend(n.to_le_bytes());
        data.extend(0x000D_DCE7u32.to_le_bytes());
        data.extend(n.to_le_bytes());
        data.extend(32u32.to_le_bytes());
        data.extend(packed);
        std::fs::write(&path, data).unwrap();
        let mut found = Vec::new();
        scan_raw(&path, &mut |bytes| found.push(bytes.to_vec())).unwrap();
        assert_eq!(found.len(), 2); // Packed record plus raw bytes in stored block.
        assert!(found.iter().all(|bytes| bytes == &container));
    }

    #[test]
    fn scan_handles_boundary_and_ignores_truncated_container() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shaders.bin");
        let mut container = vec![0; 32];
        container[..4].copy_from_slice(b"DXBC");
        container[24..28].copy_from_slice(&32u32.to_le_bytes());
        let mut data = vec![0; 1024 * 1024 + 1];
        data.extend(&container);
        data.extend(b"DXBC truncated");
        std::fs::write(&path, data).unwrap();
        let mut found = Vec::new();
        scan_raw(&path, &mut |bytes| found.push(bytes.to_vec())).unwrap();
        assert_eq!(found, vec![container]);
    }
}
