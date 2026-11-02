use std::fmt;
use std::path::{Component, Path, PathBuf, Prefix};

/// Where a tab points: a file-system folder or one of the app's own pages.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Location {
    Home,
    ThisPc,
    RecycleBin,
    Path(PathBuf),
}

impl Location {
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            _ => None,
        }
    }

    /// The location "Up" navigates to. A drive root goes up to This PC, a UNC share root
    /// has no parent, and app pages have none either.
    pub fn parent(&self) -> Option<Self> {
        let path = self.as_path()?;
        if is_drive_root(path) {
            return Some(Self::ThisPc);
        }
        let parent = path.parent()?;
        // `\\server\share\` has no parent in std; a bare prefix is not a navigable folder.
        if parent.as_os_str().is_empty() {
            return None;
        }
        Some(Self::Path(parent.to_path_buf()))
    }
}

/// `C:\` or `C:` — the root of a drive letter.
fn is_drive_root(path: &Path) -> bool {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return false;
    };
    let drive = matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_));
    drive && components.all(|c| matches!(c, Component::RootDir))
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Home => f.write_str("Home"),
            Self::ThisPc => f.write_str("This PC"),
            Self::RecycleBin => f.write_str("Recycle Bin"),
            Self::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_of_folder() {
        let loc = Location::path(r"C:\Users\Raka\Documents");
        assert_eq!(loc.parent(), Some(Location::path(r"C:\Users\Raka")));
    }

    #[test]
    fn drive_root_goes_to_this_pc() {
        assert_eq!(Location::path(r"C:\").parent(), Some(Location::ThisPc));
        assert_eq!(Location::path(r"\\?\D:\").parent(), Some(Location::ThisPc));
    }

    #[test]
    fn pages_have_no_parent() {
        assert_eq!(Location::Home.parent(), None);
        assert_eq!(Location::ThisPc.parent(), None);
    }

    #[test]
    fn unc_share_root_has_no_parent() {
        assert_eq!(Location::path(r"\\server\share\").parent(), None);
        assert_eq!(
            Location::path(r"\\server\share\docs").parent(),
            Some(Location::path(r"\\server\share\"))
        );
    }
}
