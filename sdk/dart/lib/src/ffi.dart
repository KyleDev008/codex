import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

/// Exception thrown when the native `codex_ffi` library cannot be located or
/// when a call into it returns an error.
class CodexFfiException implements Exception {
  final String message;
  CodexFfiException(this.message);

  @override
  String toString() => 'CodexFfiException: $message';
}

/// Opaque handle to an in-process Codex runtime.
final class CodexRuntimeHandle {
  final Pointer<Void> _ptr;

  CodexRuntimeHandle(this._ptr);

  Pointer<Void> get ptr => _ptr;

  bool get isValid => _ptr != nullptr;
}

/// Thin binding around the `codex_ffi` C ABI.
///
/// The methods mirror the Rust functions in `codex-rs/ffi/src/lib.rs` and use
/// JSON strings as the cross-language wire format.
final class CodexFfi {
  final DynamicLibrary _lib;

  late final Pointer<Void> Function(Pointer<Utf8>, Pointer<Pointer<Utf8>>)
      _runtimeCreate;
  late final void Function(Pointer<Void>) _runtimeDestroy;
  late final int Function(Pointer<Void>, Pointer<Utf8>, Pointer<Pointer<Utf8>>)
      _sendRequest;
  late final int Function(Pointer<Void>, Pointer<Utf8>, Pointer<Pointer<Utf8>>)
      _sendNotification;
  late final Pointer<Utf8> Function(Pointer<Void>, int, Pointer<Pointer<Utf8>>)
      _pollEvent;
  late final int Function(Pointer<Void>, Pointer<Utf8>, Pointer<Pointer<Utf8>>)
      _respondToServerRequest;
  late final void Function(Pointer<Utf8>) _freeString;

  CodexFfi._(this._lib) {
    _runtimeCreate = _lib.lookupFunction<
        Pointer<Void> Function(Pointer<Utf8>, Pointer<Pointer<Utf8>>),
        Pointer<Void> Function(Pointer<Utf8>, Pointer<Pointer<Utf8>>)>(
      'codex_runtime_create',
    );
    _runtimeDestroy = _lib.lookupFunction<Void Function(Pointer<Void>),
        void Function(Pointer<Void>)>('codex_runtime_destroy');
    _sendRequest = _lib.lookupFunction<
        Int32 Function(Pointer<Void>, Pointer<Utf8>, Pointer<Pointer<Utf8>>),
        int Function(Pointer<Void>, Pointer<Utf8>,
            Pointer<Pointer<Utf8>>)>('codex_send_request_json');
    _sendNotification = _lib.lookupFunction<
        Int32 Function(Pointer<Void>, Pointer<Utf8>, Pointer<Pointer<Utf8>>),
        int Function(Pointer<Void>, Pointer<Utf8>,
            Pointer<Pointer<Utf8>>)>('codex_send_notification_json');
    _pollEvent = _lib.lookupFunction<
        Pointer<Utf8> Function(Pointer<Void>, Uint32, Pointer<Pointer<Utf8>>),
        Pointer<Utf8> Function(Pointer<Void>, int,
            Pointer<Pointer<Utf8>>)>('codex_poll_event_json');
    _respondToServerRequest = _lib.lookupFunction<
        Int32 Function(Pointer<Void>, Pointer<Utf8>, Pointer<Pointer<Utf8>>),
        int Function(Pointer<Void>, Pointer<Utf8>,
            Pointer<Pointer<Utf8>>)>('codex_respond_to_server_request_json');
    _freeString = _lib.lookupFunction<Void Function(Pointer<Utf8>),
        void Function(Pointer<Utf8>)>('codex_free_string');
  }

  /// Loads `codex_ffi` from [libraryPath].
  factory CodexFfi.load(String libraryPath) {
    if (!File(libraryPath).existsSync()) {
      throw CodexFfiException('library not found: $libraryPath');
    }
    final lib = DynamicLibrary.open(libraryPath);
    return CodexFfi._(lib);
  }

  String? _readAndFreeError(
      Pointer<Pointer<Utf8>> errorPtr, void Function(Pointer<Utf8>) free) {
    final ptr = errorPtr.value;
    if (ptr == nullptr) {
      return null;
    }
    final message = ptr.toDartString();
    free(ptr);
    return message;
  }

  /// Creates a runtime from a JSON configuration string.
  CodexRuntimeHandle createRuntime(String configJson) {
    return using((arena) {
      final configNative = configJson.toNativeUtf8(allocator: arena);
      final errorPtr = arena<Pointer<Utf8>>(1);
      final handle = _runtimeCreate(configNative, errorPtr);
      final error = _readAndFreeError(errorPtr, _freeString);
      if (error != null) {
        throw CodexFfiException(error);
      }
      return CodexRuntimeHandle(handle);
    });
  }

  void destroyRuntime(CodexRuntimeHandle handle) {
    _runtimeDestroy(handle.ptr);
  }

  void _callWithJson(
    CodexRuntimeHandle handle,
    String json,
    int Function(Pointer<Void>, Pointer<Utf8>, Pointer<Pointer<Utf8>>) fn,
    String operation,
  ) {
    using((arena) {
      final native = json.toNativeUtf8(allocator: arena);
      final errorPtr = arena<Pointer<Utf8>>(1);
      final result = fn(handle.ptr, native, errorPtr);
      final error = _readAndFreeError(errorPtr, _freeString);
      if (result != 0 || error != null) {
        throw CodexFfiException(
            '$operation failed: ${error ?? 'code $result'}');
      }
    });
  }

  void sendRequest(CodexRuntimeHandle handle, String requestJson) =>
      _callWithJson(handle, requestJson, _sendRequest, 'send_request');

  void sendNotification(CodexRuntimeHandle handle, String notificationJson) =>
      _callWithJson(
          handle, notificationJson, _sendNotification, 'send_notification');

  /// Responds to a server request (approval, etc.).
  /// [responseJson] must be a JSON-RPC `response` or `error` object with an
  /// `id` matching the server request.
  void respondToServerRequest(CodexRuntimeHandle handle, String responseJson) =>
      _callWithJson(handle, responseJson, _respondToServerRequest,
          'respond_to_server_request');

  /// Polls the next event from the runtime.
  ///
  /// Returns `null` on timeout. The returned string is copied to a Dart
  /// [String] and the native memory is freed.
  String? pollEvent(CodexRuntimeHandle handle, int timeoutMs) {
    return using((arena) {
      final errorPtr = arena<Pointer<Utf8>>(1);
      final ptr = _pollEvent(handle.ptr, timeoutMs, errorPtr);
      final error = _readAndFreeError(errorPtr, _freeString);
      if (error != null) {
        throw CodexFfiException(error);
      }

      if (ptr == nullptr) {
        return null;
      }

      final event = ptr.toDartString();
      _freeString(ptr);
      return event;
    });
  }
}
