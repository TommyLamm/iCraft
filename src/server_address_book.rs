//! Persistent multiplayer server addresses and server-list ping helpers.
//!
//! This module deliberately owns only the management-plane data.  It does
//! not authenticate or join a server; the UI records a target here and the
//! normal multiplayer client remains responsible for login.

use crate::network::protocol::{Packet, PROTOCOL_VERSION};
use crate::network::transport::Connection;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

const FILE_NAME: &str = "server-addresses.json";
const FORMAT_VERSION: u16 = 1;
const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 32;
const MAX_ADDRESS_BYTES: usize = 256;
const MAX_TEXT_BYTES: usize = 4 * 1024;
const DEFAULT_CAPACITY: usize = 16;

/// A server-list response (or a bounded error captured while trying one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPingResult {
    pub address: String,
    pub version: String,
    pub motd: String,
    pub online_players: u16,
    pub max_players: u16,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressBookError {
    Io(String),
    Invalid(String),
    Oversize { bytes: usize, limit: usize },
    Ping(String),
}

impl fmt::Display for AddressBookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "address book I/O error: {error}"),
            Self::Invalid(error) => write!(f, "invalid address book: {error}"),
            Self::Oversize { bytes, limit } => {
                write!(
                    f,
                    "address book is too large ({bytes} bytes; limit {limit})"
                )
            }
            Self::Ping(error) => write!(f, "server-list ping failed: {error}"),
        }
    }
}

impl std::error::Error for AddressBookError {}

impl From<io::Error> for AddressBookError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddressBook {
    addresses: Vec<String>,
    recent_results: Vec<ServerPingResult>,
    capacity: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskBook {
    version: u16,
    #[serde(default = "default_capacity")]
    capacity: usize,
    addresses: Vec<String>,
    #[serde(default)]
    recent_results: Vec<DiskPingResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskPingResult {
    address: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    motd: String,
    #[serde(default)]
    online_players: u16,
    #[serde(default)]
    max_players: u16,
    #[serde(default)]
    error: Option<String>,
}

impl Default for ServerAddressBook {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

fn default_capacity() -> usize {
    DEFAULT_CAPACITY
}

impl ServerAddressBook {
    pub fn new(capacity: usize) -> Self {
        Self {
            addresses: Vec::new(),
            recent_results: Vec::new(),
            capacity: capacity.clamp(1, MAX_ENTRIES),
        }
    }

    pub fn addresses(&self) -> &[String] {
        &self.addresses
    }

    pub fn recent_results(&self) -> &[ServerPingResult] {
        &self.recent_results
    }

    pub fn result_for(&self, address: &str) -> Option<&ServerPingResult> {
        self.recent_results
            .iter()
            .find(|result| result.address == address)
    }

    /// Load the default management-plane file.  A missing file is equivalent
    /// to an empty book; malformed files are reported to the caller so the UI
    /// can keep running without silently replacing user data.
    pub fn load_default() -> Result<Self, AddressBookError> {
        Self::load(default_path())
    }

    pub fn save_default(&self) -> Result<(), AddressBookError> {
        self.save(default_path())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, AddressBookError> {
        let path = path.as_ref();
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Err(AddressBookError::Invalid(
                "address book path must not be a symlink".into(),
            ));
        }
        if !metadata.is_file() {
            return Err(AddressBookError::Invalid(
                "address book path is not a regular file".into(),
            ));
        }
        let bytes = fs::read(path)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(AddressBookError::Oversize {
                bytes: bytes.len(),
                limit: MAX_FILE_BYTES,
            });
        }
        let disk: DiskBook = serde_json::from_slice(&bytes)
            .map_err(|error| AddressBookError::Invalid(format!("JSON: {error}")))?;
        if disk.version != FORMAT_VERSION {
            return Err(AddressBookError::Invalid(format!(
                "unsupported format version {}",
                disk.version
            )));
        }
        let capacity = if disk.capacity == 0 {
            disk.addresses.len().max(disk.recent_results.len()).max(1)
        } else {
            disk.capacity
        }
        .clamp(1, MAX_ENTRIES);
        let mut book = Self::new(capacity);
        for address in disk.addresses.into_iter().take(MAX_ENTRIES) {
            let Some(address) = normalize_address(&address) else {
                return Err(AddressBookError::Invalid(
                    "saved address is empty, contains whitespace, or exceeds the limit".into(),
                ));
            };
            if !book.addresses.iter().any(|existing| existing == &address) {
                book.addresses.push(address);
            }
        }
        for result in disk.recent_results.into_iter().take(MAX_ENTRIES) {
            let result = ServerPingResult {
                address: result.address,
                version: result.version,
                motd: result.motd,
                online_players: result.online_players,
                max_players: result.max_players,
                error: result.error,
            };
            if !valid_result(&result) {
                return Err(AddressBookError::Invalid(
                    "saved ping result contains an invalid or oversized field".into(),
                ));
            }
            if !book
                .recent_results
                .iter()
                .any(|existing| existing.address == result.address)
            {
                book.recent_results.push(result);
            }
        }
        book.addresses.truncate(book.capacity);
        book.recent_results.truncate(book.capacity);
        Ok(book)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), AddressBookError> {
        let path = path.as_ref();
        validate_destination(path)?;
        let disk = DiskBook {
            version: FORMAT_VERSION,
            capacity: self.capacity,
            addresses: self.addresses.iter().take(MAX_ENTRIES).cloned().collect(),
            recent_results: self
                .recent_results
                .iter()
                .take(MAX_ENTRIES)
                .map(|result| DiskPingResult {
                    address: result.address.clone(),
                    version: sanitize_text(&result.version, MAX_TEXT_BYTES),
                    motd: sanitize_text(&result.motd, MAX_TEXT_BYTES),
                    online_players: result.online_players,
                    max_players: result.max_players,
                    error: result
                        .error
                        .as_deref()
                        .map(|error| sanitize_text(error, MAX_TEXT_BYTES)),
                })
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&disk)
            .map_err(|error| AddressBookError::Invalid(format!("JSON: {error}")))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(AddressBookError::Oversize {
                bytes: bytes.len(),
                limit: MAX_FILE_BYTES,
            });
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AddressBookError::Invalid("address book path has no filename".into()))?;
        let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
        // Refuse a pre-existing temporary symlink rather than following it.
        if let Ok(metadata) = fs::symlink_metadata(&temporary) {
            if metadata.file_type().is_symlink() {
                return Err(AddressBookError::Invalid(
                    "temporary address book path must not be a symlink".into(),
                ));
            }
        }
        fs::write(&temporary, bytes)?;
        if let Err(error) = fs::rename(&temporary, path) {
            if error.kind() == io::ErrorKind::AlreadyExists {
                // Windows does not replace an existing file with rename.  The
                // temporary file is complete before this fallback, so a
                // failed remove/rename still leaves the prior book intact.
                if let Err(remove_error) = fs::remove_file(path) {
                    let _ = fs::remove_file(&temporary);
                    return Err(remove_error.into());
                }
                if let Err(rename_error) = fs::rename(&temporary, path) {
                    let _ = fs::remove_file(&temporary);
                    return Err(rename_error.into());
                }
            } else {
                let _ = fs::remove_file(&temporary);
                return Err(error.into());
            }
        }
        Ok(())
    }

    pub fn remember(&mut self, address: impl Into<String>) {
        let Some(address) = normalize_address(&address.into()) else {
            return;
        };
        self.addresses.retain(|existing| existing != &address);
        self.addresses.insert(0, address);
        self.addresses.truncate(self.capacity);
    }

    pub fn record_ping(&mut self, result: ServerPingResult) {
        let Some(address) = normalize_address(&result.address) else {
            return;
        };
        let result = ServerPingResult {
            address,
            version: sanitize_text(&result.version, MAX_TEXT_BYTES),
            motd: sanitize_text(&result.motd, MAX_TEXT_BYTES),
            online_players: result.online_players,
            max_players: result.max_players,
            error: result
                .error
                .as_deref()
                .map(|error| sanitize_text(error, MAX_TEXT_BYTES)),
        };
        self.remember(result.address.clone());
        self.recent_results
            .retain(|existing| existing.address != result.address);
        self.recent_results.insert(0, result);
        self.recent_results.truncate(self.capacity);
    }

    /// Perform the public server-list ping request and retain both success and
    /// failure in the book.  The operation has a finite timeout and never
    /// performs login or mutates world state.
    pub fn ping(&mut self, address: impl Into<String>, timeout: Duration) -> ServerPingResult {
        let address = normalize_address(&address.into()).unwrap_or_default();
        let result = ping_once(&address, timeout);
        self.record_ping(result.clone());
        result
    }
}

fn default_path() -> PathBuf {
    PathBuf::from(FILE_NAME)
}

fn validate_destination(path: &Path) -> Result<(), AddressBookError> {
    if path.as_os_str().is_empty() {
        return Err(AddressBookError::Invalid(
            "address book path is empty".into(),
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(AddressBookError::Invalid(
                "address book path must not be a symlink".into(),
            ));
        }
        if !metadata.is_file() {
            return Err(AddressBookError::Invalid(
                "address book path is not a regular file".into(),
            ));
        }
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Ok(metadata) = fs::symlink_metadata(parent) {
        if metadata.file_type().is_symlink() {
            return Err(AddressBookError::Invalid(
                "address book parent must not be a symlink".into(),
            ));
        }
    }
    Ok(())
}

fn normalize_address(address: &str) -> Option<String> {
    let address = address.trim();
    if address.is_empty()
        || address.len() > MAX_ADDRESS_BYTES
        || address
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    Some(address.to_string())
}

fn sanitize_text(value: &str, limit: usize) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if output.len() + ch.len_utf8() > limit {
            break;
        }
        output.push(ch);
    }
    output
}

fn valid_result(result: &ServerPingResult) -> bool {
    normalize_address(&result.address).is_some()
        && result.version.len() <= MAX_TEXT_BYTES
        && result.motd.len() <= MAX_TEXT_BYTES
        && result
            .error
            .as_deref()
            .map_or(true, |error| error.len() <= MAX_TEXT_BYTES)
}

fn ping_once(address: &str, timeout: Duration) -> ServerPingResult {
    let timeout = timeout.clamp(Duration::from_millis(1), Duration::from_secs(5));
    let mut result = ServerPingResult {
        address: address.to_string(),
        version: String::new(),
        motd: String::new(),
        online_players: 0,
        max_players: 0,
        error: None,
    };
    if address.is_empty() {
        result.error = Some("address is empty or invalid".into());
        return result;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            result.error = Some(format!("runtime: {error}"));
            return result;
        }
    };
    match runtime.block_on(async {
        let stream = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(address))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connection timed out"))??;
        let mut connection = Connection::new(stream);
        tokio::time::timeout(
            timeout,
            connection.send(&Packet::ServerListPingRequest {
                protocol_version: PROTOCOL_VERSION,
            }),
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ping send timed out"))??;
        let packet = tokio::time::timeout(timeout, connection.recv())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ping response timed out"))??;
        match packet {
            Packet::ServerListPingResponse {
                protocol_version,
                version,
                motd,
                online_players,
                max_players,
            } if protocol_version == PROTOCOL_VERSION => Ok(ServerPingResult {
                address: address.to_string(),
                version,
                motd,
                online_players,
                max_players,
                error: None,
            }),
            Packet::ServerListPingResponse {
                protocol_version, ..
            } => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("protocol version mismatch ({protocol_version})"),
            )),
            packet => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected ping response: {packet:?}"),
            )),
        }
    }) {
        Ok(success) => success,
        Err(error) => {
            result.error = Some(error.to_string());
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("icraft-address-book-{name}-{}", std::process::id()))
    }

    #[test]
    fn round_trip_preserves_addresses_and_ping_metadata() {
        let path = temp_path("round-trip.json");
        let _ = fs::remove_file(&path);
        let mut book = ServerAddressBook::new(3);
        book.record_ping(ServerPingResult {
            address: "example.test:25565".into(),
            version: "1.21.5".into(),
            motd: "Welcome".into(),
            online_players: 2,
            max_players: 20,
            error: None,
        });
        book.record_ping(ServerPingResult {
            address: "bad.test:25565".into(),
            version: String::new(),
            motd: String::new(),
            online_players: 0,
            max_players: 0,
            error: Some("offline".into()),
        });
        book.save(&path).unwrap();
        book.remember("second.test:25565");
        book.save(&path).unwrap();
        let loaded = ServerAddressBook::load(&path).unwrap();
        assert_eq!(loaded.addresses(), book.addresses());
        assert_eq!(loaded.recent_results(), book.recent_results());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_and_oversized_files_are_rejected_without_replacement() {
        let path = temp_path("corrupt.json");
        fs::write(&path, b"not-json").unwrap();
        assert!(matches!(
            ServerAddressBook::load(&path),
            Err(AddressBookError::Invalid(_))
        ));
        fs::write(&path, vec![b' '; MAX_FILE_BYTES + 1]).unwrap();
        assert!(matches!(
            ServerAddressBook::load(&path),
            Err(AddressBookError::Oversize { .. })
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn path_safety_rejects_directory_targets() {
        let directory = temp_path("directory-target");
        fs::create_dir_all(&directory).unwrap();
        let book = ServerAddressBook::default();
        assert!(matches!(
            book.save(&directory),
            Err(AddressBookError::Invalid(_))
        ));
        assert!(matches!(
            ServerAddressBook::load(&directory),
            Err(AddressBookError::Invalid(_))
        ));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn ping_text_is_bounded_and_safe_for_menu_rendering() {
        let mut book = ServerAddressBook::new(1);
        book.record_ping(ServerPingResult {
            address: "example.test:25565".into(),
            version: "v\n".to_string() + &"x".repeat(MAX_TEXT_BYTES + 32),
            motd: "hello\r\nworld".into(),
            online_players: 0,
            max_players: 20,
            error: Some("bad\t".into()),
        });
        let result = &book.recent_results()[0];
        assert!(result.version.len() <= MAX_TEXT_BYTES);
        assert!(!result.version.chars().any(char::is_control));
        assert!(!result.motd.chars().any(char::is_control));
        assert!(!result
            .error
            .as_deref()
            .unwrap()
            .chars()
            .any(char::is_control));
    }

    #[test]
    fn ping_uses_server_list_api_and_records_result() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nonblocking(true).unwrap();
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async move {
                let mut connection =
                    Connection::new(tokio::net::TcpStream::from_std(stream).unwrap());
                assert!(matches!(
                    connection.recv().await.unwrap(),
                    Packet::ServerListPingRequest { .. }
                ));
                connection
                    .send(&Packet::ServerListPingResponse {
                        protocol_version: PROTOCOL_VERSION,
                        version: "test".into(),
                        motd: "hello".into(),
                        online_players: 1,
                        max_players: 8,
                    })
                    .await
                    .unwrap();
            });
        });
        let mut book = ServerAddressBook::new(2);
        let result = book.ping(address, Duration::from_secs(2));
        assert_eq!(result.version, "test");
        assert_eq!(book.recent_results()[0].motd, "hello");
        server.join().unwrap();
    }
}
