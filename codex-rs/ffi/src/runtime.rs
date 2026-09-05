use std::ffi::c_char;
use std::ffi::c_int;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use codex_app_server::in_process::InProcessClientHandle;
use codex_app_server::in_process::InProcessClientSender;
use codex_app_server::in_process::InProcessServerEvent;
use codex_app_server::in_process::InProcessStartArgs;
use codex_app_server_client::EnvironmentManager;
use codex_app_server_client::ExecServerRuntimePaths;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::ClientNotification;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Result as JsonRpcResult;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_config::NoopThreadConfigLoader;
use codex_core::config::ConfigBuilder;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

pub(crate) fn set_error(out_error: *mut *mut c_char, msg: String) {
    if !out_error.is_null() {
        unsafe {
            *out_error = match std::ffi::CString::new(msg) {
                Ok(s) => s.into_raw(),
                Err(_) => std::ptr::null_mut(),
            };
        }
    }
}

pub(crate) fn ok(out_error: *mut *mut c_char) {
    if !out_error.is_null() {
        unsafe {
            *out_error = std::ptr::null_mut();
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiConfig {
    pub codex_home: Option<String>,
    pub config_toml: Option<String>,
    pub client_name: Option<String>,
    pub client_title: Option<String>,
    pub client_version: Option<String>,
    pub codex_self_exe: Option<String>,
    pub codex_linux_sandbox_exe: Option<String>,
    pub experimental_api: Option<bool>,
    pub mcp_server_openai_form_elicitation: Option<bool>,
    pub opt_out_notification_methods: Option<Vec<String>>,
    pub channel_capacity: Option<usize>,
}

impl FfiConfig {
    fn codex_home(&self) -> std::io::Result<PathBuf> {
        if let Some(home) = &self.codex_home {
            Ok(Path::new(home).to_path_buf())
        } else {
            Ok(codex_utils_home_dir::find_codex_home()?.to_path_buf())
        }
    }

    fn client_name(&self) -> String {
        self.client_name
            .clone()
            .unwrap_or_else(|| "codex_ffi".to_string())
    }

    fn client_version(&self) -> String {
        self.client_version
            .clone()
            .unwrap_or_else(|| "0.0.0".to_string())
    }
}

/// Opaque handle returned to C callers.
pub struct CodexRuntime {
    runtime: Runtime,
    sender: InProcessClientSender,
    event_tx: mpsc::UnboundedSender<String>,
    event_rx: std::sync::Mutex<mpsc::UnboundedReceiver<String>>,
    _worker: tokio::task::JoinHandle<()>,
}

impl CodexRuntime {
    pub fn create(config_json: &str) -> Result<Box<CodexRuntime>, String> {
        let config: FfiConfig =
            serde_json::from_str(config_json).map_err(|e| format!("invalid config json: {e}"))?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_stack_size(16 * 1024 * 1024)
            .enable_all()
            .build()
            .map_err(|e| format!("failed to create tokio runtime: {e}"))?;

        let (sender, event_tx, event_rx, worker) = runtime
            .block_on(Self::start_client(config))
            .map_err(|e| e.to_string())?;

        Ok(Box::new(CodexRuntime {
            runtime,
            sender,
            event_tx,
            event_rx: std::sync::Mutex::new(event_rx),
            _worker: worker,
        }))
    }

    /// Shut down the runtime, giving spawned tasks a short grace period to stop.
    pub fn destroy(self) {
        self.runtime.shutdown_timeout(Duration::from_secs(5));
    }

    async fn start_client(
        config: FfiConfig,
    ) -> std::io::Result<(
        InProcessClientSender,
        mpsc::UnboundedSender<String>,
        mpsc::UnboundedReceiver<String>,
        tokio::task::JoinHandle<()>,
    )> {
        let codex_home = config.codex_home()?;
        std::fs::create_dir_all(&codex_home)?;

        if let Some(toml) = &config.config_toml {
            let config_path = codex_home.join("config.toml");
            std::fs::write(&config_path, toml)?;
        }

        let cfg = ConfigBuilder::default()
            .codex_home(codex_home.clone())
            .build()
            .await?;

        let codex_self_exe = config
            .codex_self_exe
            .as_deref()
            .map(Path::new)
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_exe().ok())
            .and_then(|p| AbsolutePathBuf::from_absolute_path(p).ok());

        let runtime_paths = if let Some(ref self_exe) = codex_self_exe {
            Some(ExecServerRuntimePaths::new(
                self_exe.as_path().to_path_buf(),
                config
                    .codex_linux_sandbox_exe
                    .as_deref()
                    .map(Path::new)
                    .map(Path::to_path_buf),
            )?)
        } else {
            None
        };

        let arg0_paths = Arg0DispatchPaths {
            codex_self_exe: codex_self_exe.as_ref().map(AbsolutePathBuf::to_path_buf),
            ..Arg0DispatchPaths::default()
        };

        let environment_manager = EnvironmentManager::from_codex_home(
            codex_home.clone(),
            runtime_paths,
            cfg.http_client_factory(),
        )
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;

        let client_name = config.client_name();
        let client_version = config.client_version();

        let args = InProcessStartArgs {
            arg0_paths,
            config: Arc::new(cfg),
            cli_overrides: Vec::new(),
            loader_overrides: LoaderOverrides::default(),
            strict_config: false,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
            thread_config_loader: Arc::new(NoopThreadConfigLoader),
            feedback: CodexFeedback::new(),
            log_db: None,
            state_db: None,
            environment_manager: Arc::new(environment_manager),
            config_warnings: Vec::new(),
            session_source: SessionSource::VSCode,
            enable_codex_api_key_env: true,
            initialize: InitializeParams {
                client_info: ClientInfo {
                    name: client_name,
                    title: config.client_title.clone(),
                    version: client_version,
                },
                capabilities: Some(InitializeCapabilities {
                    experimental_api: config.experimental_api.unwrap_or(true),
                    request_attestation: false,
                    extensions: None,
                    opt_out_notification_methods: config.opt_out_notification_methods,
                    mcp_server_openai_form_elicitation: config
                        .mcp_server_openai_form_elicitation
                        .unwrap_or(false),
                }),
            },
            channel_capacity: config.channel_capacity.unwrap_or(1024),
        };

        let client = codex_app_server::in_process::start(args).await?;
        let sender = client.sender();

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let worker = Self::spawn_event_worker(client, event_tx.clone());

        Ok((sender, event_tx, event_rx, worker))
    }

    fn spawn_event_worker(
        mut client: InProcessClientHandle,
        event_tx: mpsc::UnboundedSender<String>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(event) = client.next_event().await {
                if let Some(json) = event_to_json(event)
                    && event_tx.send(json).is_err()
                {
                    break;
                }
            }
        })
    }

    pub fn send_request(&self, request_json: &str, out_error: *mut *mut c_char) -> c_int {
        let request: ClientRequest = match serde_json::from_str(request_json) {
            Ok(r) => r,
            Err(e) => {
                set_error(out_error, format!("invalid request json: {e}"));
                return -1;
            }
        };

        let request_id = request.id().clone();
        let sender = self.sender.clone();
        let event_tx = self.event_tx.clone();

        self.runtime.spawn(async move {
            let result = sender.request(request).await;
            let message = request_result_to_message(request_id, result);
            if let Ok(json) = serde_json::to_string(&message) {
                let _ = event_tx.send(json);
            }
        });

        ok(out_error);
        0
    }

    pub fn send_notification(&self, notification_json: &str, out_error: *mut *mut c_char) -> c_int {
        let notification: ClientNotification = match serde_json::from_str(notification_json) {
            Ok(n) => n,
            Err(e) => {
                set_error(out_error, format!("invalid notification json: {e}"));
                return -1;
            }
        };

        if let Err(e) = self.sender.notify(notification) {
            set_error(out_error, e.to_string());
            return -1;
        }

        ok(out_error);
        0
    }

    fn lock_event_rx(
        &self,
        out_error: *mut *mut c_char,
    ) -> Option<std::sync::MutexGuard<'_, mpsc::UnboundedReceiver<String>>> {
        match self.event_rx.lock() {
            Ok(guard) => Some(guard),
            Err(_poisoned) => {
                set_error(out_error, "event receiver lock poisoned".to_string());
                None
            }
        }
    }

    pub fn poll_event(&self, timeout_ms: u32, out_error: *mut *mut c_char) -> *mut c_char {
        let result = if timeout_ms == 0 {
            let mut rx = match self.lock_event_rx(out_error) {
                Some(g) => g,
                None => return std::ptr::null_mut(),
            };
            Ok::<_, tokio::time::error::Elapsed>(rx.try_recv().ok())
        } else {
            self.runtime.block_on(async {
                let deadline =
                    tokio::time::Instant::now() + Duration::from_millis(timeout_ms.into());
                loop {
                    {
                        let mut rx = match self.event_rx.lock() {
                            Ok(g) => g,
                            Err(poisoned) => return Ok(poisoned.into_inner().try_recv().ok()),
                        };
                        if let Ok(event) = rx.try_recv() {
                            return Ok(Some(event));
                        }
                    }

                    tokio::time::sleep(Duration::from_millis(1)).await;

                    if tokio::time::Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
            })
        };

        match result {
            Ok(None) => std::ptr::null_mut(),
            Ok(Some(json)) => match std::ffi::CString::new(json) {
                Ok(s) => s.into_raw(),
                Err(_) => {
                    set_error(out_error, "failed to encode event as c string".to_string());
                    std::ptr::null_mut()
                }
            },
            Err(_) => {
                ok(out_error);
                std::ptr::null_mut()
            }
        }
    }

    pub fn respond_to_server_request(
        &self,
        response_json: &str,
        out_error: *mut *mut c_char,
    ) -> c_int {
        let message: JSONRPCMessage = match serde_json::from_str(response_json) {
            Ok(m) => m,
            Err(e) => {
                set_error(out_error, format!("invalid response json: {e}"));
                return -1;
            }
        };

        match message {
            JSONRPCMessage::Response(JSONRPCResponse { id, result }) => {
                if let Err(e) = self.sender.respond_to_server_request(id, result) {
                    set_error(out_error, e.to_string());
                    return -1;
                }
            }
            JSONRPCMessage::Error(JSONRPCError { id, error }) => {
                if let Err(e) = self.sender.fail_server_request(id, error) {
                    set_error(out_error, e.to_string());
                    return -1;
                }
            }
            _ => {
                set_error(
                    out_error,
                    "response must be a json-rpc response or error".to_string(),
                );
                return -1;
            }
        }

        ok(out_error);
        0
    }
}

fn event_to_json(event: InProcessServerEvent) -> Option<String> {
    let message = match event {
        InProcessServerEvent::ServerNotification(n) => server_notification_message(*n),
        InProcessServerEvent::ServerRequest(r) => server_request_message(*r),
        InProcessServerEvent::Lagged { skipped } => {
            JSONRPCMessage::Notification(JSONRPCNotification {
                method: "_ffi/lagged".to_string(),
                params: Some(serde_json::json!({ "skipped": skipped })),
            })
        }
    };
    serde_json::to_string(&message).ok()
}

fn server_notification_message(notification: ServerNotification) -> JSONRPCMessage {
    let value = match serde_json::to_value(notification) {
        Ok(v) => v,
        Err(_) => {
            return JSONRPCMessage::Notification(JSONRPCNotification {
                method: "_ffi/serializationError".to_string(),
                params: None,
            });
        }
    };

    let method = value
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("_ffi/unknown")
        .to_string();
    let params = value.get("params").cloned();

    JSONRPCMessage::Notification(JSONRPCNotification { method, params })
}

fn server_request_message(request: ServerRequest) -> JSONRPCMessage {
    let id = request.id().clone();
    let value = match serde_json::to_value(request) {
        Ok(v) => v,
        Err(_) => {
            return JSONRPCMessage::Request(JSONRPCRequest {
                id,
                method: "_ffi/serializationError".to_string(),
                params: None,
                trace: None,
            });
        }
    };

    let method = value
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("_ffi/unknown")
        .to_string();
    let params = value.get("params").cloned();

    JSONRPCMessage::Request(JSONRPCRequest {
        id,
        method,
        params,
        trace: None,
    })
}

fn request_result_to_message(
    id: RequestId,
    result: std::io::Result<std::result::Result<JsonRpcResult, JSONRPCErrorError>>,
) -> JSONRPCMessage {
    match result {
        Ok(Ok(result)) => JSONRPCMessage::Response(JSONRPCResponse { id, result }),
        Ok(Err(error)) => JSONRPCMessage::Error(JSONRPCError { id, error }),
        Err(source) => JSONRPCMessage::Error(JSONRPCError {
            id,
            error: JSONRPCErrorError {
                code: -32000,
                message: source.to_string(),
                data: None,
            },
        }),
    }
}

/// Free a string previously returned by the FFI.
pub fn free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = std::ffi::CString::from_raw(s);
        }
    }
}
