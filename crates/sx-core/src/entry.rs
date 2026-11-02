/// What a directory entry is. Stored as one byte per entry in the directory model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum EntryKind {
    #[default]
    File,
    Dir,
    /// Symbolic link to a file.
    Symlink,
    /// Directory symbolic link or junction (mount point); navigable like a folder.
    Junction,
}

impl EntryKind {
    /// Whether opening the entry navigates into it (folders-first sorting uses the same rule).
    pub fn is_container(self) -> bool {
        matches!(self, Self::Dir | Self::Junction)
    }
}
