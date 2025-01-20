use std::{
    borrow::Cow,
    fmt::Debug,
    hash::Hash,
    ops::{Deref, Sub},
    path::Path,
};

use once_cell::sync::OnceCell;

use path_dedot::ParseDot;

/// A normalized path.
/// - This contains a relative path and an absolute path.
/// - This doesn't contain any dots, other than the current directory.
/// - This is encoded in UTF-8.
pub struct NormarizedPath {
    rel: Option<OnceCell<String>>,
    abs: String,
}

impl PartialEq for NormarizedPath {
    fn eq(&self, other: &Self) -> bool {
        self.abs == other.abs
    }
}

impl Eq for NormarizedPath {}

impl Hash for NormarizedPath {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.abs.hash(state)
    }
}

impl PartialOrd for NormarizedPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.abs.partial_cmp(&other.abs)
    }
}

impl NormarizedPath {
    /// Returns the path as a string slice.
    pub fn as_rel_str(&self) -> &str {
        if let Some(rel) = &self.rel {
            rel.get_or_init(|| {
                pathdiff::diff_paths(self.as_abs_str(), get_current_dir().as_abs_str())
                    .expect(NORM_PATH_ERR)
                    .into_os_string()
                    .into_string()
                    .expect(NORM_PATH_ERR)
            })
        } else {
            "."
        }
    }

    /// Returns the absolute path as a string slice.
    pub fn as_abs_str(&self) -> &str {
        self.abs.as_str()
    }
}

impl Debug for NormarizedPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.rel, f)
    }
}

impl Deref for NormarizedPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.as_abs_str().as_ref()
    }
}

impl AsRef<Path> for NormarizedPath {
    fn as_ref(&self) -> &Path {
        self.as_abs_str().as_ref()
    }
}

/// NOTE: This tool users must ensure that the path is encoded in UTF-8 and they have permission to access the current directory.
const NORM_PATH_ERR: &str = "Failed to process path. Please check:\n\t① Paths must be encoded in UTF-8;\n\t② You must have permission to access the current directory.";

impl<'a, T: Into<Cow<'a, Path>>> From<T> for NormarizedPath {
    fn from(value: T) -> Self {
        normalize_path(value)
    }
}

impl Sub for &NormarizedPath {
    type Output = String;

    fn sub(self, rhs: Self) -> Self::Output {
        pathdiff::diff_paths(self.as_abs_str(), rhs)
            .expect(NORM_PATH_ERR)
            .into_os_string()
            .into_string()
            .expect(NORM_PATH_ERR)
    }
}

fn normalize_path<'a>(path: impl Into<Cow<'a, Path>>) -> NormarizedPath {
    let path: Cow<'_, Path> = path.into();
    let path = path
        .parse_dot_from(get_current_dir().as_abs_str())
        .expect(NORM_PATH_ERR);
    let abs = std::path::absolute(path).expect(NORM_PATH_ERR);
    let abs = abs.into_os_string().into_string().expect(NORM_PATH_ERR);
    let rel = Some(OnceCell::new());
    NormarizedPath { rel, abs }
}

/// Returns the current directory as a normalized path.
pub fn get_current_dir() -> &'static NormarizedPath {
    static CWD: OnceCell<NormarizedPath> = OnceCell::new();
    CWD.get_or_init(|| {
        let path = std::env::current_dir().expect(NORM_PATH_ERR);
        let path = std::path::absolute(path).expect(NORM_PATH_ERR);
        NormarizedPath {
            rel: None,
            abs: path.into_os_string().into_string().expect(NORM_PATH_ERR),
        }
    })
}
