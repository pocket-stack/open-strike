//! Cooked-map discovery and reusable-buffer loading for the Vita VPK.
//!
//! The shipping root is `app0:/maps`. Tests and development hosts may prepend
//! additional roots, but the first root containing any `.p3d` files wins so a
//! catalogue can never accidentally mix versions from two packages.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// A map allocation whose base is guaranteed 16-byte aligned for zero-copy
/// vertex/index consumers. `Vec<u8>` is commonly over-aligned by malloc but
/// does not promise it; using u128 makes the renderer contract explicit.
#[derive(Default)]
pub struct AlignedMapBuffer {
    words: Vec<u128>,
    len: usize,
}

impl AlignedMapBuffer {
    pub fn with_capacity(bytes: usize) -> Self {
        Self {
            words: vec![0; bytes.div_ceil(16)],
            len: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: `u8` accepts every bit pattern; `len` is bounded by the
        // initialized `words` allocation in `read_file`.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), self.len) }
    }

    fn read_file(&mut self, mut file: File, required_capacity: usize) -> io::Result<&[u8]> {
        let words = required_capacity.div_ceil(16);
        if self.words.len() < words {
            self.words.resize(words, 0);
        }
        let capacity = self.words.len() * 16;
        // SAFETY: this exposes the fully initialized u128 allocation as bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast::<u8>(), capacity)
        };
        let mut offset = 0usize;
        while offset < bytes.len() {
            let count = file.read(&mut bytes[offset..])?;
            if count == 0 {
                self.len = offset;
                return Ok(self.as_bytes());
            }
            offset += count;
        }
        let mut overflow = [0u8; 1];
        if file.read(&mut overflow)? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "map grew beyond the catalogue allocation",
            ));
        }
        self.len = offset;
        Ok(self.as_bytes())
    }
}

#[derive(Clone, Debug)]
pub struct MapCatalogue {
    root: PathBuf,
    names: Vec<String>,
    largest_bytes: usize,
}

impl MapCatalogue {
    pub fn scan(roots: impl IntoIterator<Item = impl AsRef<Path>>) -> io::Result<Self> {
        let mut last_error = None;
        for candidate in roots {
            let root = candidate.as_ref();
            let entries = match fs::read_dir(root) {
                Ok(entries) => entries,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };

            let mut names = Vec::new();
            let mut largest_bytes = 0usize;
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
                    continue;
                };
                if !extension.eq_ignore_ascii_case("p3d") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                names.push(stem.to_ascii_lowercase());
                if let Ok(metadata) = entry.metadata() {
                    largest_bytes = largest_bytes.max(metadata.len() as usize);
                }
            }
            names.sort();
            names.dedup();
            if !names.is_empty() {
                return Ok(Self {
                    root: root.to_owned(),
                    names,
                    largest_bytes,
                });
            }
        }

        Err(last_error
            .unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no cooked maps found")))
    }

    pub fn vita() -> io::Result<Self> {
        Self::scan([Path::new("app0:/maps")])
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn largest_bytes(&self) -> usize {
        self.largest_bytes
    }

    /// Read one catalogue member into a caller-owned allocation that can be
    /// reused on the next map. The returned slice is valid until that reload.
    pub fn load<'a>(&self, name: &str, buffer: &'a mut AlignedMapBuffer) -> io::Result<&'a [u8]> {
        let normalized = name.to_ascii_lowercase();
        if self.names.binary_search(&normalized).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "map is not in the catalogue",
            ));
        }

        let path = self.root.join(format!("{normalized}.p3d"));
        buffer.read_file(File::open(path)?, self.largest_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("openstrike-vita-{name}-{}", std::process::id()))
    }

    #[test]
    fn first_non_empty_root_wins_and_names_are_normalized() {
        let empty = scratch("empty");
        let maps = scratch("maps");
        let _ = fs::remove_dir_all(&empty);
        let _ = fs::remove_dir_all(&maps);
        fs::create_dir_all(&empty).unwrap();
        fs::create_dir_all(&maps).unwrap();
        fs::write(maps.join("DE_DUST2.P3D"), [1, 2, 3]).unwrap();
        fs::write(maps.join("readme.txt"), [9]).unwrap();

        let catalogue = MapCatalogue::scan([&empty, &maps]).unwrap();
        assert_eq!(catalogue.names(), &["de_dust2"]);
        assert_eq!(catalogue.largest_bytes(), 3);

        // VPK paths are staged lowercase. Exercise loading with a matching
        // lowercase file while preserving the uppercase scan assertion.
        fs::rename(maps.join("DE_DUST2.P3D"), maps.join("de_dust2.p3d")).unwrap();
        let mut buffer = AlignedMapBuffer::default();
        assert_eq!(catalogue.load("DE_DUST2", &mut buffer).unwrap(), [1, 2, 3]);
        assert_eq!((buffer.as_bytes().as_ptr() as usize) % 16, 0);

        let _ = fs::remove_dir_all(&empty);
        let _ = fs::remove_dir_all(&maps);
    }

    #[test]
    fn rejects_names_outside_the_catalogue() {
        let maps = scratch("traversal");
        let _ = fs::remove_dir_all(&maps);
        fs::create_dir_all(&maps).unwrap();
        fs::write(maps.join("de_dust2.p3d"), [1]).unwrap();
        let catalogue = MapCatalogue::scan([&maps]).unwrap();
        let error = catalogue
            .load("../secret", &mut AlignedMapBuffer::default())
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(&maps);
    }
}
