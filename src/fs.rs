use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    fmt::Display,
    path::Path,
};

use anyhow::Error;
use colored::Colorize;
use futures::future::join_all;
use hashbrown::{HashMap, hash_map::EntryRef};
use ignore::{WalkBuilder, WalkState};
use itertools::Itertools;
use toml::Table;
use unicode_width::UnicodeWidthStr;

use crate::{
    path::NormarizedPath,
    rusk::Task,
    taskkey::{TaskKey, TaskKeyRef, TaskKeyRelative},
};

/// Configuration files
#[derive(Default)]
pub struct RuskfileComposer {
    /// Map of rusk.toml files
    map: HashMap<NormarizedPath, Result<RuskfileDeserializer, String>>,
}

/// Check if the filename is ruskfile
fn is_ruskfile(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    name == "rusk.toml" || name.ends_with(".rusk.toml")
}

/// Item of tasks_list
#[derive(PartialEq, Eq, PartialOrd)]
pub struct TasksListItem<'a> {
    /// Task content
    content: Result<TaskListItemContent<'a>, &'a str>,
    /// Path to rusk.toml
    path: &'a NormarizedPath,
}

/// Display TasksListItem for tty
pub struct TasksListItemPretty<'a> {
    inner: TasksListItem<'a>,
    task_word_width: usize,
}

impl Display for TasksListItemPretty<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let TasksListItem { content, path } = &self.inner;
        ////////////////////////////////////////////////
        //
        // Format:
        //     (task_name)  (description)"  in "(path)
        //
        ////////////////////////////////////////////////

        let width = self.task_word_width + 2;
        match content {
            Ok(TaskListItemContent { key, description }) => {
                // (task_name)
                let task_key = key.as_task_key();
                write!(f, "{}", task_key)?;
                for _ in 0..width - task_key.as_ref().width() {
                    ' '.fmt(f)?;
                }
                if let Some(description) = description {
                    // (description)
                    write!(f, "{}  ", description.green().italic())?;
                }
            }
            Err(_) => {
                // (task_name): Undefined Task
                write!(f, "{:width$}  ", "(null)".dimmed().italic(), width = width)?;
            }
        }

        // "in "
        "in".dimmed().italic().fmt(f)?;
        ' '.fmt(f)?;

        // (path)
        path.as_short_str().yellow().dimmed().italic().fmt(f)?;
        Ok(())
    }
}

impl<'a> TasksListItem<'a> {
    /// Write verbose error
    pub fn into_verbose(self) -> impl Display + 'a {
        if self.content.is_ok() {
            panic!("Programming Error: TasksListItem::verbose() is not for Ok variant");
        }
        TaskErrorVerboseDisplayer(self)
    }
}

/// Struct which implements Display to show error verbose
struct TaskErrorVerboseDisplayer<'a>(TasksListItem<'a>);

impl Display for TaskErrorVerboseDisplayer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(inner) = self;
        match inner.content {
            Err(err) => {
                // Error Title: Decorated path
                inner
                    .path
                    .as_short_str()
                    .yellow()
                    .bold()
                    .italic()
                    .underline()
                    .fmt(f)?;

                ':'.fmt(f)?;

                // Indented error message
                for line in err.lines() {
                    "\n    ".fmt(f)?;
                    line.fmt(f)?;
                }
            }
            _ => unimplemented!(),
        };
        Ok(())
    }
}

impl Ord for TasksListItem<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let cmp = self.content.cmp(&other.content);
        if let std::cmp::Ordering::Equal = cmp {
            self.path.cmp(other.path)
        } else {
            cmp
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd)]
struct TaskListItemContent<'a> {
    /// TaskKey
    key: TaskKeyRef<'a>,
    /// Task description
    description: Option<&'a str>,
}

impl Ord for TaskListItemContent<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

impl Display for TasksListItem<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        ////////////////////////////////////////////////
        //
        // Format:
        //     (task_name)\t(description)\t"in "(path)
        //
        ////////////////////////////////////////////////

        /// write content with tab
        macro_rules! writet {
            ($x: expr) => {
                $x.fmt(f)?;
                '\t'.fmt(f)?;
            };
        }

        match &self.content {
            Ok(TaskListItemContent { key, description }) => {
                // (task_name)
                writet!(key);
                if let Some(description) = description {
                    // (description)
                    writet!(description);
                }
            }
            Err(_) => {
                // (task_name): Undefined Task
                writet!("(null)");
            }
        }

        // "in "
        "in ".fmt(f)?;

        // (path)
        self.path.as_short_str().fmt(f)
    }
}

impl RuskfileComposer {
    /// Create a new Ruskfiles
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
    /// List all tasks
    pub fn tasks_list(&self) -> impl Iterator<Item = TasksListItem<'_>> {
        self.map
            .iter()
            .filter_map(|(path, res)| match res {
                Ok(config) => Some(config.tasks.iter().map(move |(key, task)| TasksListItem {
                    content: Ok(TaskListItemContent {
                        key: key.as_task_key(Path::parent(path).unwrap()),
                        description: task.description.as_deref(),
                    }),
                    path,
                })),
                _ => None,
            })
            .flatten()
    }
    /// List all tasks with pretty format & sorted
    pub fn tasks_list_pretty(&self) -> impl Iterator<Item = TasksListItemPretty<'_>> {
        let tasks: Vec<_> = self.tasks_list().sorted().collect();
        let task_word_width = tasks
            .iter()
            .map(|a| {
                if let Ok(content) = &a.content {
                    content.key.as_task_key().as_ref().width()
                } else {
                    0
                }
            })
            .max()
            .unwrap_or_default();
        tasks.into_iter().map(move |a| TasksListItemPretty {
            inner: a,
            task_word_width,
        })
    }
    /// List all errors
    pub fn errors_list(&self) -> impl Iterator<Item = TasksListItem<'_>> {
        self.map.iter().filter_map(|(path, res)| match res {
            Err(err) => Some(TasksListItem {
                content: Err(err),
                path,
            }),
            _ => None,
        })
    }

    /// Walk through the directory and find all rusk.toml files
    pub async fn walkdir(&mut self, path: impl AsRef<Path>) {
        let threads = {
            let (tx, mut rx) = tokio::sync::mpsc::channel(0x1000);
            tokio::task::spawn_blocking({
                let mut walkbuilder = WalkBuilder::new(path);
                move || {
                    walkbuilder
                        .require_git(true)
                        .follow_links(true)
                        .build_parallel()
                        .run(|| {
                            Box::new(|res| {
                                if let Ok(entry) = res
                                    && let Some(ft) = entry.file_type()
                                {
                                    if ft.is_file() && is_ruskfile(entry.file_name()) {
                                        let path = NormarizedPath::from(entry.path());
                                        tx.blocking_send(async move {
                                            // make Future of Config
                                            let res = tokio::fs::read_to_string(&path)
                                                .await
                                                .map_err(Error::from)
                                                .and_then(|content| {
                                                    toml::from_str::<RuskfileDeserializer>(&content)
                                                        .map_err(Error::from)
                                                })
                                                .map_err(|err| err.to_string());
                                            (path, res)
                                        })
                                        .unwrap();
                                    }
                                    WalkState::Continue
                                } else {
                                    WalkState::Skip
                                }
                            })
                        });
                }
            });
            let mut threads = Vec::new();
            while let Some(f) = rx.recv().await {
                threads.push(f);
            }
            threads
        };
        self.map.extend(join_all(threads).await);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuskfileDeserializeError {
    #[error("Task {0} is duplicated")]
    DuplicatedTaskName(TaskKey),
    #[error("Failed to convert Task: {0}")]
    DeserializeError(#[from] toml::de::Error),
}

impl TryFrom<RuskfileComposer> for HashMap<TaskKey, Task> {
    type Error = RuskfileDeserializeError;
    fn try_from(composer: RuskfileComposer) -> Result<Self, Self::Error> {
        let RuskfileComposer { map } = composer;
        let mut tasks = HashMap::new();
        for (path, res) in map {
            let Ok(config) = res else {
                continue;
            };
            let configfile_dir = path.into_parent().unwrap(); // NOTE: path is guaranteed to be a NormalizedPath of an existing file, so it should have a parent directory
            for (key, TaskDeserializer { inner, .. }) in config.tasks {
                let key = key.into_task_key(&configfile_dir);
                let TaskDeserializerInner {
                    envs,
                    script,
                    depends,
                    cwd,
                } = inner.try_into()?; // NOTE: It is guaranteed to be a table, and fields that are not present will have default values.
                match tasks.entry_ref(&key) {
                    EntryRef::Occupied(_) => {
                        return Err(RuskfileDeserializeError::DuplicatedTaskName(key));
                    }
                    EntryRef::Vacant(e) => {
                        e.insert(Task {
                            envs,
                            script,
                            cwd: configfile_dir.join(cwd.as_ref()).into(),
                            depends: depends
                                .into_iter()
                                .map(|key| key.into_task_key(&configfile_dir))
                                .collect(),
                        });
                    }
                }
            }
        }
        Ok(tasks)
    }
}

/// serde::Deserialize of Ruskfile File content
#[derive(serde::Deserialize)]
struct RuskfileDeserializer {
    /// TaskDeserializers map
    #[serde(default)]
    tasks: HashMap<TaskKeyRelative, TaskDeserializer>,
}

/// serde::Deserialize of Each rusk Task
#[derive(serde::Deserialize)]
struct TaskDeserializer {
    /// Task Raw content
    #[serde(flatten)]
    inner: Table,
    /// Description for help
    #[serde(default)]
    description: Option<String>,
}

#[derive(serde::Deserialize)]
struct TaskDeserializerInner {
    /// Environment variables that are specific to this task
    #[serde(default)]
    envs: HashMap<OsString, OsString>,
    /// Script to be executed
    #[serde(default)]
    script: Option<String>,
    /// Dependencies
    #[serde(default)]
    depends: Vec<TaskKeyRelative>,
    /// Working directory
    #[serde(default)]
    cwd: Cow<'static, str>,
}

impl Default for TaskDeserializerInner {
    fn default() -> Self {
        Self {
            envs: Default::default(),
            script: Default::default(),
            depends: Default::default(),
            cwd: Cow::Borrowed("."),
        }
    }
}
