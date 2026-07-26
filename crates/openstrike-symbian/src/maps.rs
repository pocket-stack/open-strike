//! App-private cooked-map loading through Symbian/Open C.
//!
//! The generated catalogue embeds names and NUL-terminated paths only. Map
//! bytes live on mass storage and one allocation is reused after the previous
//! renderer has released every view into it.

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_long, c_void};

pub(crate) struct MapCatalogEntry {
    pub(crate) name: &'static str,
    pub(crate) path: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/symbian_map_catalog.rs"));

#[repr(C)]
struct CFile {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut CFile;
    fn fseek(file: *mut CFile, offset: c_long, origin: c_int) -> c_int;
    fn ftell(file: *mut CFile) -> c_long;
    fn fread(buffer: *mut c_void, size: usize, count: usize, file: *mut CFile) -> usize;
    fn fclose(file: *mut CFile) -> c_int;
}

const SEEK_SET: c_int = 0;
const SEEK_END: c_int = 2;
const READ_BINARY: &[u8] = b"rb\0";

struct FileHandle(*mut CFile);

impl Drop for FileHandle {
    fn drop(&mut self) {
        // SAFETY: FileHandle is constructed only from a successful fopen and
        // owns that handle until this single Drop call.
        unsafe {
            fclose(self.0);
        }
    }
}

/// Reusable storage whose base alignment is part of its type.
#[derive(Default)]
pub(crate) struct AlignedMapBuffer {
    words: Vec<u64>,
    offset: usize,
    len: usize,
}

impl AlignedMapBuffer {
    /// Read one generated catalogue path. The returned view remains valid
    /// until the next `load` or until this buffer is dropped.
    pub(crate) unsafe fn load(&mut self, entry: &MapCatalogEntry) -> Result<&[u8], &'static str> {
        if entry.path.last() != Some(&0) || entry.path[..entry.path.len() - 1].contains(&0) {
            return Err("invalid map catalogue path");
        }

        let raw = fopen(
            entry.path.as_ptr().cast(),
            READ_BINARY.as_ptr().cast::<c_char>(),
        );
        if raw.is_null() {
            return Err("map file missing");
        }
        let file = FileHandle(raw);
        if fseek(file.0, 0, SEEK_END) != 0 {
            return Err("map seek failed");
        }
        let length = ftell(file.0);
        if length <= 0 {
            return Err("map file is empty");
        }
        let length = usize::try_from(length).map_err(|_| "map file is too large")?;
        // Cooked P3D sections require a 16-byte base. Symbian malloc only
        // promises 8-byte alignment, so retain Vec<u64> ownership but reserve
        // a full 16 bytes of headroom and align the exposed start manually.
        let allocation_bytes = length
            .checked_add(16)
            .ok_or("map file is too large")?
            .checked_add(core::mem::size_of::<u64>() - 1)
            .ok_or("map file is too large")?;
        let words = allocation_bytes / core::mem::size_of::<u64>();

        // No world may borrow this allocation when load is called. Resize may
        // therefore grow/reallocate it safely before the new static view is
        // created by the caller.
        self.offset = 0;
        self.len = 0;
        self.words.resize(words, 0);
        let base = self.words.as_mut_ptr().cast::<u8>();
        let offset = base.align_offset(16);
        if offset == usize::MAX {
            return Err("map alignment allocation failed");
        }
        let aligned = base.add(offset);
        self.offset = offset;
        if self.offset + length > self.words.len() * core::mem::size_of::<u64>() {
            return Err("map alignment allocation failed");
        }
        if fseek(file.0, 0, SEEK_SET) != 0 {
            return Err("map rewind failed");
        }
        let read = fread(aligned.cast(), 1, length, file.0);
        if read != length {
            return Err("map read failed");
        }
        self.len = length;
        Ok(self.as_bytes())
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: u8 accepts every initialized u64 bit pattern, and len is
        // bounded by the resized words allocation after the aligned offset.
        unsafe {
            core::slice::from_raw_parts(self.words.as_ptr().cast::<u8>().add(self.offset), self.len)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_catalogue_is_sorted_safe_and_nul_terminated() {
        assert_eq!(MAP_CATALOG.len(), 8);
        assert!(MAP_CATALOG
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name));
        for entry in MAP_CATALOG {
            assert!(!entry.name.is_empty());
            assert_eq!(entry.path.last(), Some(&0));
            assert!(!entry.path[..entry.path.len() - 1].contains(&0));
        }
    }

    #[test]
    fn buffer_alignment_is_explicit() {
        let mut buffer = AlignedMapBuffer::default();
        buffer.words.resize(3, 0);
        let base = buffer.words.as_mut_ptr().cast::<u8>();
        buffer.offset = base.align_offset(16);
        buffer.len = 1;
        assert_eq!((buffer.as_bytes().as_ptr() as usize) % 16, 0);
    }
}
