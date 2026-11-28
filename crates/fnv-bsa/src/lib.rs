//! Reader and writer for Bethesda archives (BSA) as shipped with Fallout 3 and
//! Fallout: New Vegas (archive version 104), and reader of the older version 103
//! (Oblivion's), which the original game also loads (#2092): the same layout, read by
//! its own flag rules ([`flags::known`], [`Archive::embeds_names`]). Its archives are
//! rewritten as 103.
//!
//! Format knowledge: see `docs/spec-notes/bsa.md` (clean-room description with
//! confidence tags). Every structural claim used here was checked against the
//! user's own archives: name hashes for 15,072 files and all folders matched, and
//! the writer reproduces every staged archive byte-for-byte.
//!
//! Tolerance rules (spec section 7): unknown flag bits are preserved and reported,
//! never silently dropped. Anything inconsistent becomes a `Warning`, not a panic.

use fnv_util::Cursor;
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const MAGIC: [u8; 4] = *b"BSA\0";
pub const HEADER_SIZE: u32 = 36;
/// Fallout 3's and New Vegas' archive version, the one written by default.
pub const VERSION: u32 = 104;
/// Oblivion's archive version, read as well (#2092; docs/spec-notes/bsa.md, Version 103).
pub const VERSION_103: u32 = 103;

/// Archive flag bits. Names follow public community documentation; meanings marked
/// "unconfirmed" have not been verified against FNV behaviour.
pub mod flags {
    pub const DIR_NAMES: u32 = 0x1;
    pub const FILE_NAMES: u32 = 0x2;
    pub const COMPRESSED_DEFAULT: u32 = 0x4;
    pub const RETAIN_DIR_NAMES: u32 = 0x8; // unconfirmed effect
    pub const RETAIN_FILE_NAMES: u32 = 0x10; // unconfirmed effect
    pub const RETAIN_FILE_NAME_OFFSETS: u32 = 0x20; // unconfirmed effect
    pub const XBOX360: u32 = 0x40; // big-endian hashes/numbers on Xbox 360; not supported
    pub const UNKNOWN_0X80: u32 = 0x80; // set on NIF/texture archives; no layout effect observed
    pub const EMBED_FILE_NAMES: u32 = 0x100; // v104: data blocks prefixed by a bstring path; v103: no layout effect
    pub const KNOWN: u32 = 0x1FF;
    /// Set on every official version 103 archive with meanings unknown, no layout effect
    /// (as 0x100 there) [documented: UESP "Oblivion Mod:BSA File Format"].
    pub const V103_0X200: u32 = 0x200;
    pub const V103_0X400: u32 = 0x400;

    /// The flag bits known for archive `version` (others are reported).
    pub fn known(version: u32) -> u32 {
        if version == super::VERSION_103 {
            KNOWN | V103_0X200 | V103_0X400
        } else {
            KNOWN
        }
    }
}

/// Size field bit 30: inverts the archive's default compression for this file.
pub const SIZE_COMPRESSION_TOGGLE: u32 = 0x4000_0000;
/// Size field bit 31: meaning undocumented; preserved.
pub const SIZE_UNKNOWN_BIT31: u32 = 0x8000_0000;
pub const SIZE_MASK: u32 = 0x3FFF_FFFF;
/// Largest decompressed size a compressed file may declare: the largest a stored file
/// can be (`SIZE_MASK`, 1 GiB). The shipped archives' largest file is 11,184,976 bytes
/// (a Textures2 DDS) [observed]; the cap only stops a small block that claims, or
/// inflates to, gigabytes (a zip bomb) from being allocated.
pub const MAX_DECOMPRESSED_FILE: usize = SIZE_MASK as usize;

// Messages embed the inner error, so no variant declares a #[source]: an error
// chain printer would otherwise repeat it.
#[derive(Debug, thiserror::Error)]
pub enum BsaError {
    #[error("I/O error: {0}")]
    Io(io::Error),
    #[error("not a BSA archive (bad magic)")]
    NotBsa,
    #[error("unsupported BSA version {0} (expected 104, or Oblivion's 103)")]
    UnsupportedVersion(u32),
    #[error("Xbox 360 (big-endian) archives are not supported")]
    BigEndian,
    #[error("archive truncated: {0}")]
    Truncated(String),
    #[error("cannot decompress {path}: {err}")]
    Decompress { path: String, err: fnv_inflate::InflateError },
    #[error("layout error: {0}")]
    Layout(String),
}

impl From<io::Error> for BsaError {
    fn from(e: io::Error) -> Self {
        BsaError::Io(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub folder_records_offset: u32,
    pub archive_flags: u32,
    pub folder_count: u32,
    pub file_count: u32,
    pub total_folder_name_length: u32,
    pub total_file_name_length: u32,
    /// Content-type bits (meshes/textures/menus/sounds/voices/shaders/...), low 16 bits.
    pub file_flags: u16,
    /// High 16 bits of the file-flags dword. Not zero in every shipped archive; preserved.
    pub file_flags_padding: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub hash: u64,
    /// Raw size field including flag bits.
    pub size_field: u32,
    /// Absolute offset of the file's data from the start of the archive.
    pub offset: u32,
    /// Raw (Windows-1252) file name without directory, from the file name block.
    pub name: Vec<u8>,
}

impl FileRecord {
    pub fn stored_size(&self) -> u32 {
        self.size_field & SIZE_MASK
    }
    pub fn toggles_compression(&self) -> bool {
        self.size_field & SIZE_COMPRESSION_TOGGLE != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRecord {
    pub hash: u64,
    pub count: u32,
    /// Raw offset field: position of this folder's file-record block plus
    /// `total_file_name_length` (observed).
    pub offset: u32,
    /// Raw folder path bytes (Windows-1252, backslash separated), without the NUL.
    pub name: Vec<u8>,
    pub files: Vec<FileRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub code: &'static str,
    pub detail: String,
}

/// Parsed directory of an archive. File data is read on demand.
pub struct Archive {
    pub header: Header,
    pub folders: Vec<FolderRecord>,
    pub warnings: Vec<Warning>,
    /// Bytes of the file-name block after the last name (counted in
    /// `total_file_name_length`). Empty in canonical archives; 105 bytes in
    /// `Fallout - Voices1.bsa` [observed].
    pub name_block_tail: Vec<u8>,
    file: File,
    file_len: u64,
}

/// A flattened view of one file in the archive.
#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub folder: &'a FolderRecord,
    pub record: &'a FileRecord,
    pub folder_index: usize,
    pub file_index: usize,
}

impl<'a> Entry<'a> {
    /// Full path as stored (`folder\name`), decoded from Windows-1252.
    pub fn path(&self) -> String {
        if self.folder.name.is_empty() {
            fnv_util::text::decode(&self.record.name)
        } else {
            format!("{}\\{}", fnv_util::text::decode(&self.folder.name), fnv_util::text::decode(&self.record.name))
        }
    }
}

/// Compute the archive name hash of a folder path or a file name.
///
/// Input is lower-cased (ASCII) and `/` becomes `\`. For files, the extension
/// (from the last '.') is hashed separately and four known extensions set marker
/// bits in the low dword. Verified against every name in the user's archives.
pub fn name_hash(name: &[u8], is_file: bool) -> u64 {
    let lowered: Vec<u8> = name
        .iter()
        .map(|&b| if b == b'/' { b'\\' } else { b.to_ascii_lowercase() })
        .collect();
    let (stem, ext): (&[u8], &[u8]) = if is_file {
        match lowered.iter().rposition(|&b| b == b'.') {
            Some(i) => (&lowered[..i], &lowered[i..]),
            None => (&lowered[..], &[][..]),
        }
    } else {
        (&lowered[..], &[][..])
    };
    let n = stem.len();
    let mut lo: u32 = 0;
    if n > 0 {
        lo = stem[n - 1] as u32
            | if n > 2 { (stem[n - 2] as u32) << 8 } else { 0 }
            | (n as u32) << 16
            | (stem[0] as u32) << 24;
    }
    match ext {
        b".kf" => lo |= 0x80,
        b".nif" => lo |= 0x8000,
        b".dds" => lo |= 0x8080,
        b".wav" => lo |= 0x8000_0000,
        _ => {}
    }
    let mut h2: u32 = 0;
    if n > 2 {
        for &c in &stem[1..n - 2] {
            h2 = h2.wrapping_mul(0x1003F).wrapping_add(c as u32);
        }
    }
    let mut h3: u32 = 0;
    for &c in ext {
        h3 = h3.wrapping_mul(0x1003F).wrapping_add(c as u32);
    }
    ((h2.wrapping_add(h3) as u64) << 32) | lo as u64
}

fn read_bstring_z(c: &mut Cursor) -> Result<Vec<u8>, BsaError> {
    let len = c.u8().map_err(|e| BsaError::Truncated(e.to_string()))? as usize;
    let b = c.bytes(len).map_err(|e| BsaError::Truncated(e.to_string()))?;
    // Length includes the terminating NUL.
    Ok(b.strip_suffix(&[0]).unwrap_or(b).to_vec())
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> Result<Archive, BsaError> {
        let mut file = File::open(path.as_ref())?;
        let file_len = file.metadata()?.len();
        if file_len < HEADER_SIZE as u64 {
            return Err(BsaError::Truncated(format!("the file is {file_len} bytes, shorter than the {HEADER_SIZE}-byte header")));
        }
        let mut hb = [0u8; 36];
        file.read_exact(&mut hb).map_err(|_| BsaError::NotBsa)?;
        let mut c = Cursor::new(&hb);
        let magic = c.tag().unwrap();
        if magic != MAGIC {
            return Err(BsaError::NotBsa);
        }
        let header = Header {
            version: c.u32().unwrap(),
            folder_records_offset: c.u32().unwrap(),
            archive_flags: c.u32().unwrap(),
            folder_count: c.u32().unwrap(),
            file_count: c.u32().unwrap(),
            total_folder_name_length: c.u32().unwrap(),
            total_file_name_length: c.u32().unwrap(),
            file_flags: c.u16().unwrap(),
            file_flags_padding: c.u16().unwrap(),
        };
        if header.version != VERSION && header.version != VERSION_103 {
            return Err(BsaError::UnsupportedVersion(header.version));
        }
        if header.archive_flags & flags::XBOX360 != 0 {
            return Err(BsaError::BigEndian);
        }
        let mut warnings = Vec::new();
        let known = flags::known(header.version);
        if header.archive_flags & !known != 0 {
            warnings.push(Warning {
                code: "unknown_archive_flags",
                detail: format!("0x{:x}", header.archive_flags & !known),
            });
        }
        if header.folder_records_offset != HEADER_SIZE {
            warnings.push(Warning {
                code: "unusual_header_offset",
                detail: header.folder_records_offset.to_string(),
            });
        }
        // Directory size: folder records + file record blocks (with names) + file name block.
        let has_dir_names = header.archive_flags & flags::DIR_NAMES != 0;
        let has_file_names = header.archive_flags & flags::FILE_NAMES != 0;
        let dir_len = header.folder_count as u64 * 16
            + if has_dir_names { header.total_folder_name_length as u64 + header.folder_count as u64 } else { 0 }
            + header.file_count as u64 * 16
            + if has_file_names { header.total_file_name_length as u64 } else { 0 };
        let start = header.folder_records_offset as u64;
        if start + dir_len > file_len {
            return Err(BsaError::Truncated(format!(
                "directory needs {dir_len} bytes at {start}, file is {file_len}"
            )));
        }
        let mut dir = vec![0u8; dir_len as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut dir)?;
        let mut c = Cursor::new(&dir);
        let mut folders = Vec::with_capacity(header.folder_count as usize);
        for _ in 0..header.folder_count {
            let hash = c.u64().map_err(|e| BsaError::Truncated(e.to_string()))?;
            let count = c.u32().map_err(|e| BsaError::Truncated(e.to_string()))?;
            let offset = c.u32().map_err(|e| BsaError::Truncated(e.to_string()))?;
            folders.push(FolderRecord { hash, count, offset, name: Vec::new(), files: Vec::new() });
        }
        let mut total_files = 0u64;
        for (fi, f) in folders.iter_mut().enumerate() {
            let block_pos = start as usize + c.pos;
            let expect = block_pos as u64 + header.total_file_name_length as u64;
            if f.offset as u64 != expect {
                warnings.push(Warning {
                    code: "folder_offset_mismatch",
                    detail: format!("folder #{fi}: offset field {} != computed {}", f.offset, expect),
                });
            }
            if has_dir_names {
                let len_byte = dir.get(c.pos).copied().unwrap_or(0);
                f.name = read_bstring_z(&mut c)?;
                if len_byte as usize != f.name.len() + 1 {
                    // Canonical names carry a NUL counted in the length byte. Archives
                    // made by third-party tools sometimes do not; such an archive
                    // still loads but will not re-serialize byte-identically.
                    warnings.push(Warning {
                        code: "noncanonical_folder_name",
                        detail: format!("folder #{fi}: length byte {len_byte}, name {:?}", fnv_util::text::decode(&f.name)),
                    });
                }
            }
            for _ in 0..f.count {
                let hash = c.u64().map_err(|e| BsaError::Truncated(e.to_string()))?;
                let size_field = c.u32().map_err(|e| BsaError::Truncated(e.to_string()))?;
                let offset = c.u32().map_err(|e| BsaError::Truncated(e.to_string()))?;
                f.files.push(FileRecord { hash, size_field, offset, name: Vec::new() });
            }
            total_files += f.count as u64;
        }
        if total_files != header.file_count as u64 {
            warnings.push(Warning {
                code: "file_count_mismatch",
                detail: format!("header says {}, folder records sum to {total_files}", header.file_count),
            });
        }
        let mut name_block_tail = Vec::new();
        if has_file_names {
            let names = c
                .bytes(header.total_file_name_length as usize)
                .map_err(|e| BsaError::Truncated(e.to_string()))?;
            let mut it = names.split(|&b| b == 0);
            let mut used = 0usize;
            for f in folders.iter_mut() {
                for r in f.files.iter_mut() {
                    match it.next() {
                        Some(n) => {
                            r.name = n.to_vec();
                            used += n.len() + 1;
                        }
                        None => {
                            return Err(BsaError::Layout("file name block has fewer names than files".into()))
                        }
                    }
                }
            }
            if used < names.len() {
                name_block_tail = names[used..].to_vec();
                let zeros = name_block_tail.iter().all(|&b| b == 0);
                warnings.push(Warning {
                    code: "name_block_slack",
                    detail: format!(
                        "{} byte(s) after the last file name ({})",
                        name_block_tail.len(),
                        if zeros { "all NUL" } else { "not all NUL" }
                    ),
                });
            }
        }
        // Consistency checks that drive the work queue; never fatal.
        for f in &folders {
            if has_dir_names && name_hash(&f.name, false) != f.hash {
                warnings.push(Warning {
                    code: "folder_hash_mismatch",
                    detail: format!("{} stored {:016x}", fnv_util::text::decode(&f.name), f.hash),
                });
            }
            for r in &f.files {
                if has_file_names && name_hash(&r.name, true) != r.hash {
                    warnings.push(Warning {
                        code: "file_hash_mismatch",
                        detail: format!("{} stored {:016x}", fnv_util::text::decode(&r.name), r.hash),
                    });
                }
                if r.offset as u64 + r.stored_size() as u64 > file_len {
                    warnings.push(Warning {
                        code: "data_out_of_bounds",
                        detail: format!("{} offset {} size {}", fnv_util::text::decode(&r.name), r.offset, r.stored_size()),
                    });
                }
                if r.size_field & SIZE_UNKNOWN_BIT31 != 0 {
                    warnings.push(Warning {
                        code: "size_bit31_set",
                        detail: fnv_util::text::decode(&r.name),
                    });
                }
            }
        }
        // Data layout. Usual: blocks follow the name block contiguously in record order
        // and the file ends with the last block [observed]. Contiguity is checked in
        // offset order, so an archive whose blocks are merely stored in another order
        // (Fallout - Textures.bsa [observed]) reports only data_not_in_record_order.
        let mut by_offset: Vec<&FileRecord> = folders.iter().flat_map(|f| f.files.iter()).collect();
        let in_record_order = by_offset.windows(2).all(|w| w[0].offset <= w[1].offset);
        if !in_record_order {
            let moved = by_offset.windows(2).filter(|w| w[0].offset > w[1].offset).count();
            warnings.push(Warning {
                code: "data_not_in_record_order",
                detail: format!("{moved} place(s) where a block is stored before the previous record's"),
            });
            by_offset.sort_by_key(|r| r.offset);
        }
        let mut expected = start + dir_len;
        let mut gaps = 0u64;
        let mut first_gap = None;
        for r in by_offset {
            if r.offset as u64 != expected {
                gaps += 1;
                if first_gap.is_none() {
                    first_gap = Some(format!("{}: offset {} expected {}", fnv_util::text::decode(&r.name), r.offset, expected));
                }
            }
            expected = expected.max(r.offset as u64 + r.stored_size() as u64);
        }
        if let Some(g) = first_gap {
            warnings.push(Warning { code: "data_not_contiguous", detail: format!("{gaps} block(s); first {g}") });
        }
        if expected < file_len {
            warnings.push(Warning {
                code: "trailing_bytes",
                detail: format!("{} byte(s) after the last data block", file_len - expected),
            });
        }
        for w in folders.windows(2) {
            if w[0].hash >= w[1].hash {
                warnings.push(Warning { code: "folders_not_sorted", detail: format!("{:016x}", w[1].hash) });
                break;
            }
        }
        tracing::debug!(
            folders = header.folder_count,
            files = header.file_count,
            flags = header.archive_flags,
            "opened BSA"
        );
        for w in &warnings {
            tracing::debug!(code = w.code, detail = %w.detail, "BSA layout anomaly");
        }
        Ok(Archive { header, folders, warnings, name_block_tail, file, file_len })
    }

    /// The tables' half of [`check`]: every file record's data lies inside the file. An
    /// archive that fails it still opens (the files inside the file read), but its tables
    /// are damaged.
    pub fn check_tables(&self) -> Result<(), BsaError> {
        let mut past = self
            .folders
            .iter()
            .flat_map(|f| f.files.iter().map(move |r| (f, r)))
            .filter(|(_, r)| r.offset as u64 + r.stored_size() as u64 > self.file_len);
        let Some((f, r)) = past.next() else { return Ok(()) };
        Err(BsaError::Layout(format!(
            "{} file record(s) point past the end of the file ({} bytes); first {}\\{} at offset {}, {} bytes",
            1 + past.count(),
            self.file_len,
            fnv_util::text::decode(&f.name),
            fnv_util::text::decode(&r.name),
            r.offset,
            r.stored_size()
        )))
    }

    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    pub fn entries(&self) -> impl Iterator<Item = Entry<'_>> {
        self.folders.iter().enumerate().flat_map(|(fi, f)| {
            f.files.iter().enumerate().map(move |(ri, r)| Entry { folder: f, record: r, folder_index: fi, file_index: ri })
        })
    }

    pub fn is_compressed(&self, r: &FileRecord) -> bool {
        (self.header.archive_flags & flags::COMPRESSED_DEFAULT != 0) ^ r.toggles_compression()
    }

    /// Whether each data block starts with the file's path: flag 0x100 in a version 104
    /// archive; never in a version 103 one, where 0x100 is set on every official archive
    /// with another meaning [documented: UESP "Oblivion Mod:BSA File Format" and "Skyrim
    /// Mod:Archive File Format"].
    pub fn embeds_names(&self) -> bool {
        self.header.version != VERSION_103 && self.header.archive_flags & flags::EMBED_FILE_NAMES != 0
    }

    /// The exact stored bytes of a file's data block (still compressed, with any
    /// embedded name prefix).
    pub fn read_raw(&mut self, r: &FileRecord) -> Result<Vec<u8>, BsaError> {
        let size = r.stored_size() as usize;
        if r.offset as u64 + size as u64 > self.file_len {
            return Err(BsaError::Truncated(format!("data for {} beyond end", fnv_util::text::decode(&r.name))));
        }
        let mut buf = vec![0u8; size];
        self.file.seek(SeekFrom::Start(r.offset as u64))?;
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Decoded file contents: embedded name stripped, decompressed if needed.
    pub fn extract(&mut self, r: &FileRecord) -> Result<Vec<u8>, BsaError> {
        self.extract_checked(r).map(|(d, _)| d)
    }

    /// Like [`Archive::extract`], also returning the zlib trailer status
    /// (`Verified` for uncompressed files and empty compressed entries).
    pub fn extract_checked(&mut self, r: &FileRecord) -> Result<(Vec<u8>, fnv_inflate::Trailer), BsaError> {
        self.read_stored(r)?.decode_checked()
    }

    /// The stored block of a file with what decoding it needs, read but not decoded:
    /// the only step that uses the archive's file handle. A caller sharing the archive
    /// behind a lock reads under the lock and decodes ([`StoredBlock::decode`]) after
    /// letting it go, so readers of one archive do not wait on each other's inflate.
    pub fn read_stored(&mut self, r: &FileRecord) -> Result<StoredBlock, BsaError> {
        Ok(StoredBlock {
            raw: self.read_raw(r)?,
            compressed: self.is_compressed(r),
            embedded_name: self.embeds_names(),
            name: r.name.clone(),
        })
    }

    /// Decode many files and summarise each decoded file with `f`, in parallel.
    /// Results are in the order of `recs` and equal to calling
    /// [`Archive::extract_checked`] on each. Stored blocks are read sequentially in
    /// batches of about 64 MiB, so memory stays bounded for large archives.
    pub fn decode_files<T: Send>(
        &mut self,
        recs: &[FileRecord],
        f: impl Fn(&[u8]) -> T + Sync,
    ) -> Vec<Result<(T, fnv_inflate::Trailer), BsaError>> {
        const BATCH_BYTES: usize = 64 << 20;
        let embeds = self.embeds_names();
        let mut out = Vec::with_capacity(recs.len());
        let mut rest = recs;
        while !rest.is_empty() {
            let mut batch = Vec::new();
            let mut bytes = 0usize;
            while let Some((r, tail)) = rest.split_first() {
                if !batch.is_empty() && bytes >= BATCH_BYTES {
                    break;
                }
                bytes += r.stored_size() as usize;
                batch.push((r, self.is_compressed(r), self.read_raw(r)));
                rest = tail;
            }
            let decoded: Vec<_> = batch
                .into_par_iter()
                .map(|(r, compressed, raw)| {
                    let (data, trailer) = decode_block_checked(&raw?, compressed, embeds, &fnv_util::text::decode(&r.name))?;
                    Ok((f(&data), trailer))
                })
                .collect();
            out.extend(decoded);
        }
        out
    }

    /// Look up a file by full path (case-insensitive, either slash).
    pub fn find(&self, path: &str) -> Option<(usize, usize)> {
        let p = path.replace('/', "\\");
        let (dir, name) = match p.rfind('\\') {
            Some(i) => (&p[..i], &p[i + 1..]),
            None => ("", &p[..]),
        };
        let dir_b = fnv_util::text::encode(dir)?;
        let name_b = fnv_util::text::encode(name)?;
        let dh = name_hash(&dir_b, false);
        let fh = name_hash(&name_b, true);
        let dir_lower: Vec<u8> = dir_b.iter().map(|b| b.to_ascii_lowercase()).collect();
        let name_lower: Vec<u8> = name_b.iter().map(|b| b.to_ascii_lowercase()).collect();
        for (fi, f) in self.folders.iter().enumerate() {
            if f.hash != dh {
                continue;
            }
            if !f.name.is_empty() && f.name.to_ascii_lowercase() != dir_lower {
                continue;
            }
            for (ri, r) in f.files.iter().enumerate() {
                if r.hash == fh && (r.name.is_empty() || r.name.to_ascii_lowercase() == name_lower) {
                    return Some((fi, ri));
                }
            }
        }
        None
    }
}

/// The start-up check of an archive the game will load: it opens, its header, folder and
/// file tables read, and every file's data lies inside the file. Headers and tables only;
/// nothing is decompressed (a file that fails to decompress later is that file's error).
pub fn check(path: impl AsRef<Path>) -> Result<(), BsaError> {
    Archive::open(path)?.check_tables()
}

/// One file's stored data block as read by [`Archive::read_stored`], not yet decoded.
#[derive(Debug, Clone)]
pub struct StoredBlock {
    pub raw: Vec<u8>,
    pub compressed: bool,
    pub embedded_name: bool,
    /// The file's name as stored (for error messages).
    pub name: Vec<u8>,
}

impl StoredBlock {
    /// Decoded contents, the same bytes as [`Archive::extract`].
    pub fn decode(&self) -> Result<Vec<u8>, BsaError> {
        self.decode_checked().map(|(d, _)| d)
    }

    /// How long the decoded contents say they are, without decoding: the size declared before
    /// a compressed stream ([`StoredBlock::decode`] gives no more), the stored bytes of an
    /// uncompressed one. `None` for a block too short to say, which does not decode either.
    pub fn contents_len(&self) -> Option<usize> {
        let name = if self.embedded_name { 1 + usize::from(*self.raw.first()?) } else { 0 };
        let body = self.raw.get(name..)?;
        if self.compressed {
            Some(u32::from_le_bytes(*body.first_chunk()?) as usize)
        } else {
            Some(body.len())
        }
    }

    /// Like [`StoredBlock::decode`], also returning the zlib trailer status.
    pub fn decode_checked(&self) -> Result<(Vec<u8>, fnv_inflate::Trailer), BsaError> {
        decode_block_checked(&self.raw, self.compressed, self.embedded_name, &fnv_util::text::decode(&self.name))
    }
}

/// Decode one stored data block.
pub fn decode_block(raw: &[u8], compressed: bool, embedded_name: bool, label: &str) -> Result<Vec<u8>, BsaError> {
    decode_block_checked(raw, compressed, embedded_name, label).map(|(d, _)| d)
}

/// Decode one stored data block, reporting zlib trailer anomalies instead of hiding them.
pub fn decode_block_checked(
    raw: &[u8],
    compressed: bool,
    embedded_name: bool,
    label: &str,
) -> Result<(Vec<u8>, fnv_inflate::Trailer), BsaError> {
    use fnv_inflate::Trailer;
    let mut body = raw;
    if embedded_name {
        let n = *body.first().ok_or_else(|| BsaError::Truncated(format!("{label}: empty block")))? as usize;
        if body.len() < 1 + n {
            return Err(BsaError::Truncated(format!("{label}: embedded name")));
        }
        body = &body[1 + n..];
    }
    if !compressed {
        return Ok((body.to_vec(), Trailer::Verified));
    }
    if body.len() < 4 {
        return Err(BsaError::Truncated(format!("{label}: compressed block shorter than 4 bytes")));
    }
    let orig = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    if orig > MAX_DECOMPRESSED_FILE {
        return Err(BsaError::Decompress { path: label.to_string(), err: fnv_inflate::InflateError::OutputLimit });
    }
    if orig == 0 && body.len() == 4 {
        // Observed: Fallout - Misc.bsa, menus\do_not_delete.txt.
        return Ok((Vec::new(), Trailer::Verified));
    }
    // The game data contains at least one zlib stream without an Adler-32 trailer
    // (Fallout - Misc.bsa, menus\s.txt) [observed]; accept it. A wrong checksum
    // with a clean stream of the exact expected size is also accepted, matching
    // the ESM reader's policy.
    fnv_inflate::zlib_decompress_ex(&body[4..], Some(orig))
        .map_err(|err| BsaError::Decompress { path: label.to_string(), err })
}

// --------------------------------------------------------------------------- writer

/// One file to write. `data` is the exact stored block (already compressed and
/// name-prefixed if applicable); use [`encode_block`] to produce it.
#[derive(Debug, Clone)]
pub struct WriteFile {
    pub name: Vec<u8>,
    pub size_flag_bits: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct WriteFolder {
    pub name: Vec<u8>,
    pub files: Vec<WriteFile>,
}

/// Build a stored data block from file contents.
pub fn encode_block(contents: &[u8], compress: bool, embed_path: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(p) = embed_path {
        out.push(p.len().min(255) as u8);
        out.extend_from_slice(&p[..p.len().min(255)]);
    }
    if compress {
        out.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        out.extend_from_slice(&fnv_inflate::zlib_store(contents));
    } else {
        out.extend_from_slice(contents);
    }
    out
}

/// Serialize an archive in canonical layout: folders sorted by hash, files sorted
/// by hash within each folder, data blocks in record order immediately after the
/// file name block. This is the layout observed in every shipped archive.
pub fn write_archive(
    out: &mut impl Write,
    archive_flags: u32,
    file_flags: u16,
    file_flags_padding: u16,
    folders: &[WriteFolder],
) -> Result<(), BsaError> {
    let meta: Vec<LayoutFolder> = folders
        .iter()
        .map(|f| LayoutFolder {
            name: f.name.clone(),
            files: f
                .files
                .iter()
                .map(|x| LayoutFile { name: x.name.clone(), size_flag_bits: x.size_flag_bits, len: x.data.len() as u64 })
                .collect(),
        })
        .collect();
    write_archive_streaming(out, archive_flags, file_flags, file_flags_padding, &meta, |fi, ri| {
        Ok(folders[fi].files[ri].data.clone())
    })
}

/// Name and stored length of one file, for [`write_archive_streaming`].
#[derive(Debug, Clone)]
pub struct LayoutFile {
    pub name: Vec<u8>,
    pub size_flag_bits: u32,
    pub len: u64,
}

#[derive(Debug, Clone)]
pub struct LayoutFolder {
    pub name: Vec<u8>,
    pub files: Vec<LayoutFile>,
}

/// Canonical writer that pulls each stored block from `fetch(folder, file)`
/// (indices into `folders` as given) only when it is written, so memory use is the
/// directory plus one file. `fetch` must return exactly `len` bytes.
pub fn write_archive_streaming<W: Write, F: FnMut(usize, usize) -> Result<Vec<u8>, BsaError>>(
    out: &mut W,
    archive_flags: u32,
    file_flags: u16,
    file_flags_padding: u16,
    folders: &[LayoutFolder],
    fetch: F,
) -> Result<(), BsaError> {
    let extras = LayoutExtras::default();
    write_archive_layout(out, archive_flags, file_flags, file_flags_padding, folders, &extras, fetch)
}

/// Departures from the canonical layout that a byte-identical rewrite must reproduce.
#[derive(Debug, Clone, Default)]
pub struct LayoutExtras {
    /// Bytes written after the last file name, inside the name block.
    pub name_block_tail: Vec<u8>,
    /// Order of the data blocks as `(folder, file)` indices into the `folders` given;
    /// `None` = record order. Must list every file exactly once.
    pub data_order: Option<Vec<(usize, usize)>>,
    /// The header's version; `None` = [`VERSION`] (104). A version 103 archive is
    /// rewritten as 103.
    pub version: Option<u32>,
}

/// [`write_archive_streaming`] with [`LayoutExtras`]. Records and names are always in
/// canonical (hash-sorted) order; blocks are packed contiguously in `data_order`.
pub fn write_archive_layout<W: Write, F: FnMut(usize, usize) -> Result<Vec<u8>, BsaError>>(
    out: &mut W,
    archive_flags: u32,
    file_flags: u16,
    file_flags_padding: u16,
    folders: &[LayoutFolder],
    extras: &LayoutExtras,
    mut fetch: F,
) -> Result<(), BsaError> {
    let mut order: Vec<usize> = (0..folders.len()).collect();
    order.sort_by_key(|&i| name_hash(&folders[i].name, false));
    let sorted_files: Vec<Vec<usize>> = order
        .iter()
        .map(|&fi| {
            let mut v: Vec<usize> = (0..folders[fi].files.len()).collect();
            v.sort_by_key(|&ri| name_hash(&folders[fi].files[ri].name, true));
            v
        })
        .collect();
    let has_dir = archive_flags & flags::DIR_NAMES != 0;
    let has_names = archive_flags & flags::FILE_NAMES != 0;
    let folder_count = folders.len() as u32;
    let file_count: u32 = folders.iter().map(|f| f.files.len() as u32).sum();
    let total_folder_name_length: u32 =
        if has_dir { folders.iter().map(|f| f.name.len() as u32 + 1).sum() } else { 0 };
    if !has_names && !extras.name_block_tail.is_empty() {
        return Err(BsaError::Layout("name block tail without a file name block".into()));
    }
    let total_file_name_length: u32 = if has_names {
        folders.iter().flat_map(|f| f.files.iter()).map(|x| x.name.len() as u32 + 1).sum::<u32>()
            + extras.name_block_tail.len() as u32
    } else {
        0
    };
    // Data order: canonical record order unless the caller preserves another one.
    let canonical: Vec<(usize, usize)> =
        order.iter().enumerate().flat_map(|(k, &fi)| sorted_files[k].iter().map(move |&ri| (fi, ri))).collect();
    let data_order: &[(usize, usize)] = match &extras.data_order {
        None => &canonical,
        Some(d) => {
            let mut seen: Vec<Vec<bool>> = folders.iter().map(|f| vec![false; f.files.len()]).collect();
            for &(fi, ri) in d {
                match seen.get_mut(fi).and_then(|v| v.get_mut(ri)) {
                    Some(x) if !*x => *x = true,
                    _ => return Err(BsaError::Layout("data_order is not a permutation of the files".into())),
                }
            }
            if d.len() != canonical.len() {
                return Err(BsaError::Layout("data_order is not a permutation of the files".into()));
            }
            d
        }
    };

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&MAGIC);
    for v in [extras.version.unwrap_or(VERSION), HEADER_SIZE, archive_flags, folder_count, file_count, total_folder_name_length, total_file_name_length] {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf.extend_from_slice(&file_flags.to_le_bytes());
    buf.extend_from_slice(&file_flags_padding.to_le_bytes());

    let records_start = HEADER_SIZE as usize + 16 * folders.len();
    let mut block_pos = Vec::with_capacity(folders.len());
    let mut p = records_start;
    for (k, &fi) in order.iter().enumerate() {
        block_pos.push(p);
        if has_dir {
            p += 1 + folders[fi].name.len() + 1;
        }
        p += 16 * sorted_files[k].len();
    }
    let names_start = p;
    let data_start = names_start + total_file_name_length as usize;
    for (k, &fi) in order.iter().enumerate() {
        let off = block_pos[k] as u64 + total_file_name_length as u64;
        if off > u32::MAX as u64 {
            return Err(BsaError::Layout("archive too large".into()));
        }
        buf.extend_from_slice(&name_hash(&folders[fi].name, false).to_le_bytes());
        buf.extend_from_slice(&(sorted_files[k].len() as u32).to_le_bytes());
        buf.extend_from_slice(&(off as u32).to_le_bytes());
    }
    let mut offsets: Vec<Vec<u64>> = folders.iter().map(|f| vec![0; f.files.len()]).collect();
    let mut data_off = data_start as u64;
    for &(fi, ri) in data_order {
        offsets[fi][ri] = data_off;
        data_off += folders[fi].files[ri].len;
    }
    for (k, &fi) in order.iter().enumerate() {
        let f = &folders[fi];
        if has_dir {
            if f.name.len() > 254 {
                return Err(BsaError::Layout("folder name too long".into()));
            }
            buf.push((f.name.len() + 1) as u8);
            buf.extend_from_slice(&f.name);
            buf.push(0);
        }
        for &ri in &sorted_files[k] {
            let x = &f.files[ri];
            let off = offsets[fi][ri];
            if x.len > SIZE_MASK as u64 || off > u32::MAX as u64 {
                return Err(BsaError::Layout("file too large".into()));
            }
            buf.extend_from_slice(&name_hash(&x.name, true).to_le_bytes());
            buf.extend_from_slice(&((x.len as u32) | (x.size_flag_bits & !SIZE_MASK)).to_le_bytes());
            buf.extend_from_slice(&(off as u32).to_le_bytes());
        }
    }
    if has_names {
        for (k, &fi) in order.iter().enumerate() {
            for &ri in &sorted_files[k] {
                buf.extend_from_slice(&folders[fi].files[ri].name);
                buf.push(0);
            }
        }
        buf.extend_from_slice(&extras.name_block_tail);
    }
    debug_assert_eq!(buf.len(), data_start);
    out.write_all(&buf)?;
    for &(fi, ri) in data_order {
        let d = fetch(fi, ri)?;
        if d.len() as u64 != folders[fi].files[ri].len {
            return Err(BsaError::Layout("fetched block length differs from declared length".into()));
        }
        out.write_all(&d)?;
    }
    Ok(())
}

/// Re-serialize an opened archive from its own stored blocks, streaming one file at
/// a time. Used by the golden-file round-trip test: output must equal the input
/// byte for byte.
pub fn rewrite(archive: &mut Archive, out: &mut impl Write) -> Result<(), BsaError> {
    let meta: Vec<LayoutFolder> = archive
        .folders
        .iter()
        .map(|f| LayoutFolder {
            name: f.name.clone(),
            files: f
                .files
                .iter()
                .map(|r| LayoutFile {
                    name: r.name.clone(),
                    size_flag_bits: r.size_field & !SIZE_MASK,
                    len: r.stored_size() as u64,
                })
                .collect(),
        })
        .collect();
    let h = archive.header.clone();
    let recs: Vec<Vec<FileRecord>> = archive.folders.iter().map(|f| f.files.clone()).collect();
    // Keep the archive's own block order and any bytes after the last name, so that
    // archives laid out differently from the canonical rule still rebuild identically.
    let mut data_order: Vec<(usize, usize)> =
        recs.iter().enumerate().flat_map(|(fi, f)| (0..f.len()).map(move |ri| (fi, ri))).collect();
    data_order.sort_by_key(|&(fi, ri)| recs[fi][ri].offset);
    let extras = LayoutExtras { name_block_tail: archive.name_block_tail.clone(), data_order: Some(data_order), version: Some(h.version) };
    write_archive_layout(out, h.archive_flags, h.file_flags, h.file_flags_padding, &meta, &extras, |fi, ri| {
        archive.read_raw(&recs[fi][ri])
    })
}

/// A `Write` sink that compares everything written against an existing file, so a
/// round-trip check needs no second copy in memory.
pub struct CompareWriter<R: Read> {
    original: R,
    pub written: u64,
    pub first_difference: Option<u64>,
    scratch: Vec<u8>,
}

impl<R: Read> CompareWriter<R> {
    pub fn new(original: R) -> Self {
        CompareWriter { original, written: 0, first_difference: None, scratch: Vec::new() }
    }

    /// True if everything matched and the original has no bytes left.
    pub fn finish(mut self) -> io::Result<(bool, Option<u64>)> {
        let mut extra = [0u8; 1];
        let more = self.original.read(&mut extra)? > 0;
        if more && self.first_difference.is_none() {
            self.first_difference = Some(self.written);
        }
        Ok((self.first_difference.is_none(), self.first_difference))
    }
}

impl<R: Read> Write for CompareWriter<R> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.first_difference.is_none() {
            self.scratch.resize(data.len(), 0);
            let mut got = 0;
            while got < data.len() {
                let n = self.original.read(&mut self.scratch[got..])?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            match data[..got].iter().zip(&self.scratch[..got]).position(|(a, b)| a != b) {
                Some(i) => self.first_difference = Some(self.written + i as u64),
                None if got < data.len() => self.first_difference = Some(self.written + got as u64),
                None => {}
            }
        }
        self.written += data.len() as u64;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_examples() {
        // Values observed in the user's Update.bsa / Fallout - Misc.bsa directories.
        assert_eq!(name_hash(b"tutorial_menu.xml", true), 0x04b1_d76e_740d_6e75);
        assert_eq!(name_hash(b"death.kf", true), 0x1776_fd25_6405_74e8);
        assert_eq!(name_hash(b"DEATH.KF", true), 0x1776_fd25_6405_74e8);
        assert_eq!(name_hash(b"dummy.dds", true), 0x8e50_c6fd_6405_edf9);
    }

    #[test]
    fn hash_short_names() {
        // Must not panic on 0-, 1- and 2-character stems.
        let _ = name_hash(b"", false);
        let _ = name_hash(b"a", false);
        let _ = name_hash(b"ab.nif", true);
        let _ = name_hash(b".nif", true);
        assert_eq!(name_hash(b"a/b", false), name_hash(b"A\\B", false));
    }

    fn sample() -> Vec<WriteFolder> {
        vec![
            WriteFolder {
                name: b"meshes\\test".to_vec(),
                files: vec![
                    WriteFile { name: b"b.nif".to_vec(), size_flag_bits: 0, data: encode_block(b"BBBB", false, None) },
                    WriteFile { name: b"a.nif".to_vec(), size_flag_bits: 0, data: encode_block(b"AAAAAA", false, None) },
                ],
            },
            WriteFolder {
                name: b"textures".to_vec(),
                files: vec![WriteFile {
                    name: b"c.dds".to_vec(),
                    size_flag_bits: SIZE_COMPRESSION_TOGGLE,
                    data: encode_block(&[7u8; 1000], true, None),
                }],
            },
        ]
    }

    #[test]
    fn write_read_roundtrip() {
        let dir = std::env::temp_dir().join(format!("fnv-bsa-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.bsa");
        let mut bytes = Vec::new();
        write_archive(&mut bytes, flags::DIR_NAMES | flags::FILE_NAMES, 0x3, 0, &sample()).unwrap();
        std::fs::write(&p, &bytes).unwrap();
        let mut a = Archive::open(&p).unwrap();
        assert!(a.warnings.is_empty(), "{:?}", a.warnings);
        assert_eq!(a.header.file_count, 3);
        let (fi, ri) = a.find("Meshes/Test/A.NIF").unwrap();
        let r = a.folders[fi].files[ri].clone();
        assert_eq!(a.extract(&r).unwrap(), b"AAAAAA");
        let (fi, ri) = a.find("textures\\c.dds").unwrap();
        let r = a.folders[fi].files[ri].clone();
        assert!(a.is_compressed(&r));
        assert_eq!(a.extract(&r).unwrap(), vec![7u8; 1000]);
        assert!(a.find("textures\\missing.dds").is_none());
        let mut again = Vec::new();
        rewrite(&mut a, &mut again).unwrap();
        assert_eq!(again, bytes);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// #2092: a version 103 archive (Oblivion's layout) reads as a version 104 one: names,
    /// uncompressed and compressed files, with and without compression by default, and
    /// rewrites byte-identically as 103. Flags 0x100, 0x200 and 0x400 (set on every
    /// official 103 archive) are known there, and 0x100 does not strip a path.
    #[test]
    fn version_103_archives_read_by_their_own_rules() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("old.bsa");
        let official = flags::EMBED_FILE_NAMES | flags::V103_0X200 | flags::V103_0X400;
        for default_compressed in [false, true] {
            let mut folders = sample();
            let mut archive_flags = flags::DIR_NAMES | flags::FILE_NAMES | official;
            if default_compressed {
                // Compressed by default: the two .nif files compressed, c.dds toggled off.
                archive_flags |= flags::COMPRESSED_DEFAULT;
                folders[0].files[0].data = encode_block(b"BBBB", true, None);
                folders[0].files[1].data = encode_block(b"AAAAAA", true, None);
                folders[1].files[0].data = encode_block(&[7u8; 1000], false, None);
            }
            let mut bytes = Vec::new();
            let extras = LayoutExtras { version: Some(VERSION_103), ..Default::default() };
            let meta: Vec<LayoutFolder> = folders
                .iter()
                .map(|f| LayoutFolder {
                    name: f.name.clone(),
                    files: f.files.iter().map(|x| LayoutFile { name: x.name.clone(), size_flag_bits: x.size_flag_bits, len: x.data.len() as u64 }).collect(),
                })
                .collect();
            write_archive_layout(&mut bytes, archive_flags, 0x3, 0, &meta, &extras, |fi, ri| Ok(folders[fi].files[ri].data.clone())).unwrap();
            assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 103);
            std::fs::write(&p, &bytes).unwrap();
            let mut a = Archive::open(&p).unwrap();
            assert!(a.warnings.is_empty(), "{:?}", a.warnings);
            assert_eq!((a.header.version, a.header.file_count), (103, 3));
            assert!(!a.embeds_names());
            for (path, want, compressed) in [("meshes\\test\\a.nif", b"AAAAAA".to_vec(), default_compressed), ("meshes\\test\\b.nif", b"BBBB".to_vec(), default_compressed), ("textures\\c.dds", vec![7u8; 1000], !default_compressed)] {
                let (fi, ri) = a.find(path).unwrap();
                let r = a.folders[fi].files[ri].clone();
                assert_eq!(a.is_compressed(&r), compressed, "{path}");
                assert_eq!(a.extract(&r).unwrap(), want, "{path}");
            }
            let mut again = Vec::new();
            rewrite(&mut a, &mut again).unwrap();
            assert_eq!(again, bytes, "rewritten as 103, byte for byte");
            assert!(check(&p).is_ok());
        }
        // The same flags in a version 104 archive: 0x200 and 0x400 are unknown there, and
        // 0x100 strips a path.
        let mut bytes = Vec::new();
        write_archive(&mut bytes, flags::DIR_NAMES | flags::FILE_NAMES | official, 0x3, 0, &sample()).unwrap();
        std::fs::write(&p, &bytes).unwrap();
        let a = Archive::open(&p).unwrap();
        assert!(a.embeds_names());
        assert!(a.warnings.iter().any(|w| w.code == "unknown_archive_flags" && w.detail == "0x600"), "{:?}", a.warnings);
    }

    #[test]
    fn reports_trailing_bytes_and_trailer_status() {
        let dir = std::env::temp_dir().join(format!("fnv-bsa-trail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.bsa");
        let mut bytes = Vec::new();
        write_archive(&mut bytes, flags::DIR_NAMES | flags::FILE_NAMES, 0, 0, &sample()).unwrap();
        bytes.extend_from_slice(&[0, 0, 0]);
        std::fs::write(&p, &bytes).unwrap();
        let mut a = Archive::open(&p).unwrap();
        assert!(a.warnings.iter().any(|w| w.code == "trailing_bytes"), "{:?}", a.warnings);
        let (fi, ri) = a.find("textures\\c.dds").unwrap();
        let r = a.folders[fi].files[ri].clone();
        assert_eq!(a.extract_checked(&r).unwrap().1, fnv_inflate::Trailer::Verified);
        // Streaming comparison detects the 3 extra bytes.
        let mut cmp = CompareWriter::new(std::fs::File::open(&p).unwrap());
        rewrite(&mut a, &mut cmp).unwrap();
        let (same, at) = cmp.finish().unwrap();
        assert!(!same);
        assert_eq!(at, Some(bytes.len() as u64 - 3));
        std::fs::remove_dir_all(&dir).ok();
    }

    mod malformed {
        use super::*;
        use proptest::prelude::*;
        use proptest::sample::Index;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(128))]

            /// Mutated archives open with an error or with warnings, every file
            /// extracts or reports an error, and an archive that opens without
            /// warnings and decodes cleanly is rewritten byte-for-byte.
            #[test]
            fn mutated_archives(
                edits in prop::collection::vec((any::<Index>(), any::<u8>()), 1..6),
                cut in prop::option::of(any::<Index>()),
            ) {
                let mut bytes = Vec::new();
                write_archive(&mut bytes, flags::DIR_NAMES | flags::FILE_NAMES, 0x3, 0, &sample()).unwrap();
                for (i, b) in &edits {
                    let at = i.index(bytes.len());
                    bytes[at] = *b;
                }
                if let Some(c) = cut {
                    bytes.truncate(c.index(bytes.len() + 1));
                }
                let f = tempfile::NamedTempFile::new().unwrap();
                std::fs::write(f.path(), &bytes).unwrap();
                if let Ok(mut a) = Archive::open(f.path()) {
                    let recs: Vec<FileRecord> = a.entries().map(|e| e.record.clone()).collect();
                    let par = a.decode_files(&recs, |d| d.to_vec());
                    let mut clean = a.warnings.is_empty();
                    for (r, p) in recs.iter().zip(&par) {
                        let one = a.extract_checked(r);
                        prop_assert_eq!(one.is_ok(), p.is_ok());
                        if let (Ok(x), Ok(y)) = (one, p) {
                            prop_assert_eq!(&x.0, &y.0);
                            clean &= x.1 == fnv_inflate::Trailer::Verified;
                        } else {
                            clean = false;
                        }
                    }
                    let mut again = Vec::new();
                    let rewritten = rewrite(&mut a, &mut again);
                    if clean {
                        rewritten.unwrap();
                        prop_assert_eq!(again, bytes);
                    }
                }
            }
        }
    }

    /// Layouts observed in shipped archives that differ from the canonical rule:
    /// blocks stored out of record order (Fallout - Textures.bsa) and bytes after the
    /// last file name (Fallout - Voices1.bsa). Both must rebuild byte-identically.
    #[test]
    fn noncanonical_layouts_roundtrip() {
        let folders = sample();
        let meta: Vec<LayoutFolder> = folders
            .iter()
            .map(|f| LayoutFolder {
                name: f.name.clone(),
                files: f
                    .files
                    .iter()
                    .map(|x| LayoutFile { name: x.name.clone(), size_flag_bits: x.size_flag_bits, len: x.data.len() as u64 })
                    .collect(),
            })
            .collect();
        let reversed: Vec<(usize, usize)> = vec![(1, 0), (0, 1), (0, 0)];
        for (extras, expect) in [
            (LayoutExtras { name_block_tail: vec![0; 5], data_order: None, version: None }, "name_block_slack"),
            (LayoutExtras { name_block_tail: vec![], data_order: Some(reversed.clone()), version: None }, "data_not_in_record_order"),
            (LayoutExtras { name_block_tail: b"xy\0".to_vec(), data_order: Some(reversed.clone()), version: None }, "name_block_slack"),
        ] {
            let mut bytes = Vec::new();
            write_archive_layout(&mut bytes, flags::DIR_NAMES | flags::FILE_NAMES, 0x3, 0, &meta, &extras, |fi, ri| {
                Ok(folders[fi].files[ri].data.clone())
            })
            .unwrap();
            let f = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(f.path(), &bytes).unwrap();
            let mut a = Archive::open(f.path()).unwrap();
            let codes: Vec<&str> = a.warnings.iter().map(|w| w.code).collect();
            assert!(codes.contains(&expect), "{codes:?}");
            assert!(!codes.contains(&"data_not_contiguous") && !codes.contains(&"trailing_bytes"), "{codes:?}");
            assert_eq!(a.name_block_tail, extras.name_block_tail);
            let (fi, ri) = a.find("textures\\c.dds").unwrap();
            let r = a.folders[fi].files[ri].clone();
            assert_eq!(a.extract(&r).unwrap(), vec![7u8; 1000]);
            let mut again = Vec::new();
            rewrite(&mut a, &mut again).unwrap();
            assert_eq!(again, bytes);
        }
        let bad = LayoutExtras { name_block_tail: vec![], data_order: Some(vec![(0, 0), (0, 0), (1, 0)]), version: None };
        assert!(write_archive_layout(&mut Vec::new(), 3, 0, 0, &meta, &bad, |_, _| Ok(vec![])).is_err());
    }

    #[test]
    fn the_start_up_check_names_what_is_wrong() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.bsa");
        let mut good = Vec::new();
        write_archive(&mut good, 3, 0, 0, &sample()).unwrap();
        let check_bytes = |bytes: &[u8]| {
            std::fs::write(&p, bytes).unwrap();
            check(&p)
        };
        assert!(check_bytes(&good).is_ok());
        // Missing file.
        assert!(matches!(check(dir.path().join("missing.bsa")), Err(BsaError::Io(_))));
        // Zero bytes, and a header cut short.
        let zero = check_bytes(&[]).unwrap_err();
        assert!(matches!(zero, BsaError::Truncated(_)) && zero.to_string().contains("0 bytes"), "{zero}");
        assert!(matches!(check_bytes(&good[..20]), Err(BsaError::Truncated(_))));
        // Bad magic.
        let mut bad = good.clone();
        bad[0] = b'X';
        assert!(matches!(check_bytes(&bad), Err(BsaError::NotBsa)));
        // Bad version (Skyrim SE's 105, Morrowind-era and unknown ones); Oblivion's 103
        // is read (#2092).
        for v in [105u32, 102, 0, 0x100] {
            let mut bad = good.clone();
            bad[4..8].copy_from_slice(&v.to_le_bytes());
            assert!(matches!(check_bytes(&bad), Err(BsaError::UnsupportedVersion(x)) if x == v));
        }
        // Folder and file tables longer than the file: a folder count of a million, and
        // the folder table offset past the end.
        let mut bad = good.clone();
        bad[16..20].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert!(matches!(check_bytes(&bad), Err(BsaError::Truncated(_))));
        let mut bad = good.clone();
        bad[8..12].copy_from_slice(&(good.len() as u32 + 100).to_le_bytes());
        assert!(matches!(check_bytes(&bad), Err(BsaError::Truncated(_))));
        // A folder record claiming more files than the tables hold.
        let mut bad = good.clone();
        bad[36 + 8..36 + 12].copy_from_slice(&50_000u32.to_le_bytes());
        assert!(check_bytes(&bad).is_err());
        // Tables intact, data cut short: a file record points past the end. It still
        // opens (the files inside the file read) but the check refuses it.
        let cut = &good[..good.len() - 10];
        std::fs::write(&p, cut).unwrap();
        assert!(Archive::open(&p).is_ok());
        let e = check(&p).unwrap_err();
        assert!(matches!(e, BsaError::Layout(_)) && e.to_string().contains("textures\\c.dds"), "{e}");
        // A file offset field pointing far past the end.
        let a = Archive::open({ std::fs::write(&p, &good).unwrap(); &p }).unwrap();
        let first = &a.folders[0].files[0];
        let at = good.windows(4).position(|w| w == first.offset.to_le_bytes()).unwrap();
        let mut bad = good.clone();
        bad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(check_bytes(&bad), Err(BsaError::Layout(_))));
    }

    #[test]
    fn rejects_garbage() {
        let dir = std::env::temp_dir().join(format!("fnv-bsa-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.bsa");
        std::fs::write(&p, b"not an archive at all, definitely not 36 bytes long..").unwrap();
        assert!(matches!(Archive::open(&p), Err(BsaError::NotBsa)));
        let mut bytes = Vec::new();
        write_archive(&mut bytes, 3, 0, 0, &sample()).unwrap();
        std::fs::write(&p, &bytes[..60]).unwrap();
        assert!(matches!(Archive::open(&p), Err(BsaError::Truncated(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bad_compressed_blocks_are_errors() {
        let block = |declared: u32, stream: &[u8]| {
            let mut b = declared.to_le_bytes().to_vec();
            b.extend_from_slice(stream);
            decode_block_checked(&b, true, false, "x")
        };
        let z = fnv_inflate::zlib_store(b"abc");
        assert_eq!(block(3, &z).unwrap().0, b"abc");
        // A declared size over the cap is refused before anything is inflated.
        let over = block(MAX_DECOMPRESSED_FILE as u32 + 1, &z).unwrap_err();
        assert!(matches!(over, BsaError::Decompress { err: fnv_inflate::InflateError::OutputLimit, .. }));
        assert!(block(u32::MAX, &z).is_err());
        assert!(block(4, &z).is_err());
        assert!(block(3, b"garbage!").is_err());
        assert!(block(3, &z[..4]).is_err());
        assert!(decode_block_checked(&[1, 2], true, false, "x").is_err());
        // An embedded name longer than the block.
        assert!(decode_block_checked(&[200, b'a'], false, true, "x").is_err());
    }
}
