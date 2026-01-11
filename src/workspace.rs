//! Workspace detection utilities.

use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    str::FromStr,
    string::String,
};

use ignore::WalkBuilder;

use crate::{AdapterConfig, Workspaces};

/// Detected project type with its configuration.
#[derive(Debug, Clone)]
pub struct DetectedProject {
    pub test_kind: String,
    pub root: PathBuf,
}

/// Detect project types in a directory by looking for marker files.
/// Returns a list of detected projects with their configurations.
#[must_use]
pub fn detect_projects(base_dir: &Path) -> Vec<DetectedProject> {
    let mut projects = Vec::new();

    // Check for Cargo.toml (Rust)
    if base_dir.join("Cargo.toml").exists() {
        projects.push(DetectedProject {
            test_kind: "cargo-test".to_string(),
            root: base_dir.to_path_buf(),
        });
    }

    // Check for package.json (JavaScript/TypeScript)
    if base_dir.join("package.json").exists() {
        // Try to determine which test framework
        if let Ok(content) = std::fs::read_to_string(base_dir.join("package.json")) {
            if content.contains("\"vitest\"") {
                projects.push(DetectedProject {
                    test_kind: "vitest".to_string(),
                    root: base_dir.to_path_buf(),
                });
            } else if content.contains("\"jest\"") {
                projects.push(DetectedProject {
                    test_kind: "jest".to_string(),
                    root: base_dir.to_path_buf(),
                });
            }
        }
    }

    // Check for deno.json (Deno)
    if base_dir.join("deno.json").exists() || base_dir.join("deno.jsonc").exists() {
        projects.push(DetectedProject {
            test_kind: "deno".to_string(),
            root: base_dir.to_path_buf(),
        });
    }

    // Check for go.mod (Go)
    if base_dir.join("go.mod").exists() {
        projects.push(DetectedProject {
            test_kind: "go-test".to_string(),
            root: base_dir.to_path_buf(),
        });
    }

    // Check for composer.json with phpunit (PHP)
    if base_dir.join("composer.json").exists() {
        if let Ok(content) = std::fs::read_to_string(base_dir.join("composer.json")) {
            if content.contains("\"phpunit\"") || base_dir.join("phpunit.xml").exists() {
                projects.push(DetectedProject {
                    test_kind: "phpunit".to_string(),
                    root: base_dir.to_path_buf(),
                });
            }
        }
    }

    projects
}

/// Create adapter configuration from a detected project.
#[must_use]
pub fn config_from_detected(project: &DetectedProject) -> AdapterConfig {
    let (include, exclude) = match project.test_kind.as_str() {
        "cargo-test" | "cargo-nextest" => (
            vec!["**/*.rs".to_string()],
            vec!["**/target/**".to_string()],
        ),
        "jest" | "vitest" => (
            vec![
                "**/*.test.{js,ts,jsx,tsx}".to_string(),
                "**/*.spec.{js,ts,jsx,tsx}".to_string(),
            ],
            vec!["**/node_modules/**".to_string()],
        ),
        "deno" => (
            vec!["**/*_test.ts".to_string(), "**/*.test.ts".to_string()],
            vec![],
        ),
        "go-test" => (vec!["**/*_test.go".to_string()], vec![]),
        "phpunit" => (
            vec!["**/*Test.php".to_string()],
            vec!["**/vendor/**".to_string()],
        ),
        "node-test" => (
            vec!["**/*.test.{js,mjs}".to_string()],
            vec!["**/node_modules/**".to_string()],
        ),
        _ => (vec![], vec![]),
    };

    AdapterConfig {
        test_kind: project.test_kind.clone(),
        extra_arg: vec![],
        env: HashMap::new(),
        include,
        exclude,
        workspace_dir: Some(project.root.to_string_lossy().to_string()),
    }
}

/// Walk directory respecting .gitignore and return matching files.
#[must_use]
pub fn walk_files(base_dir: &Path, extensions: &[&str]) -> Vec<String> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(base_dir)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext) {
                    files.push(path.to_string_lossy().to_string());
                }
            }
        }
    }

    files
}

/// Get file extensions for a test kind.
#[must_use]
pub fn extensions_for_test_kind(test_kind: &str) -> Vec<&'static str> {
    match test_kind {
        "cargo-test" | "cargo-nextest" => vec!["rs"],
        "jest" | "vitest" | "node-test" => vec!["js", "ts", "jsx", "tsx", "mjs"],
        "deno" => vec!["ts"],
        "go-test" => vec!["go"],
        "phpunit" => vec!["php"],
        _ => vec![],
    }
}

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
#[must_use]
pub fn detect_from_files(file_paths: &[String], marker_files: &[&str]) -> Workspaces {
    let mut result_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut sorted_paths = file_paths.to_vec();
    sorted_paths.sort_by_key(String::len);

    for file_path in sorted_paths {
        let existing_workspace = result_map
            .iter()
            .find(|(workspace_root, _)| file_path.contains(workspace_root.as_str()));

        if let Some((workspace_root, _)) = existing_workspace {
            result_map
                .entry(workspace_root.clone())
                .or_default()
                .push(file_path.clone());
        }

        let workspace =
            detect_workspace_from_file(PathBuf::from_str(&file_path).unwrap(), marker_files);
        if let Some(workspace) = workspace
            && result_map
                .get(&workspace)
                .is_none_or(|v| !v.contains(&file_path))
        {
            result_map
                .entry(workspace)
                .or_default()
                .push(file_path.clone());
        }
    }

    Workspaces { map: result_map }
}

/// Resolve a relative path against a base directory, handling ../ and ./
/// components.
#[must_use]
pub fn resolve_path(base_dir: &Path, relative_path: &str) -> PathBuf {
    let absolute = if Path::new(relative_path).is_absolute() {
        PathBuf::from(relative_path)
    } else {
        base_dir.join(relative_path)
    };

    let mut components = Vec::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                components.pop();
            }
            Component::Normal(_) | Component::RootDir => {
                components.push(component);
            }
            _ => {}
        }
    }

    PathBuf::from_iter(components)
}

#[cfg(test)]
mod tests {
    use std::env::current_dir;

    use super::*;
    use crate::config::init;

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

    #[test]
    fn test_workspace_detection() {
        init();
        let abs_path_of_demo = current_dir().unwrap().join("demo/rust");
        let demo_lib = abs_path_of_demo.join("src/lib.rs");

        // Detect workspace for demo Rust files
        let workspaces =
            detect_from_files(&[demo_lib.to_str().unwrap().to_string()], &["Cargo.toml"]);

        // Should detect demo/rust as workspace (where Cargo.toml is)
        let detected_workspace = workspaces.map.get(abs_path_of_demo.to_str().unwrap());
        assert!(
            detected_workspace.is_some(),
            "Should detect demo/rust workspace at {abs_path_of_demo:?}"
        );

        let paths = detected_workspace.unwrap();
        assert_eq!(paths.len(), 1);
        assert!(
            paths[0].contains("demo/rust/src"),
            "Path should be in demo/rust/src: {}",
            paths[0]
        );
    }
}
