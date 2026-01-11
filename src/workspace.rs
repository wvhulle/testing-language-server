//! Workspace detection utilities.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::Workspaces;

/// Determine if a particular file is the root of workspace based on marker
/// files.
fn detect_workspace_from_file(file_path: PathBuf, marker_files: &[&str]) -> Option<String> {
    let parent = file_path.parent()?;
    if marker_files
        .iter()
        .any(|file_name| parent.join(file_name).exists())
    {
        Some(parent.to_string_lossy().to_string())
    } else {
        detect_workspace_from_file(parent.to_path_buf(), marker_files)
    }
}

/// Detect workspaces from a list of file paths using marker files.
///
/// Walks up the directory tree from each file looking for marker files
/// (e.g., Cargo.toml, package.json) to determine workspace roots.
pub fn detect_from_files(file_paths: &[String], marker_files: &[&str]) -> Workspaces {
    let mut result_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut sorted_paths = file_paths.to_vec();
    sorted_paths.sort_by_key(|p| p.len());

    for file_path in sorted_paths {
        let existing_workspace = result_map
            .iter()
            .find(|(workspace_root, _)| file_path.contains(workspace_root.as_str()));

        if let Some((workspace_root, _)) = existing_workspace {
            result_map
                .entry(workspace_root.to_string())
                .or_default()
                .push(file_path.clone());
        }

        let workspace =
            detect_workspace_from_file(PathBuf::from_str(&file_path).unwrap(), marker_files);
        if let Some(workspace) = workspace {
            if result_map
                .get(&workspace)
                .map(|v| !v.contains(&file_path))
                .unwrap_or(true)
            {
                result_map
                    .entry(workspace)
                    .or_default()
                    .push(file_path.clone());
            }
        }
    }

    Workspaces { map: result_map }
}

/// Resolve a relative path against a base directory, handling ../ and ./
/// components.
pub fn resolve_path(base_dir: &Path, relative_path: &str) -> PathBuf {
    let absolute = if Path::new(relative_path).is_absolute() {
        PathBuf::from(relative_path)
    } else {
        base_dir.join(relative_path)
    };

    let mut components = Vec::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::Normal(_) | std::path::Component::RootDir => {
                components.push(component);
            }
            _ => {}
        }
    }

    PathBuf::from_iter(components)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_path() {
        let base_dir = PathBuf::from("/Users/test/projects");

        assert_eq!(
            resolve_path(&base_dir, "github.com/hoge/fuga"),
            PathBuf::from("/Users/test/projects/github.com/hoge/fuga")
        );

        assert_eq!(
            resolve_path(&base_dir, "./github.com/hoge/fuga"),
            PathBuf::from("/Users/test/projects/github.com/hoge/fuga")
        );

        assert_eq!(
            resolve_path(&base_dir, "../other/project"),
            PathBuf::from("/Users/test/other/project")
        );

        assert_eq!(
            resolve_path(&base_dir, "foo/bar/../../../baz"),
            PathBuf::from("/Users/test/baz")
        );

        assert_eq!(
            resolve_path(&base_dir, "/absolute/path"),
            PathBuf::from("/absolute/path")
        );
    }
}
