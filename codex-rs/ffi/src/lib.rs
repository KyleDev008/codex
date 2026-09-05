use std::ffi::CStr;
use std::ffi::c_char;
use std::ffi::c_int;

mod runtime;

use runtime::CodexRuntime;
use runtime::free_string;
use runtime::ok;
use runtime::set_error;

/// Create and initialize a new in-process Codex app-server runtime.
///
/// `config_json` is a UTF-8 JSON object. Supported fields:
/// - `codexHome` (string, optional): writable directory used for config and sessions.
/// - `configToml` (string, optional): TOML content written to `codexHome/config.toml`.
/// - `clientName` / `clientVersion` / `clientTitle` (optional): identity sent in initialize.
/// - `experimentalApi` (bool, default true): opt into experimental app-server methods.
/// - `mcpServerOpenaiFormElicitation` (bool, default false): enable OpenAI form elicitation.
/// - `optOutNotificationMethods` (string array): suppress selected notifications.
/// - `channelCapacity` (number, default 1024): runtime queue capacity.
///
/// On success, returns a non-null handle. On failure, `*out_error` is set to a
/// NUL-terminated error string owned by the library; free it with `codex_free_string`.
///
/// # Safety
/// `config_json` and `out_error` must be valid pointers. If `out_error` is non-null,
/// the caller must free the returned error string with `codex_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_runtime_create(
    config_json: *const c_char,
    out_error: *mut *mut c_char,
) -> *mut CodexRuntime {
    if config_json.is_null() {
        set_error(out_error, "config_json is null".to_string());
        return std::ptr::null_mut();
    }

    let config_str = unsafe {
        match CStr::from_ptr(config_json).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_error(out_error, "config_json is not valid utf-8".to_string());
                return std::ptr::null_mut();
            }
        }
    };

    match CodexRuntime::create(config_str) {
        Ok(runtime) => {
            ok(out_error);
            Box::into_raw(runtime)
        }
        Err(msg) => {
            set_error(out_error, msg);
            std::ptr::null_mut()
        }
    }
}

/// Destroy a runtime created by `codex_runtime_create`.
///
/// # Safety
/// `runtime` must be a non-null pointer returned by `codex_runtime_create` and
/// not already destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_runtime_destroy(runtime: *mut CodexRuntime) {
    if !runtime.is_null() {
        unsafe {
            Box::from_raw(runtime).destroy();
        }
    }
}

/// Send a JSON-RPC request to the in-process app-server.
///
/// The request must be a UTF-8 JSON object with `method`, `id`, and `params`.
/// The response (or error) is delivered asynchronously through the event stream
/// returned by `codex_poll_event_json`.
///
/// Returns 0 on success, -1 on failure. On failure, `*out_error` is set.
///
/// # Safety
/// `runtime`, `request_json`, and `out_error` must be valid pointers. If
/// `out_error` is non-null, the caller must free the returned error string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_send_request_json(
    runtime: *mut CodexRuntime,
    request_json: *const c_char,
    out_error: *mut *mut c_char,
) -> c_int {
    if runtime.is_null() || request_json.is_null() {
        set_error(out_error, "runtime or request_json is null".to_string());
        return -1;
    }

    let request_str = unsafe {
        match CStr::from_ptr(request_json).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_error(out_error, "request_json is not valid utf-8".to_string());
                return -1;
            }
        }
    };

    let runtime = unsafe { &*runtime };
    runtime.send_request(request_str, out_error)
}

/// Send a JSON-RPC notification to the in-process app-server.
///
/// Returns 0 on success, -1 on failure. On failure, `*out_error` is set.
///
/// # Safety
/// `runtime`, `notification_json`, and `out_error` must be valid pointers. If
/// `out_error` is non-null, the caller must free the returned error string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_send_notification_json(
    runtime: *mut CodexRuntime,
    notification_json: *const c_char,
    out_error: *mut *mut c_char,
) -> c_int {
    if runtime.is_null() || notification_json.is_null() {
        set_error(
            out_error,
            "runtime or notification_json is null".to_string(),
        );
        return -1;
    }

    let notification_str = unsafe {
        match CStr::from_ptr(notification_json).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_error(
                    out_error,
                    "notification_json is not valid utf-8".to_string(),
                );
                return -1;
            }
        }
    };

    let runtime = unsafe { &*runtime };
    runtime.send_notification(notification_str, out_error)
}

/// Poll the next event from the in-process app-server event stream.
///
/// Blocks up to `timeout_ms` milliseconds. Returns a NUL-terminated JSON string
/// on success, or `null` on timeout or when the worker has exited. The returned
/// string is owned by the library; free it with `codex_free_string`.
///
/// # Safety
/// `runtime` must be a valid pointer returned by `codex_runtime_create`. If
/// `out_error` is non-null, the caller must free the returned error string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_poll_event_json(
    runtime: *mut CodexRuntime,
    timeout_ms: u32,
    out_error: *mut *mut c_char,
) -> *mut c_char {
    if runtime.is_null() {
        set_error(out_error, "runtime is null".to_string());
        return std::ptr::null_mut();
    }

    let runtime = unsafe { &*runtime };
    runtime.poll_event(timeout_ms, out_error)
}

/// Respond to a server request (for example an approval prompt) with a JSON-RPC
/// response object `{"id": ..., "result": ...}` or an error object
/// `{"id": ..., "error": {"code": ..., "message": ...}}`.
///
/// Returns 0 on success, -1 on failure. On failure, `*out_error` is set.
///
/// # Safety
/// `runtime`, `response_json`, and `out_error` must be valid pointers. If
/// `out_error` is non-null, the caller must free the returned error string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_respond_to_server_request_json(
    runtime: *mut CodexRuntime,
    response_json: *const c_char,
    out_error: *mut *mut c_char,
) -> c_int {
    if runtime.is_null() || response_json.is_null() {
        set_error(out_error, "runtime or response_json is null".to_string());
        return -1;
    }

    let response_str = unsafe {
        match CStr::from_ptr(response_json).to_str() {
            Ok(s) => s,
            Err(_) => {
                set_error(out_error, "response_json is not valid utf-8".to_string());
                return -1;
            }
        }
    };

    let runtime = unsafe { &*runtime };
    runtime.respond_to_server_request(response_str, out_error)
}

/// Free a string previously returned by this library.
///
/// # Safety
/// `s` must be either a string returned by this library or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codex_free_string(s: *mut c_char) {
    free_string(s);
}
