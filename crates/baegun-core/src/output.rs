use crate::errors::{BaegunError, Result};
use same_file::Handle;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use tempfile::{Builder, NamedTempFile};

pub(crate) fn open_source_distinct_from_destination(
    source_path: &Path,
    destination_path: &Path,
) -> Result<(File, Handle)> {
    let source_file = File::open(source_path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            BaegunError::bad_args(format!(
                "Input file does not exist: {}",
                source_path.display()
            ))
        } else {
            BaegunError::internal(format!(
                "Failed opening input file '{}': {error}",
                source_path.display()
            ))
        }
    })?;

    let metadata = source_file.metadata().map_err(|error| {
        BaegunError::internal(format!(
            "Failed reading input metadata '{}': {error}",
            source_path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(BaegunError::bad_args(format!(
            "Input path is not a file: {}",
            source_path.display()
        )));
    }

    let source_identity = Handle::from_file(source_file.try_clone().map_err(|error| {
        BaegunError::internal(format!(
            "Failed cloning input handle '{}': {error}",
            source_path.display()
        ))
    })?)
    .map_err(|error| {
        BaegunError::internal(format!(
            "Failed identifying input file '{}': {error}",
            source_path.display()
        ))
    })?;

    ensure_destination_is_distinct(&source_identity, source_path, destination_path)?;
    Ok((source_file, source_identity))
}

pub(crate) fn ensure_destination_is_distinct(
    source_identity: &Handle,
    source_path: &Path,
    destination_path: &Path,
) -> Result<()> {
    let destination_identity = match Handle::from_path(destination_path) {
        Ok(identity) => identity,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(BaegunError::bad_args(format!(
                "Cannot safely verify output path '{}': {error}",
                destination_path.display()
            )))
        }
    };

    if source_identity == &destination_identity {
        return Err(BaegunError::bad_args(format!(
            "Input '{}' and output '{}' identify the same filesystem file",
            source_path.display(),
            destination_path.display()
        )));
    }

    Ok(())
}

pub(crate) struct AtomicOutput {
    destination: PathBuf,
    temporary: NamedTempFile,
}

impl AtomicOutput {
    pub(crate) fn create(destination: &Path) -> Result<Self> {
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| {
            BaegunError::epub(format!(
                "Failed creating output directory '{}': {error}",
                parent.display()
            ))
        })?;

        let mut suffix = OsString::from(".");
        suffix.push(
            destination
                .extension()
                .unwrap_or_else(|| std::ffi::OsStr::new("tmp")),
        );
        let temporary = Builder::new()
            .prefix(".baegun-")
            .suffix(&suffix)
            .tempfile_in(parent)
            .map_err(|error| {
                BaegunError::epub(format!(
                    "Failed creating temporary EPUB in '{}': {error}",
                    parent.display()
                ))
            })?;

        Ok(Self {
            destination: destination.to_path_buf(),
            temporary,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        self.temporary.path()
    }

    pub(crate) fn file_mut(&mut self) -> &mut File {
        self.temporary.as_file_mut()
    }

    pub(crate) fn sync(&self) -> Result<()> {
        self.temporary.as_file().sync_all().map_err(|error| {
            BaegunError::epub(format!(
                "Failed syncing temporary EPUB '{}': {error}",
                self.path().display()
            ))
        })
    }

    pub(crate) fn publish(self) -> Result<()> {
        self.temporary
            .persist(&self.destination)
            .map(|_| ())
            .map_err(|error| {
                BaegunError::epub(format!(
                    "Failed atomically publishing EPUB '{}': {}",
                    self.destination.display(),
                    error.error
                ))
            })
    }
}
