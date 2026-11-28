//! [`Vfs::read_at_most`] at, under and over each of its limits, on archives and loose files
//! made here.
use super::Vfs;
use fnv_bsa::{encode_block, flags, write_archive, WriteFile, WriteFolder, SIZE_COMPRESSION_TOGGLE};
use std::io::ErrorKind;
use std::path::PathBuf;

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = std::env::temp_dir().join(format!("nvf-vfs-at-most-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("meshes")).unwrap();
        Folder(dir)
    }

    /// An archive of these files under `meshes`, with their paths before their data or not.
    fn archive(&self, embed: bool, files: Vec<(&str, u32, Vec<u8>)>) -> Vfs {
        let files = files.into_iter().map(|(name, size_flag_bits, data)| WriteFile { name: name.as_bytes().to_vec(), size_flag_bits, data }).collect();
        let named = if embed { flags::EMBED_FILE_NAMES } else { 0 };
        let mut bytes = Vec::new();
        write_archive(&mut bytes, flags::DIR_NAMES | flags::FILE_NAMES | named, 0x1, 0, &[WriteFolder { name: b"meshes".to_vec(), files }]).unwrap();
        std::fs::write(self.0.join("A.bsa"), bytes).unwrap();
        self.open()
    }

    fn open(&self) -> Vfs {
        Vfs::open(&self.0, &["A.bsa".to_string()]).unwrap()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn error(vfs: &Vfs, path: &str, stored: usize, contents: usize) -> (ErrorKind, String) {
    let e = vfs.read_at_most(path, stored, contents).unwrap().unwrap_err();
    (e.kind(), e.to_string())
}

/// An embedded name counts as stored and not as contents: a limit taken for the other fails.
#[test]
fn an_archived_file_is_refused_by_its_stored_and_by_its_declared_size() {
    for embed in [false, true] {
        let dir = Folder::new(if embed { "named" } else { "plain" });
        let contents: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let name = |n: &str| embed.then(|| format!("meshes\\{n}").into_bytes());
        let packed = encode_block(&contents, true, name("packed.nif").as_deref());
        let flat = encode_block(&contents, false, name("flat.nif").as_deref());
        let sizes = [("packed.nif", packed.len()), ("flat.nif", flat.len())];
        let vfs = dir.archive(embed, vec![("packed.nif", SIZE_COMPRESSION_TOGGLE, packed), ("flat.nif", 0, flat)]);
        for (n, stored) in sizes {
            let path = format!("Meshes/{n}");
            assert_eq!(vfs.read_at_most(&path, stored, 1000).unwrap().unwrap(), contents, "{n} at both limits");
            let says = format!("meshes\\{n}: {stored} bytes stored, more than the {} allowed", stored - 1);
            assert_eq!(error(&vfs, &path, stored - 1, 1000), (ErrorKind::FileTooLarge, says));
            let says = format!("meshes\\{n}: 1000 bytes decoded, more than the 999 allowed");
            assert_eq!(error(&vfs, &path, stored, 999), (ErrorKind::FileTooLarge, says));
        }
        assert!(vfs.read_at_most("meshes\\none.nif", 1, 1).is_none());
    }
}

/// A stored size over the limit is refused before the block is read: this one ends past the
/// archive, so reading it first is another error.
#[test]
fn a_stored_size_is_looked_at_before_the_block_is_read() {
    let dir = Folder::new("cut");
    drop(dir.archive(false, vec![("cut.nif", 0, vec![7; 4096])]));
    let file = dir.0.join("A.bsa");
    let whole = std::fs::read(&file).unwrap();
    std::fs::write(&file, &whole[..whole.len() - 1]).unwrap();
    let vfs = dir.open();
    assert_eq!(error(&vfs, "meshes/cut.nif", 4095, 4096).0, ErrorKind::FileTooLarge);
    let (kind, says) = error(&vfs, "meshes/cut.nif", 4096, 4096);
    assert!(kind != ErrorKind::FileTooLarge && says.starts_with("meshes\\cut.nif: "), "{says}");
}

/// A declared size over the limit is refused before anything is inflated: the stream here is
/// garbage, so inflating first is another error. One that holds more than it declares is that
/// error, whatever the limit, and so is one too short to declare anything.
#[test]
fn a_declared_size_is_looked_at_before_the_block_is_inflated() {
    let dir = Folder::new("declared");
    let mut bomb = 0x4000_0000u32.to_le_bytes().to_vec();
    bomb.extend([0xFF; 16]);
    let mut more = encode_block(&[5; 300], true, None);
    more[..4].copy_from_slice(&200u32.to_le_bytes());
    let vfs = dir.archive(false, vec![("bomb.nif", SIZE_COMPRESSION_TOGGLE, bomb), ("more.nif", SIZE_COMPRESSION_TOGGLE, more), ("stub.nif", SIZE_COMPRESSION_TOGGLE, vec![1, 2])]);
    let (kind, says) = error(&vfs, "meshes/bomb.nif", 4096, 1 << 20);
    assert!(kind == ErrorKind::FileTooLarge && says.contains("1073741824 bytes decoded"), "{says}");
    for n in ["more.nif", "stub.nif"] {
        let (kind, says) = error(&vfs, &format!("meshes/{n}"), 4096, 250);
        assert!(kind != ErrorKind::FileTooLarge && says.starts_with(&format!("meshes\\{n}: ")), "{says}");
    }
}

/// A loose file's length is the opened file's, not the listed one's; either limit is its limit.
#[test]
fn a_loose_file_is_refused_by_its_length() {
    let dir = Folder::new("loose");
    let file = dir.0.join("meshes/Loose.nif");
    std::fs::write(&file, [1; 10]).unwrap();
    std::fs::write(dir.0.join("meshes/empty.nif"), []).unwrap();
    let vfs = Vfs::open(&dir.0, &[]).unwrap();
    std::fs::write(&file, [2; 100]).unwrap();
    for (stored, contents) in [(100, 100), (100, usize::MAX), (usize::MAX, 100)] {
        assert_eq!(vfs.read_at_most("meshes\\loose.nif", stored, contents).unwrap().unwrap(), [2; 100]);
    }
    for (stored, contents) in [(99, 100), (100, 99)] {
        let says = "meshes\\loose.nif: 100 bytes, more than the 99 allowed".to_string();
        assert_eq!(error(&vfs, "Meshes/loose.nif", stored, contents), (ErrorKind::FileTooLarge, says));
    }
    assert!(vfs.read_at_most("meshes/empty.nif", 0, 0).unwrap().unwrap().is_empty());
    std::fs::remove_file(&file).unwrap();
    let (kind, says) = error(&vfs, "meshes/loose.nif", 100, 100);
    assert!(kind == ErrorKind::NotFound && says.starts_with("meshes\\loose.nif: "), "{says}");
}

/// A file that is longer (or shorter) when it is read than its length said when it was opened:
/// at most one byte past that is read, and none of one over the limit.
#[test]
fn a_file_that_grows_after_its_length_is_taken_is_not_read_on() {
    /// The bytes it still has, and those it gave.
    struct File(usize, usize);
    impl std::io::Read for File {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            let n = b.len().min(self.0);
            (self.0, self.1) = (self.0 - n, self.1 + n);
            Ok(n)
        }
    }
    for (holds, len, most, gave, kind) in [
        (1 << 20, 1000, 1000, 1001, Some(ErrorKind::FileTooLarge)),
        (1 << 20, 1000, 4000, 1001, Some(ErrorKind::Other)),
        (1 << 20, 1001, 1000, 0, Some(ErrorKind::FileTooLarge)),
        (1000, 1000, 1000, 1000, None),
        (0, 0, 0, 0, None),
        (600, 1000, 1000, 600, None),
    ] {
        let mut file = File(holds, 0);
        let got = super::take_at_most(&mut file, len, most);
        assert_eq!((file.1, got.as_ref().err().map(|e| e.kind())), (gave, kind));
        // Room was made once, for the length.
        assert!(got.is_err() || got.is_ok_and(|b| b.len() == holds && b.capacity() <= most));
    }
}
