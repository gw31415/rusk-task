//! Implementations for TaskKey and its related types.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    ops::{Deref, DerefMut},
    path::Path,
};

use colored::Colorize;
use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::path::NormarizedPath;

/// String representing the Phony task.
/// Must match `^[a-zA-Z][a-zA-Z0-9_-]*$`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhonyTaskString {
    inner: String,
}

impl AsRef<str> for PhonyTaskString {
    fn as_ref(&self) -> &str {
        self.inner.as_str()
    }
}

/// Error when parsing PhonyTaskString.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PhonyTaskStringParseError(&'static str);

impl TryFrom<String> for PhonyTaskString {
    type Error = PhonyTaskStringParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(PhonyTaskStringParseError("Empty string is not allowed"));
        }
        let mut chars = value.chars();
        let first = chars.next().unwrap();
        if !first.is_ascii_alphabetic() {
            return Err(PhonyTaskStringParseError(
                "First character must be alphabetic",
            ));
        }
        for c in chars {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
                return Err(PhonyTaskStringParseError(
                    "Only /^[a-zA-Z][a-zA-Z0-9_-]*$/ is allowed",
                ));
            }
        }
        Ok(PhonyTaskString { inner: value })
    }
}

/// String representing the Path task.
/// Must contain '/' or '.'.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathTaskString {
    inner: String,
}

impl AsRef<str> for PathTaskString {
    fn as_ref(&self) -> &str {
        self.inner.as_str()
    }
}

/// Error when parsing PathTaskString.
#[derive(Debug, thiserror::Error)]
#[error("Failed to parse Phony-TaskKey: {0}")]
pub struct PathTaskStringParseError(&'static str);

impl TryFrom<String> for PathTaskString {
    type Error = PathTaskStringParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(PathTaskStringParseError("Empty string is not allowed"));
        }
        if value.contains('/') || value.contains('.') {
            Ok(PathTaskString { inner: value })
        } else {
            Err(PathTaskStringParseError("Path must contain '/' or '.'"))
        }
    }
}

/// Reference to TaskKey.
pub struct TaskKeyRef<'a> {
    inner: &'a TaskKeyRelative,
    owned: Lazy<TaskKey, Box<dyn Fn() -> TaskKey + 'a>>,
}

impl Display for TaskKeyRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.as_task_key(), f)
    }
}

impl Eq for TaskKeyRef<'_> {}

impl PartialEq for TaskKeyRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl PartialOrd for TaskKeyRef<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TaskKeyRef<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inner.cmp(other.inner)
    }
}

impl<'a> TaskKeyRef<'a> {
    fn new(inner: &'a TaskKeyRelative, base: &'a Path) -> Self {
        Self {
            inner,
            owned: Lazy::new(Box::new(move || match inner {
                TaskKeyRelative::Phony(phony_name) => TaskKey::Phony(phony_name.clone()),
                TaskKeyRelative::File(path) => {
                    TaskKey::File(NormarizedPath::from(base.join(&path.inner)))
                }
            })),
        }
    }
    pub fn as_task_key(&self) -> &TaskKey {
        self.owned.deref()
    }
    pub fn into_task_key(mut self) -> TaskKey {
        std::mem::replace(self.owned.deref_mut(), unsafe {
            use std::{
                alloc::{Layout, alloc},
                ptr::read,
            };
            read(alloc(Layout::new::<TaskKey>()) as *const TaskKey)
        })
    }
}

/// TaskKey is either Phony or File.
#[derive(Clone, Eq)]
pub enum TaskKey {
    Phony(PhonyTaskString),
    File(NormarizedPath),
}

/// TaskKey string data without the base path information.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub enum TaskKeyRelative {
    Phony(PhonyTaskString),
    File(PathTaskString),
}

impl PartialOrd for TaskKeyRelative {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TaskKeyRelative {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (TaskKeyRelative::Phony(_), TaskKeyRelative::File(_)) => std::cmp::Ordering::Less,
            (TaskKeyRelative::File(_), TaskKeyRelative::Phony(_)) => std::cmp::Ordering::Greater,
            (TaskKeyRelative::Phony(a), TaskKeyRelative::Phony(b)) => a.as_ref().cmp(b.as_ref()),
            (TaskKeyRelative::File(a), TaskKeyRelative::File(b)) => {
                AsRef::<str>::as_ref(a).cmp(b.as_ref())
            }
        }
    }
}

/// Error when parsing TaskKey.
#[derive(Debug, thiserror::Error)]
pub enum TaskKeyParseError {
    #[error("empty string is not allowed")]
    Empty,
    #[error(transparent)]
    Phony(#[from] PhonyTaskStringParseError),
    #[error(transparent)]
    Path(#[from] PathTaskStringParseError),
}

impl TryFrom<String> for TaskKeyRelative {
    type Error = TaskKeyParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(TaskKeyParseError::Empty);
        }
        if value.contains('/') || value.contains('.') {
            let path = PathTaskString::try_from(value)?;
            return Ok(TaskKeyRelative::File(path));
        }
        let phony_name = PhonyTaskString::try_from(value)?;
        Ok(TaskKeyRelative::Phony(phony_name))
    }
}

impl TaskKeyRelative {
    pub fn as_task_key<'a>(&'a self, base: &'a Path) -> TaskKeyRef<'a> {
        TaskKeyRef::new(self, base)
    }
    pub fn into_task_key(self, cwd: &Path) -> TaskKey {
        self.as_task_key(cwd).into_task_key()
    }
}

impl Hash for TaskKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state);
    }
}

impl AsRef<str> for TaskKey {
    fn as_ref(&self) -> &str {
        match self {
            TaskKey::Phony(phony_name) => phony_name.inner.as_str(),
            TaskKey::File(normarized_path) => normarized_path.as_short_str(),
        }
    }
}

impl PartialEq for TaskKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl Display for TaskKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskKey::Phony(phony_name) => write!(f, "{}", phony_name.inner.bright_purple().bold()),
            TaskKey::File(normarized_path) => {
                write!(f, "{}", normarized_path.as_short_str().bright_blue().bold())
            }
        }
    }
}

// NOTE: Used as a means (hack-like) to display in the digraph module and to distinguish it from Display.
impl Debug for TaskKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskKey::Phony(phony_name) => {
                write!(f, "{}", format!("{:?}", phony_name.inner).bright_purple())
            }
            TaskKey::File(normarized_path) => {
                write!(
                    f,
                    "{}",
                    format!("{:?}", normarized_path.as_short_str()).bright_blue(),
                )
            }
        }
    }
}

impl From<&Self> for TaskKey {
    fn from(val: &Self) -> Self {
        val.clone()
    }
}
