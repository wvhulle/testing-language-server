use std::{
    collections::HashMap,
    env::current_dir,
    path::{Path, PathBuf},
};

use glob::Pattern;
use lsp_types::*;
use serde::Deserialize;
use serde_json::{Value, json};
use test_lsp::{
    AdapterConfiguration, AdapterId, DiscoveredTests, FileDiagnostics, WorkspaceAnalysis,
    Workspaces, error::LSError, protocol, runner, workspace,
};

const TOML_FILE_NAME: &str = ".testingls.toml";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializedOptions {
    adapter_command: HashMap<AdapterId, AdapterConfiguration>,
    enable_workspace_diagnostics: Option<bool>,
}

pub struct TestingLS {
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,
    pub options: InitializedOptions,
    pub workspaces_cache: Vec<WorkspaceAnalysis>,
}

impl Default for TestingLS {
    fn default() -> Self {
        Self::new()
    }
}

/// The status of workspace diagnostics
/// - Skipped: Skip workspace diagnostics (when `enable_workspace_diagnostics`
///   is false)
/// - Done: Finish workspace diagnostics (when `enable_workspace_diagnostics` is
///   true)
#[derive(Debug, PartialEq, Eq)]
pub enum WorkspaceDiagnosticsStatus {
    Skipped,
    Done,
}

impl TestingLS {
    pub fn new() -> Self {
        Self {
            workspace_folders: None,
            options: Default::default(),
            workspaces_cache: Vec::new(),
        }
    }

    fn project_dir(&self) -> Result<PathBuf, LSError> {
        if let Ok(cwd) = current_dir() {
            Ok(cwd)
        } else {
            let folders = self
                .workspace_folders
                .as_ref()
                .ok_or(LSError::NoWorkspaceFolders)?;
            let uri = &folders[0].uri;
            Ok(uri.to_file_path().unwrap())
        }
    }

    pub fn initialize(
        &mut self,
        id: i64,
        initialize_params: InitializeParams,
    ) -> Result<(), LSError> {
        self.workspace_folders = initialize_params.workspace_folders;
        self.options = (self
            .handle_initialization_options(initialize_params.initialization_options.as_ref()))?;
        let result = InitializeResult {
            capabilities: self.build_capabilities(),
            ..InitializeResult::default()
        };

        protocol::send(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
        }))?;

        Ok(())
    }

    fn adapter_commands(&self) -> HashMap<AdapterId, AdapterConfiguration> {
        self.options.adapter_command.clone()
    }

    fn project_files(base_dir: &Path, include: &[String], exclude: &[String]) -> Vec<String> {
        let mut result: Vec<String> = vec![];

        let exclude_pattern = exclude
            .iter()
            .filter_map(|exclude_pattern| {
                Pattern::new(base_dir.join(exclude_pattern).to_str().unwrap()).ok()
            })
            .collect::<Vec<Pattern>>();
        let base_dir = base_dir.to_str().unwrap();
        let entries = globwalk::GlobWalkerBuilder::from_patterns(base_dir, include)
            .follow_links(true)
            .build()
            .unwrap()
            .filter_map(Result::ok);
        for path in entries {
            let should_exclude = exclude_pattern
                .iter()
                .any(|exclude_pattern| exclude_pattern.matches(path.path().to_str().unwrap()));
            if !should_exclude {
                result.push(path.path().to_str().unwrap().to_owned());
            }
        }
        result
    }

    fn build_capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                identifier: None,
                inter_file_dependencies: false,
                workspace_diagnostics: true,
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })),
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::NONE)),
            ..ServerCapabilities::default()
        }
    }

    pub fn handle_initialization_options(
        &self,
        options: Option<&Value>,
    ) -> Result<InitializedOptions, LSError> {
        let project_dir = self.project_dir()?;
        let toml_path = project_dir.join(TOML_FILE_NAME);

        match std::fs::read_to_string(&toml_path) {
            Ok(content) => Ok(toml::from_str::<InitializedOptions>(&content)?),
            Err(_) => match options {
                Some(opts) => Ok(serde_json::from_value(opts.clone())?),
                None => Err(LSError::ConfigNotFound(toml_path)),
            },
        }
    }

    pub fn refresh_workspaces_cache(&mut self) -> Result<(), LSError> {
        let adapter_commands = self.adapter_commands();
        let project_dir = self.project_dir()?;
        self.workspaces_cache = vec![];

        // Validate adapter configurations and warn about issues
        for (adapter_id, adapter) in &adapter_commands {
            let warnings = adapter.validate(adapter_id);
            for warning in warnings {
                tracing::warn!("{}", warning);
                let params: ShowMessageParams = ShowMessageParams {
                    typ: MessageType::WARNING,
                    message: warning,
                };
                let _ = protocol::send(&json!({
                    "jsonrpc": "2.0",
                    "method": "window/showMessage",
                    "params": params,
                }));
            }
        }

        // Nested and multiple loops, but each count is small
        for (adapter_id, adapter) in adapter_commands.into_iter() {
            tracing::debug!("Processing adapter: {}", adapter_id);
            let AdapterConfiguration {
                test_kind,
                extra_arg: _,
                include,
                exclude,
                workspace_dir,
                ..
            } = &adapter;
            let file_paths = Self::project_files(&project_dir, include, exclude);
            if file_paths.is_empty() {
                continue;
            }

            // Get the runner for this test kind
            let test_runner: Box<dyn runner::Runner> = match runner::get(test_kind) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Failed to get runner for {}: {:?}", test_kind, e);
                    continue;
                }
            };

            // Call detect_workspaces directly
            let workspaces = test_runner.detect_workspaces(&file_paths);

            let workspace_map = if let Some(workspace_dir) = workspace_dir {
                let workspace_dir = workspace::resolve_path(&project_dir, workspace_dir)
                    .to_str()
                    .unwrap()
                    .to_string();
                let target_paths = workspaces
                    .map
                    .into_iter()
                    .flat_map(|kv| kv.1)
                    .collect::<Vec<_>>();
                HashMap::from([(workspace_dir, target_paths)])
            } else {
                workspaces.map
            };
            self.workspaces_cache.push(WorkspaceAnalysis::new(
                adapter,
                Workspaces { map: workspace_map },
            ))
        }
        tracing::info!("workspaces_cache={:#?}", self.workspaces_cache);
        protocol::send(&json!({
            "jsonrpc": "2.0",
            "method": "$/detectedWorkspace",
            "params": self.workspaces_cache,
        }))?;
        Ok(())
    }

    /// Diagnoses the entire workspace for diagnostics.
    /// This function will refresh the workspace cache, check if workspace
    /// diagnostics are enabled, and then iterate through all workspaces to
    /// diagnose them. It will trigger the publication of diagnostics for
    /// all files in the workspace through the Language Server Protocol.
    pub fn diagnose_workspace(&mut self) -> Result<WorkspaceDiagnosticsStatus, LSError> {
        self.refresh_workspaces_cache()?;
        if !self.options.enable_workspace_diagnostics.unwrap_or(true) {
            return Ok(WorkspaceDiagnosticsStatus::Skipped);
        }

        self.workspaces_cache.iter().for_each(
            |WorkspaceAnalysis {
                 adapter_config: adapter,
                 workspaces,
             }| {
                workspaces.map.iter().for_each(|(workspace, paths)| {
                    let _ = self.diagnose(adapter, workspace, paths);
                })
            },
        );
        Ok(WorkspaceDiagnosticsStatus::Done)
    }

    pub fn refreshing_needed(&self, path: &str) -> bool {
        let base_dir = self.project_dir();
        match base_dir {
            Ok(base_dir) => self.workspaces_cache.iter().any(|cache| {
                let include = &cache.adapter_config.include;
                let exclude = &cache.adapter_config.exclude;
                if cache
                    .workspaces
                    .map
                    .iter()
                    .any(|(_, workspace): (&String, &Vec<String>)| {
                        workspace.contains(&path.to_string())
                    })
                {
                    return false;
                }

                Self::project_files(&base_dir, include, exclude).contains(&path.to_owned())
            }),
            Err(e) => {
                tracing::error!("Error: {:?}", e);
                false
            }
        }
    }

    /// Checks a specific file for diagnostics, optionally refreshing the
    /// workspace cache. This function will trigger the publication of
    /// diagnostics for the specified file through the Language Server
    /// Protocol.
    pub fn check_file(&mut self, path: &str, refresh_needed: bool) -> Result<(), LSError> {
        if refresh_needed || self.workspaces_cache.is_empty() {
            self.refresh_workspaces_cache()?;
        }
        self.workspaces_cache.iter().for_each(
            |WorkspaceAnalysis {
                 adapter_config: adapter,
                 workspaces,
             }| {
                for (workspace, paths) in workspaces.map.iter() {
                    if !paths.contains(&path.to_string()) {
                        continue;
                    }
                    let _ = self.diagnose(adapter, workspace, &[path.to_string()]);
                }
            },
        );
        Ok(())
    }

    fn get_diagnostics(
        &self,
        adapter: &AdapterConfiguration,
        workspace: &str,
        paths: &[String],
    ) -> Result<Vec<(String, Vec<Diagnostic>)>, LSError> {
        let mut diagnostics: Vec<(String, Vec<Diagnostic>)> = vec![];

        // Get the runner for this test kind
        let test_runner = runner::get(&adapter.test_kind)?;

        // Call run_tests directly
        match test_runner.run_tests(&paths.to_vec(), workspace, &adapter.extra_arg) {
            Ok(res) => {
                for target_file in paths {
                    let diagnostics_for_file: Vec<Diagnostic> = res
                        .files
                        .clone()
                        .into_iter()
                        .filter(|FileDiagnostics { path, .. }| *path == *target_file)
                        .flat_map(|FileDiagnostics { diagnostics, .. }| diagnostics)
                        .collect();
                    let uri = Url::from_file_path(target_file.replace("file://", "")).unwrap();
                    diagnostics.push((uri.to_string(), diagnostics_for_file));
                }
            }
            Err(err) => {
                let message = format!("Test runner failed: {:?}", err);
                tracing::error!("{}", message);
                let params: ShowMessageParams = ShowMessageParams {
                    typ: MessageType::ERROR,
                    message,
                };
                protocol::send(&json!({
                    "jsonrpc": "2.0",
                    "method": "window/showMessage",
                    "params": params,
                }))
                .unwrap();
            }
        }
        Ok(diagnostics)
    }

    fn diagnose(
        &self,
        adapter: &AdapterConfiguration,
        workspace: &str,
        paths: &[String],
    ) -> Result<(), LSError> {
        let token = NumberOrString::String("testing-ls/start_testing".to_string());
        let progress_token = WorkDoneProgressCreateParams {
            token: token.clone(),
        };
        protocol::send(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "window/workDoneProgress/create",
            "params": progress_token,
        }))
        .unwrap();
        let progress_begin = WorkDoneProgressBegin {
            title: "Testing".to_string(),
            cancellable: Some(false),
            message: Some(format!("testing {} files ...", paths.len())),
            percentage: Some(0),
        };
        let params = ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(progress_begin)),
        };
        protocol::send(&json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": params,
        }))
        .unwrap();
        let diagnostics = self.get_diagnostics(adapter, workspace, paths)?;
        for (path, diagnostics) in diagnostics {
            self.send_diagnostics(
                Url::from_file_path(path.replace("file://", "")).unwrap(),
                diagnostics,
            )?;
        }
        let progress_end = WorkDoneProgressEnd {
            message: Some(format!("tested {} files", paths.len())),
        };
        let params = ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(progress_end)),
        };
        protocol::send(&json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": params,
        }))
        .unwrap();
        Ok(())
    }

    #[allow(clippy::for_kv_map)]
    pub fn discover_file(&self, path: &str) -> Result<DiscoveredTests, LSError> {
        let target_paths = vec![path.to_string()];
        let mut result: DiscoveredTests = DiscoveredTests { files: vec![] };
        for WorkspaceAnalysis {
            adapter_config: adapter,
            workspaces,
        } in &self.workspaces_cache
        {
            for (_, paths) in workspaces.map.iter() {
                if !paths.contains(&path.to_string()) {
                    continue;
                }
                result
                    .files
                    .extend(self.discover(adapter, &target_paths)?.files);
            }
        }
        Ok(result)
    }

    fn discover(
        &self,
        adapter: &AdapterConfiguration,
        paths: &[String],
    ) -> Result<DiscoveredTests, LSError> {
        let test_runner = runner::get(&adapter.test_kind)?;
        test_runner.discover(paths)
    }

    pub fn send_diagnostics(&self, uri: Url, diagnostics: Vec<Diagnostic>) -> Result<(), LSError> {
        let params = PublishDiagnosticsParams::new(uri, diagnostics, None);
        protocol::send(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": params,
        }))?;
        Ok(())
    }

    pub fn shutdown(&self, id: i64) -> Result<(), LSError> {
        protocol::send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": null
        }))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use lsp_types::{Url, WorkspaceFolder};

    use super::*;

    #[test]
    fn test_check_file() {
        let abs_path_of_demo = std::env::current_dir().unwrap().join("demo/rust");
        let mut server = TestingLS {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&abs_path_of_demo).unwrap(),
                name: "demo".to_string(),
            }]),
            options: InitializedOptions {
                adapter_command: HashMap::new(),
                enable_workspace_diagnostics: Some(true),
            },
            workspaces_cache: Vec::new(),
        };
        let librs = abs_path_of_demo.join("lib.rs");
        server.check_file(librs.to_str().unwrap(), true).unwrap();
    }

    #[test]
    fn test_check_workspace() {
        test_lsp::config::init();
        let abs_path_of_demo = std::env::current_dir().unwrap().join("demo/rust");
        let adapter_conf = AdapterConfiguration {
            test_kind: "cargo-test".to_string(),
            include: vec!["src/**/*.rs".to_string()], // Only include files in src/
            exclude: vec!["**/target/**".to_string()],
            ..Default::default()
        };
        let mut server = TestingLS {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&abs_path_of_demo).unwrap(),
                name: "demo".to_string(),
            }]),
            options: InitializedOptions {
                adapter_command: HashMap::from([(String::from(".rs"), adapter_conf)]),
                enable_workspace_diagnostics: Some(true),
            },
            workspaces_cache: Vec::new(),
        };
        server.diagnose_workspace().unwrap();
        assert!(
            !server.workspaces_cache.is_empty(),
            "Should have detected workspaces"
        );
        server
            .workspaces_cache
            .iter()
            .for_each(|workspace_analysis| {
                assert_eq!(workspace_analysis.adapter_config.test_kind, "cargo-test");
                // Check that we detected the demo/rust workspace
                let demo_workspace = workspace_analysis
                    .workspaces
                    .data
                    .get(abs_path_of_demo.to_str().unwrap());
                assert!(
                    demo_workspace.is_some(),
                    "Should detect demo/rust workspace"
                );
                let paths = demo_workspace.unwrap();
                paths.iter().for_each(|path| {
                    assert!(
                        path.contains("rust/src"),
                        "Path should be in rust/src: {}",
                        path
                    );
                });
            });
    }

    #[test]
    fn project_files_are_filtered_by_extension() {
        let absolute_path_of_demo = std::env::current_dir().unwrap().join("demo");
        let files = TestingLS::project_files(
            &absolute_path_of_demo.clone(),
            &["/rust/src/lib.rs".to_string()],
            &["/rust/target/**/*".to_string()],
        );
        let librs = absolute_path_of_demo.join("rust/src/lib.rs");
        assert_eq!(files, vec![librs.to_str().unwrap()]);
        let files = TestingLS::project_files(
            &absolute_path_of_demo.clone(),
            &["jest/*.spec.js".to_string()],
            &["jest/another.spec.js".to_string()],
        );
        let test_file = absolute_path_of_demo.join("jest/index.spec.js");
        assert_eq!(files, vec![test_file.to_str().unwrap()]);
    }

    #[test]
    fn skip_workspace_diagnostics() {
        let mut server = TestingLS {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(current_dir().unwrap()).unwrap(),
                name: "demo".to_string(),
            }]),
            options: InitializedOptions {
                adapter_command: HashMap::new(),
                enable_workspace_diagnostics: Some(false),
            },
            workspaces_cache: Vec::new(),
        };
        let status = server.diagnose_workspace().unwrap();
        assert_eq!(status, WorkspaceDiagnosticsStatus::Skipped);
    }
}
