# rusk-task

[![Crates.io](https://img.shields.io/crates/v/rusk-task?style=flat-square)](https://crates.io/crates/rusk-task)
[![Crates.io](https://img.shields.io/crates/d/rusk-task?style=flat-square)](https://crates.io/crates/rusk-task)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square)](LICENSE)
![Testing](https://github.com/gw31415/rusk-task/actions/workflows/testing.yml/badge.svg)

An asynchronous task runner in Rust, aiming to be a “simpler Make.”
![demo](https://github.com/user-attachments/assets/ee622cc9-8ebd-4ade-8cee-062e9eb8e2b3)

## Concept

Make is widely used as a task runner, but despite its relatively simple mechanism, it comes with many default settings that can lead to excessive boilerplate or copy-paste usage. rusk-task replaces these default values with more general-purpose ones, offering a “more modern” way to configure tasks in pursuit of a "simpler Make."

## Installation

```bash
cargo install rusk-task
```

## Features

- The configuration file is written in TOML.
- When run with no arguments, rusk-task displays a list of available tasks (there is no default task).
- **Task naming conventions** determine whether a target is a file or a phony:
  - File target: Contains `/` or `.` in its name.
  - Phony target: Starts with a letter, followed by letters, digits, `-`, or `_` (matching `/^[a-zA-Z][a-zA-Z0-9_-]*$/`).
- Searches for `rusk.toml` configuration files in **descendant directories**.
  - Relative paths in a config file are resolved from that config file’s location.
- Independently defined tasks run **in concurrent** whenever possible.
- Supports multiple environments via `deno_task_shell`.

## Comparison with Alternatives

### cargo-make

- cargo-make offers a richer set of features, including a plugin system and the ability to install external dependencies such as crates. In contrast, rusk-task focuses on being a “simpler Make,” purposely minimizing features to keep the design and documentation concise.
- By default, cargo-make is tightly integrated with Rust (though this can be disabled). rusk-task is not tied to any specific language or technology.
- cargo-make tasks can accept arguments, whereas rusk-task currently provides no way to define tasks that accept arguments.

### just

- just is a task runner but not a build system. Like Make, rusk-task can include file dependencies and define tasks that generate files.
- just uses its own file format for configuration.
- just tasks can accept arguments, while rusk-task currently does not support defining tasks with arguments.

### Make

- rusk-task aims to be a “simpler Make.”
- Make uses its own file format for configuration.
- Make treats targets as file-generating tasks by default. rusk-task switches between file targets and phony targets based on naming conventions.
- Make has many other features and specifications, which can add complexity.

## License

[Apache-2.0](./LICENSE)
