use std::{
    cell::{Ref, RefCell},
    ffi::OsString,
    fmt::Debug,
    ops::Deref,
};

use deno_task_shell::{ShellPipeReader, ShellPipeWriter, ShellState, parser::SequentialList};
use futures::future::try_join_all;
use hashbrown::HashMap;
use tokio::sync::watch::Receiver;

use crate::{
    digraph::{DigraphItem, TreeNode, TreeNodeCreationError},
    fs::{RuskfileComposer, RuskfileDeserializeError},
    path::{NormarizedPath, get_current_dir},
    taskkey::{TaskKey, TaskKeyParseError, TaskKeyRelative},
};

type TaskTree = TreeNode<TaskKey, TaskExecutable>;

/// Errors that can occur during Rusk operation
#[derive(Debug, thiserror::Error)]
pub enum RuskError {
    /// Argument parsing error
    #[error("Invalid argument: {0}")]
    InvalidArgument(#[from] TaskKeyParseError),
    /// TreeNode creation error
    #[error(transparent)]
    TreeNodeBroken(#[from] TreeNodeCreationError<TaskKey>),
    /// Task parsing error
    #[error(transparent)]
    TaskUnparsable(#[from] TaskParseError),
    /// Task execution error
    #[error(transparent)]
    TaskFailed(#[from] TaskError),
}

/// IO set about deno_task_shell
#[derive(Clone)]
pub struct IOSet {
    pub stdin: ShellPipeReader,
    pub stdout: ShellPipeWriter,
    pub stderr: ShellPipeWriter,
}

impl Default for IOSet {
    fn default() -> Self {
        Self {
            stdin: ShellPipeReader::stdin(),
            stdout: ShellPipeWriter::stdout(),
            stderr: ShellPipeWriter::stderr(),
        }
    }
}

/// Rusk configuration
pub struct Rusk {
    /// Tasks to be executed
    tasks: HashMap<TaskKey, Task>,
}

impl TryFrom<RuskfileComposer> for Rusk {
    type Error = RuskfileDeserializeError;
    fn try_from(value: RuskfileComposer) -> Result<Self, Self::Error> {
        Ok(Rusk {
            tasks: value.try_into()?,
        })
    }
}

impl Rusk {
    /// Execute tasks
    pub async fn exec(
        self,
        args: impl IntoIterator<Item = String>,
        opts: ExecuteOpts,
    ) -> Result<(), RuskError> {
        let Rusk { tasks } = self;
        let tasks = into_executable(tasks, opts)?;
        let tk = args
            .into_iter()
            .map({
                fn f(s: String) -> Result<TaskKey, TaskKeyParseError> {
                    let key = TaskKeyRelative::try_from(s)?;
                    Ok(key.into_task_key(get_current_dir()))
                }
                f
            })
            .collect::<Result<Vec<_>, _>>()?;
        let graph = TreeNode::new_vec(tasks, tk)?;
        exec_all(graph).await?;
        Ok(())
    }
}

/// Task configuration
pub struct Task {
    /// Environment variables that are specific to this task
    pub envs: HashMap<OsString, OsString>,
    /// Script to be executed
    pub script: Option<String>,
    /// Working directory
    pub cwd: NormarizedPath,
    /// Dependencies
    pub depends: Vec<TaskKey>,
}

/// Task execution global options
pub struct ExecuteOpts {
    /// Environment variables
    pub envs: HashMap<OsString, OsString>,
    /// IO
    pub io: IOSet,
}

impl Default for ExecuteOpts {
    fn default() -> Self {
        Self {
            envs: std::env::vars_os().collect(),
            io: Default::default(),
        }
    }
}

/// Alternative for `TryInto<HashMap<_, TaskExecutable>>` for `HashMap<_, Task>`
fn into_executable(
    tasks: HashMap<TaskKey, Task>,
    ExecuteOpts {
        envs: global_env,
        io,
    }: ExecuteOpts,
) -> Result<HashMap<TaskKey, TaskExecutable>, TaskParseError> {
    let mut parsed_tasks: HashMap<TaskKey, TaskExecutable> = HashMap::new();

    for (key, task) in tasks {
        let script = {
            let mut items = Vec::new();
            if let Some(script) = task.script {
                for line in script.lines() {
                    items.extend(match deno_task_shell::parser::parse(line) {
                        Ok(script) => script.items,
                        Err(error) => {
                            return Err(TaskParseError::ScriptParseError { key, error })?;
                        }
                    });
                }
            };
            SequentialList { items }
        };

        let Task {
            envs, cwd, depends, ..
        } = task;

        if !cwd.is_dir() {
            return Err(TaskParseError::DirectoryNotFound(cwd));
        }

        // If dependency is a file, create a virtual TaskExecutable because it may not be actual Task
        // TODO: Avoid instantiate TaskExecutable as much as possible
        for dep in depends.iter() {
            if let TaskKey::File(_) = dep {
                parsed_tasks
                    .entry_ref(dep)
                    .or_insert_with(TaskExecutable::empty);
            }
        }

        parsed_tasks.insert(
            key.clone(),
            TaskExecutableInner {
                io: io.clone(),
                key,
                script,
                depends,
                envs: global_env.clone().into_iter().chain(envs).collect(),
                cwd,
            }
            .into(),
        );
    }

    Ok(parsed_tasks)
}

async fn exec_all(roots: impl IntoIterator<Item = TaskTree>) -> TaskResult {
    async fn exec_node(node: &TaskTree) -> TaskResult {
        let child_futures = node.children.iter().map(|child| exec_node(child));
        try_join_all(child_futures).await?;
        node.item.as_future().await
    }

    let futures = roots
        .into_iter()
        .map(|root| async move { exec_node(&root).await });
    try_join_all(futures).await?;
    Ok(())
}

/// Independent TaskExecutable with state
struct TaskExecutable(RefCell<TaskExecutableState>);

impl TaskExecutable {
    /// Create an empty TaskExecutable which represents a virtual File Task
    fn empty() -> Self {
        TaskExecutable(RefCell::new(TaskExecutableState::Done(Ok(()))))
    }
    pub async fn as_future(&self) -> TaskResult {
        let res = 'res: {
            'early_return: {
                let mut rx = match &self.0.try_borrow().unwrap() as &TaskExecutableState {
                    TaskExecutableState::Done(result) => return result.clone(),
                    TaskExecutableState::Processing(rx) => {
                        if let Some(res) = rx.borrow().as_ref() {
                            break 'res res.clone();
                        }
                        rx.clone() // Bring the channel out of the block and **release self.0 references**.
                    }
                    _ => {
                        break 'early_return; // Tasks need to be performed
                    }
                };

                // If task is running (Processing), wait for results
                rx.changed().await.unwrap();
                break 'res rx.borrow().as_ref().unwrap().clone();
            }

            // If the task is actually executed, create a Watcher and send the results when finished
            let (tx, rx) = tokio::sync::watch::channel(None);
            let TaskExecutableState::Initialized(inner) = std::mem::replace(
                &mut self.0.try_borrow_mut().unwrap() as &mut TaskExecutableState,
                TaskExecutableState::Processing(rx),
            ) else {
                unreachable!()
            };
            let res = inner.into_future().await;
            tx.send(Some(res.clone())).unwrap();
            res
        };

        *self.0.try_borrow_mut().unwrap() = TaskExecutableState::Done(res.clone());
        res
    }
}

impl TaskExecutableInner {
    pub async fn into_future(self) -> TaskResult {
        let TaskExecutableInner {
            io,
            key,
            envs,
            script,
            cwd,
            depends,
        } = self;

        'check_file: {
            match &key {
                TaskKey::File(file) => {
                    // Step 1: Collect dependency file Metadata Objects.
                    // If File not found, the task won't be executed. So check at this point
                    let mut dep_file_metadatas = Vec::new();
                    let dep_count = depends.len();
                    for dep in depends {
                        if let TaskKey::File(dep_file) = dep {
                            let Ok(metadata) = tokio::fs::metadata(&dep_file).await else {
                                return Err(TaskError::DependencyFileNotFound {
                                    dep_file,
                                    task: key,
                                });
                            };
                            dep_file_metadatas.push(metadata);
                        }
                    }
                    if dep_count != dep_file_metadatas.len() {
                        // NOTE: If PhonyTask is included, the script is always executed.
                        break 'check_file;
                    }

                    // Step 2: Get the metadata of the file.
                    // If file not found, it need not to check the modified datetime
                    let Ok(metadata) = tokio::fs::metadata(file).await else {
                        break 'check_file;
                    };
                    let Ok(modified) = metadata.modified() else {
                        return Err(TaskError::FailedToGetFileMetadata);
                    };

                    for dep in dep_file_metadatas {
                        let dep_modified = dep.modified().unwrap(); // Checked above
                        if modified <= dep_modified {
                            // Execution is required if the dependency file has been updated
                            break 'check_file;
                        }
                    }

                    // If none have been updated
                    return Ok(());
                }
                TaskKey::Phony(_) => {
                    // Check only the existence of the dependency file
                    for dep in depends {
                        if let TaskKey::File(file) = dep
                            && !matches!(tokio::fs::try_exists(&file).await, Ok(true))
                        {
                            return Err(TaskError::DependencyFileNotFound {
                                dep_file: file,
                                task: key,
                            });
                        }
                    }
                }
            }
        }
        let exit_code = deno_task_shell::execute_with_pipes(
            script,
            ShellState::new(
                envs,
                cwd.to_path_buf(),
                Default::default(),
                Default::default(),
            ),
            io.stdin,
            io.stdout,
            io.stderr,
        )
        .await;
        if exit_code == 0 {
            Ok(())
        } else {
            Err(TaskError::Execution { key, exit_code })
        }
    }
}

/// TaskExecutable state
enum TaskExecutableState {
    /// Task is not executed yet
    Initialized(TaskExecutableInner),
    /// Task is being executed
    Processing(Receiver<Option<TaskResult>>),
    /// Task is done
    Done(TaskResult),
}

/// TaskExecutable inner data to exec deno_task_shell
struct TaskExecutableInner {
    /// IO set
    io: IOSet,
    /// TaskKey
    key: TaskKey,
    /// Environment variables
    envs: std::collections::HashMap<OsString, OsString>,
    /// Script to be executed
    script: SequentialList,
    /// Working directory
    cwd: NormarizedPath,
    /// TaskKeys that this task depends on
    depends: Vec<TaskKey>, // 依存関係の検索についてはTaskKeyを用いるか検討が必要
}

impl From<TaskExecutableInner> for TaskExecutable {
    fn from(val: TaskExecutableInner) -> Self {
        TaskExecutable(RefCell::new(TaskExecutableState::Initialized(val)))
    }
}

impl DigraphItem<TaskKey> for TaskExecutable {
    fn children(&self) -> impl Deref<Target = [TaskKey]> {
        Ref::map::<[TaskKey], _>(self.0.borrow(), |state| match state {
            TaskExecutableState::Initialized(inner) => inner.depends.as_slice(),
            // In case of Done or Processing, there is no additional dependency
            _ => &[],
        })
    }
}

/// Task parsing error
#[derive(Debug, thiserror::Error)]
pub enum TaskParseError {
    /// Directory not found
    #[error("Directory not found: {0}")]
    DirectoryNotFound(NormarizedPath),
    /// Task script parse error
    #[error("Task {key:?} script parse error: {error:?}")]
    ScriptParseError { key: TaskKey, error: anyhow::Error },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TaskError {
    #[error("Task {key:?} failed with exit code {exit_code}")]
    Execution { key: TaskKey, exit_code: i32 },
    #[error("Not supported platform to get file metadata")]
    FailedToGetFileMetadata,
    #[error("Dependency file {dep_file} not found which is required for {task:?} execution")]
    DependencyFileNotFound {
        dep_file: NormarizedPath,
        task: TaskKey,
    },
}

/// Task result alias
type TaskResult = Result<(), TaskError>;
