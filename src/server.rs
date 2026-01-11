use std::{
    collections::HashMap,
    env::current_dir,
    path::{Path, PathBuf},
};

use lsp_server::{Connection, ExtractError, Message, Request, RequestId, Response};
use lsp_types::{
    Diagnostic, DiagnosticOptions, DiagnosticServerCapabilities, InitializeParams,
    InitializeResult, MessageType, NumberOrString, ProgressParams, ProgressParamsValue,
    PublishDiagnosticsParams, ServerCapabilities, ShowMessageParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkDoneProgressOptions, WorkspaceFolder,
    request::{Initialize, Shutdown},
};
use serde::de::Error as _;
use serde_json::{Value, json};

use crate::{
    AdapterConfig, AdapterId, Config, DiscoveredTests, FileDiagnostics, WorkspaceAnalysis,
    Workspaces, error::LSError, protocol, runner, workspace,
};

const TOML_FILE_NAME: &str = ".assert-lsp.toml";

pub struct TestingLS {
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,
    pub config: Config,
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

fn extract_textdocument_uri(params: &Value) -> Result<String, serde_json::Error> {
    let uri = params["textDocument"]["uri"]
        .as_str()
        .ok_or(serde_json::Error::custom("`textDocument.uri` is not set"))?;
    Ok(protocol::uri_to_path(uri))
}

fn extract_uri(params: &Value) -> Result<String, serde_json::Error> {
    let uri = params["uri"]
        .as_str()
        .ok_or(serde_json::Error::custom("`uri` is not set"))?;
    Ok(protocol::uri_to_path(uri))
}

fn cast_request<R>(req: Request) -> Result<(RequestId, R::Params), ExtractError<Request>>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    req.extract(R::METHOD)
}

/// Runs the LSP server main loop.
///
/// This function creates a stdio connection and processes incoming LSP messages
/// until the client sends a shutdown request.
///
/// # Errors
///
/// Returns an error if:
/// - The connection fails to initialize
/// - Message handling encounters an unrecoverable error
pub fn run() -> Result<(), LSError> {
    let (connection, io_threads) = Connection::stdio();
    let mut server = TestingLS::new();
    let mut is_workspace_checked = false;

    for msg in &connection.receiver {
        log::info!("received message={:#?}", msg);

        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    break;
                }

                let req_id = req.id.clone();

                if let Ok((_id, params)) = cast_request::<Initialize>(req.clone()) {
                    let id_num = req_id.to_string().parse().unwrap_or(0);
                    server.initialize(id_num, params)?;
                    continue;
                }

                if let Ok((_id, _params)) = cast_request::<Shutdown>(req.clone()) {
                    let id_num = req_id.to_string().parse().unwrap_or(0);
                    server.shutdown(id_num)?;
                    continue;
                }

                match req.method.as_str() {
                    "$/discoverFileTest" => {
                        let uri = extract_uri(&req.params)?;
                        let result = server.discover_file(&uri)?;
                        let response = Response::new_ok(req_id, result);
                        connection
                            .sender
                            .send(Message::Response(response))
                            .map_err(|e| LSError::ChannelSend(e.to_string()))?;
                    }
                    _ => {
                        let response = Response::new_err(
                            req_id,
                            lsp_server::ErrorCode::MethodNotFound as i32,
                            format!("method not found: {}", req.method),
                        );
                        connection
                            .sender
                            .send(Message::Response(response))
                            .map_err(|e| LSError::ChannelSend(e.to_string()))?;
                    }
                }
            }
            Message::Notification(not) => match not.method.as_str() {
                "$/cancelRequest" => {}
                "initialized" => {
                    is_workspace_checked = true;
                    server.diagnose_workspace()?;
                }
                "workspace/diagnostic" => {
                    is_workspace_checked = true;
                    server.diagnose_workspace()?;
                }
                "textDocument/diagnostic" | "textDocument/didSave" => {
                    let uri = extract_textdocument_uri(&not.params)?;
                    server.check_file(&uri, false)?;
                }
                "textDocument/didOpen" => {
                    if !is_workspace_checked {
                        is_workspace_checked = true;
                        server.diagnose_workspace()?;
                    }
                    let uri = extract_textdocument_uri(&not.params)?;
                    if server.refreshing_needed(&uri) {
                        server.refresh_workspaces_cache()?;
                    }
                }
                "$/runFileTest" => {
                    let uri = extract_uri(&not.params)?;
                    server.check_file(&uri, false)?;
                }
                "$/runWorkspaceTest" => {
                    server.diagnose_workspace()?;
                }
                _ => {
                    log::warn!("unhandled notification: {}", not.method);
                }
            },
            Message::Response(resp) => {
                log::warn!("unexpected response: {:?}", resp);
            }
        }
    }

    io_threads.join().expect("Failed to join I/O threads");
    Ok(())
}

impl TestingLS {
    #[must_use]
    pub fn new() -> Self {
        Self {
            workspace_folders: None,
            config: Config::default(),
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
        self.config = self.load_config(initialize_params.initialization_options.as_ref())?;
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

    fn adapter_commands(&self) -> HashMap<AdapterId, AdapterConfig> {
        self.config.adapter_command.clone()
    }

    fn project_files(base_dir: &Path, extensions: &[&str]) -> Vec<String> {
        workspace::walk_files(base_dir, extensions)
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

    pub fn load_config(&self, options: Option<&Value>) -> Result<Config, LSError> {
        let project_dir = self.project_dir()?;
        let toml_path = project_dir.join(TOML_FILE_NAME);

        // Try to read .assert-lsp.toml first
        if let Ok(content) = std::fs::read_to_string(&toml_path) {
            return Ok(toml::from_str::<Config>(&content)?);
        }

        // Try LSP initialization options
        if let Some(opts) = options {
            return Ok(serde_json::from_value(opts.clone())?);
        }

        // Auto-detect project type
        let detected = workspace::detect_projects(&project_dir);
        if detected.is_empty() {
            log::info!("No project detected, using empty configuration");
            return Ok(Config::default());
        }

        log::info!("Auto-detected projects: {:?}", detected);
        let mut adapter_command = HashMap::new();
        for project in detected {
            let config = workspace::config_from_detected(&project);
            adapter_command.insert(project.test_kind.clone(), config);
        }

        Ok(Config {
            adapter_command,
            ..Config::default()
        })
    }

    pub fn refresh_workspaces_cache(&mut self) -> Result<(), LSError> {
        let adapter_commands = self.adapter_commands();
        let project_dir = self.project_dir()?;
        self.workspaces_cache = vec![];

        // Validate adapter configurations and warn about issues
        for (adapter_id, adapter) in &adapter_commands {
            let warnings = adapter.validate(adapter_id);
            for warning in warnings {
                log::warn!("{}", warning);
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
        for (adapter_id, adapter) in adapter_commands {
            log::debug!("Processing adapter: {}", adapter_id);
            let test_kind = &adapter.test_kind;
            let workspace_dir = &adapter.workspace_dir;

            // Get extensions for this test kind and walk files
            let extensions = workspace::extensions_for_test_kind(test_kind);
            let file_paths = Self::project_files(&project_dir, &extensions);
            if file_paths.is_empty() {
                continue;
            }

            // Get the runner for this test kind
            let test_runner: Box<dyn runner::Runner> = match runner::get(test_kind) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to get runner for {}: {:?}", test_kind, e);
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
            ));
        }
        log::info!("workspaces_cache={:#?}", self.workspaces_cache);
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
        if !self.config.enable_workspace_diagnostics {
            return Ok(WorkspaceDiagnosticsStatus::Skipped);
        }

        self.workspaces_cache.iter().for_each(
            |WorkspaceAnalysis {
                 adapter_config: adapter,
                 workspaces,
             }| {
                workspaces.map.iter().for_each(|(workspace, paths)| {
                    let _ = self.diagnose(adapter, workspace, paths);
                });
            },
        );
        Ok(WorkspaceDiagnosticsStatus::Done)
    }

    pub fn refreshing_needed(&self, path: &str) -> bool {
        let base_dir = self.project_dir();
        match base_dir {
            Ok(base_dir) => self.workspaces_cache.iter().any(|cache| {
                let test_kind = &cache.adapter_config.test_kind;
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

                let extensions = workspace::extensions_for_test_kind(test_kind);
                Self::project_files(&base_dir, &extensions).contains(&path.to_owned())
            }),
            Err(e) => {
                log::error!("Error: {:?}", e);
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
                for (workspace, paths) in &workspaces.map {
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
        adapter: &AdapterConfig,
        workspace: &str,
        paths: &[String],
    ) -> Result<Vec<(String, Vec<Diagnostic>)>, LSError> {
        let mut diagnostics: Vec<(String, Vec<Diagnostic>)> = vec![];

        // Get the runner for this test kind
        let test_runner = runner::get(&adapter.test_kind)?;

        // Call run_tests directly
        match test_runner.run_tests(paths, workspace, &adapter.extra_arg) {
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
                let message = format!("Test runner failed: {err:?}");
                log::error!("{}", message);
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
        adapter: &AdapterConfig,
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
            token,
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
            for (_, paths) in &workspaces.map {
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
        adapter: &AdapterConfig,
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
            config: Config {
                adapter_command: HashMap::new(),
                enable_workspace_diagnostics: true,
                ..Config::default()
            },
            workspaces_cache: Vec::new(),
        };
        let librs = abs_path_of_demo.join("lib.rs");
        server.check_file(librs.to_str().unwrap(), true).unwrap();
    }

    #[test]
    fn project_files_finds_rust_files() {
        let absolute_path_of_demo = std::env::current_dir().unwrap().join("demo/rust");
        let files = TestingLS::project_files(&absolute_path_of_demo, &["rs"]);
        assert!(!files.is_empty(), "Should find Rust files");
        assert!(
            files.iter().all(|f| f.ends_with(".rs")),
            "All files should be .rs"
        );
    }

    #[test]
    fn skip_workspace_diagnostics() {
        let mut server = TestingLS {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(current_dir().unwrap()).unwrap(),
                name: "demo".to_string(),
            }]),
            config: Config {
                adapter_command: HashMap::new(),
                enable_workspace_diagnostics: false,
                ..Config::default()
            },
            workspaces_cache: Vec::new(),
        };
        let status = server.diagnose_workspace().unwrap();
        assert_eq!(status, WorkspaceDiagnosticsStatus::Skipped);
    }
}
