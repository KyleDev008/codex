use std::path::PathBuf;

use codex_ffi::codex_free_string;
use codex_ffi::codex_poll_event_json;
use codex_ffi::codex_runtime_create;
use codex_ffi::codex_runtime_destroy;
use codex_ffi::codex_send_request_json;
use tempfile::TempDir;

fn temp_codex_home() -> std::io::Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let path = dir.path().to_path_buf();
    Ok((dir, path))
}

fn cstring(s: &str) -> std::io::Result<std::ffi::CString> {
    std::ffi::CString::new(s).map_err(std::io::Error::other)
}

#[test]
fn runtime_creates_and_responds_to_request() -> std::io::Result<()> {
    let (_dir, codex_home) = temp_codex_home()?;
    let config = serde_json::json!({
        "codexHome": codex_home.to_string_lossy(),
        "clientName": "codex_ffi_test",
        "clientVersion": "0.0.0",
    })
    .to_string();
    let config_c = cstring(&config)?;

    let mut error: *mut std::os::raw::c_char = std::ptr::null_mut();
    let runtime = unsafe { codex_runtime_create(config_c.as_ptr(), &mut error) };
    if !error.is_null() {
        let msg = unsafe {
            std::ffi::CStr::from_ptr(error)
                .to_string_lossy()
                .to_string()
        };
        unsafe {
            codex_free_string(error);
        }
        panic!("runtime creation failed: {msg}");
    }
    assert!(!runtime.is_null(), "runtime should be created");

    // Send a content-free, experimental diagnostic request that does not require
    // external credentials.
    let request = r#"{"method":"server/diagnostics","id":1,"params":{}}"#;
    let request_c = cstring(request)?;
    let result =
        unsafe { codex_send_request_json(runtime, request_c.as_ptr(), std::ptr::null_mut()) };
    assert_eq!(result, 0, "send_request should succeed");

    // Wait up to 10s for the response event.
    let json = unsafe { codex_poll_event_json(runtime, 10000, std::ptr::null_mut()) };
    assert!(!json.is_null(), "expected a response event");
    let event = unsafe { std::ffi::CStr::from_ptr(json).to_string_lossy().to_string() };
    unsafe {
        codex_free_string(json);
    }

    let parsed: serde_json::Value = serde_json::from_str(&event).map_err(std::io::Error::other)?;
    assert_eq!(parsed.get("id"), Some(&serde_json::json!(1)));

    unsafe {
        codex_runtime_destroy(runtime);
    }
    Ok(())
}

#[test]
fn runtime_fails_with_invalid_json() {
    let bad_config = std::ffi::CString::new("not json").expect("valid c string");
    let mut error: *mut std::os::raw::c_char = std::ptr::null_mut();
    let runtime = unsafe { codex_runtime_create(bad_config.as_ptr(), &mut error) };

    assert!(runtime.is_null());
    assert!(!error.is_null());
    let msg = unsafe {
        std::ffi::CStr::from_ptr(error)
            .to_string_lossy()
            .to_string()
    };
    assert!(
        msg.contains("invalid config json"),
        "unexpected error: {msg}"
    );
    unsafe {
        codex_free_string(error);
    }
}
