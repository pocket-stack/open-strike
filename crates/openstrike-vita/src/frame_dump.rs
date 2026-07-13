//! Native-resolution RGBA dumps consumed by `scripts/e2e-vita.ts`.

use std::fs;
use std::io;
use std::path::Path;

pub const WIDTH: usize = 960;
pub const HEIGHT: usize = 544;
pub const BYTES_PER_FRAME: usize = WIDTH * HEIGHT * 4;

pub fn write_rgba(root: &Path, index: u32, pixels: &[u8]) -> io::Result<()> {
    if pixels.len() != BYTES_PER_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "expected {BYTES_PER_FRAME} RGBA bytes for {WIDTH}x{HEIGHT}, got {}",
                pixels.len()
            ),
        ));
    }
    fs::create_dir_all(root)?;
    fs::write(root.join(format!("f{index:04}.rgba")), pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_size_before_touching_the_filesystem() {
        let root =
            std::env::temp_dir().join(format!("openstrike-vita-frame-dump-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let error = write_rgba(&root, 0, &[0; 4]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!root.exists());
    }
}
