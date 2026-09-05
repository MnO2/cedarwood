//! Standalone, correctness-checked comparison of raw File I/O and the buffered path helpers.
//! Build/run commands and measurement limits are recorded in benches/results/2026-09-05-persistence-io.md.

use cedarwood::Cedar;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn create_scratch_dir(parent: &Path) -> io::Result<PathBuf> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    for attempt in 0..1000 {
        let path = parent.join(format!("cedarwood-persistence-io-{}-{attempt}", std::process::id()));
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a private benchmark directory",
    ))
}

fn main() {
    let entries: Vec<_> = (0..10_000).map(|value| (format!("key-{value:08}"), value)).collect();
    let borrowed: Vec<_> = entries.iter().map(|(key, value)| (key.as_str(), *value)).collect();
    let cedar = Cedar::from_sorted(&borrowed).unwrap();
    // Atomically claim a directory before creating/truncating files. Existing paths, including
    // symlinks, are never reused; Unix permissions protect the files in a shared temporary root.
    let scratch = create_scratch_dir(&std::env::temp_dir()).unwrap();
    let path = scratch.join("trie.bin");
    let mut expected = Vec::new();
    cedar.save_to_writer(&mut expected).unwrap();
    println!("entries={} file_bytes={}", cedar.len(), expected.len());

    for round in 0..3 {
        // Alternate which implementation goes first, while keeping construction and verification
        // outside the timed sections. Raw File reproduces the former path-helper implementation.
        for buffered in [round % 2 != 0, round % 2 == 0] {
            let start = Instant::now();
            if buffered {
                cedar.save_to_path(&path).unwrap();
            } else {
                cedar.save_to_writer(File::create(&path).unwrap()).unwrap();
            }
            let save = start.elapsed();
            assert_eq!(std::fs::read(&path).unwrap(), expected);

            let start = Instant::now();
            let loaded = if buffered {
                Cedar::load_from_path(&path).unwrap()
            } else {
                Cedar::load_from_reader(File::open(&path).unwrap()).unwrap()
            };
            let load = start.elapsed();
            assert!(loaded.entries().eq(cedar.entries()));
            println!(
                "round={round} mode={} save_ms={:.3} load_ms={:.3}",
                if buffered { "buffered" } else { "raw" },
                save.as_secs_f64() * 1000.0,
                load.as_secs_f64() * 1000.0
            );
        }
    }
    fs::remove_file(path).unwrap();
    fs::remove_dir(scratch).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_directory_preserves_existing_files() {
        let parent = create_scratch_dir(&std::env::temp_dir()).unwrap();
        let occupied = parent.join(format!("cedarwood-persistence-io-{}-0", std::process::id()));
        fs::write(&occupied, b"preserve this file").unwrap();

        let scratch = create_scratch_dir(&parent).unwrap();
        assert_ne!(scratch, occupied);
        assert_eq!(fs::read(&occupied).unwrap(), b"preserve this file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&scratch).unwrap().permissions().mode() & 0o777, 0o700);
        }

        fs::remove_dir(scratch).unwrap();
        fs::remove_file(occupied).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn scratch_directory_preserves_existing_symlinks() {
        let parent = create_scratch_dir(&std::env::temp_dir()).unwrap();
        let target = parent.join("owned-target");
        fs::create_dir(&target).unwrap();
        let occupied = parent.join(format!("cedarwood-persistence-io-{}-0", std::process::id()));
        std::os::unix::fs::symlink(&target, &occupied).unwrap();

        let scratch = create_scratch_dir(&parent).unwrap();
        assert_ne!(scratch, occupied);
        assert_eq!(fs::read_link(&occupied).unwrap(), target);
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);

        fs::remove_dir(scratch).unwrap();
        fs::remove_file(occupied).unwrap();
        fs::remove_dir(target).unwrap();
        fs::remove_dir(parent).unwrap();
    }
}
