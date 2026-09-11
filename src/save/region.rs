use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegionData {
    /// Maps local coordinate (0..32, 0..32) -> Bincode serialized ChunkSaveData bytes
    pub chunks: HashMap<(u8, u8), Vec<u8>>,
}

static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    pub(crate) static ATOMIC_WRITE_FAILPOINT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    pub(crate) static COMPRESS_FAILPOINT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn atomic_write_should_fail(stage: u8) -> bool {
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.get() == stage)
}

#[cfg(not(test))]
fn atomic_write_should_fail(_stage: u8) -> bool {
    false
}

#[cfg(test)]
fn atomic_write_should_crash(stage: &str) -> bool {
    std::env::var("ICRAFT_TEST_ATOMIC_CRASH_STAGE").as_deref() == Ok(stage)
}

#[cfg(not(test))]
fn atomic_write_should_crash(_stage: &str) -> bool {
    false
}

pub fn atomic_write<P: AsRef<Path>>(path: P, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("save");
    let tmp_path = path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
    }

    if atomic_write_should_crash("before_replace") {
        std::process::abort();
    }

    if atomic_write_should_fail(1) {
        let _ = fs::remove_file(&tmp_path);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "injected failure before atomic replacement",
        ));
    }

    if let Err(error) = replace_file_atomically(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }

    if atomic_write_should_crash("after_replace") {
        std::process::abort();
    }

    if atomic_write_should_fail(2) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "injected failure after atomic replacement",
        ));
    }

    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }

    Ok(())
}

/// Write many small sidecars with a single `sync_all` before renames.
/// Each entry is replaced atomically; on failure earlier renames may have
/// already completed (same as sequential `atomic_write` today).
pub fn atomic_write_group<P: AsRef<Path>>(entries: &[(P, &[u8])]) -> io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut prepared: Vec<(std::path::PathBuf, std::path::PathBuf)> =
        Vec::with_capacity(entries.len());
    for (path, bytes) in entries {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("save");
        let tmp_path = path.with_file_name(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;
            file.write_all(bytes)?;
            file.flush()?;
        }
        prepared.push((tmp_path, path.to_path_buf()));
    }

    // One durability barrier for the whole sidecar batch.
    if let Some((tmp, _)) = prepared.last() {
        let file = fs::OpenOptions::new().read(true).write(true).open(tmp)?;
        file.sync_all()?;
    }

    if atomic_write_should_crash("before_replace") {
        std::process::abort();
    }

    if atomic_write_should_fail(1) {
        for (tmp, _) in &prepared {
            let _ = fs::remove_file(tmp);
        }
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "injected failure before atomic replacement",
        ));
    }

    for (tmp_path, path) in &prepared {
        if let Err(error) = replace_file_atomically(tmp_path, path) {
            let _ = fs::remove_file(tmp_path);
            return Err(error);
        }
    }

    if atomic_write_should_crash("after_replace") {
        std::process::abort();
    }

    if atomic_write_should_fail(2) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "injected failure after atomic replacement",
        ));
    }

    #[cfg(unix)]
    if let Some(parent) = prepared[0].1.parent() {
        File::open(parent)?.sync_all()?;
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
pub fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn compress_bytes(data: &[u8]) -> io::Result<Vec<u8>> {
    #[cfg(test)]
    if COMPRESS_FAILPOINT.with(|failpoint| failpoint.get()) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "injected compress failure",
        ));
    }
    // Level 1 (fast): same zlib wrapper as default/best, so existing disk
    // payloads still inflate. Tick-path save/projection no longer pays level 6.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(data)?;
    encoder.finish()
}

/// Inflate a zlib payload, refusing more than `max_len` output bytes.
/// The decoder is wrapped in `take(max_len + 1)` so a hostile stream cannot
/// grow past the caller-supplied budget (Plan 05 fail-closed).
pub fn decompress_bytes_limited(data: &[u8], max_len: usize) -> io::Result<Vec<u8>> {
    let decoder = ZlibDecoder::new(data);
    let mut limited = decoder.take(max_len as u64 + 1);
    let mut result = Vec::new();
    limited.read_to_end(&mut result)?;
    if result.len() > max_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("inflated payload exceeds {max_len} bytes"),
        ));
    }
    Ok(result)
}

pub fn decompress_bytes(data: &[u8]) -> io::Result<Vec<u8>> {
    let documented_max = super::format::LEGACY_VOXEL_COUNT
        .max(crate::dimension::WorldHeight::OVERWORLD.section_count() * 16 * 16 * 16);
    decompress_bytes_limited(data, documented_max)
}

pub fn backup_region_file_if_needed(region_file: &Path) {
    if region_file.exists() {
        let backup_file = region_file.with_extension("bin.bak");
        if !backup_file.exists() {
            let _ = fs::copy(region_file, backup_file);
        }
    }
}
