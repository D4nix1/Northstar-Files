use std::{
    fs,
    fs::OpenOptions,
    io,
    path::{Path, PathBuf},
};

/// Copies a file or directory tree without following symlinks or replacing an
/// existing destination. If a directory copy fails, its partial tree is removed.
pub fn copy_path(source: &Path, destination: &Path) -> io::Result<()> {
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "copying symbolic links is not supported",
        ));
    }
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        let result = (|| {
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                copy_path(&entry.path(), &destination.join(entry.file_name()))?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(destination);
        }
        result
    } else {
        let mut input = fs::File::open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        if let Err(error) = io::copy(&mut input, &mut output) {
            let _ = fs::remove_file(destination);
            return Err(error);
        }
        fs::set_permissions(destination, metadata.permissions())
    }
}

/// Renames an item in its current parent folder and returns the new path.
pub fn rename_path(source: &Path, new_name: &str) -> io::Result<PathBuf> {
    let name = Path::new(new_name);
    if new_name.trim().is_empty()
        || name.file_name().and_then(|part| part.to_str()) != Some(new_name)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid file name",
        ));
    }
    let parent = source.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "item has no parent directory")
    })?;
    let destination = parent.join(new_name);
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "an item with that name already exists",
        ));
    }
    fs::rename(source, &destination)?;
    Ok(destination)
}
