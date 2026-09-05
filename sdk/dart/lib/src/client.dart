import 'dart:async';
import 'dart:convert';

import 'ffi.dart';

/// Configuration for [CodexClient].
class CodexClientConfig {
  final String? codexHome;
  final String? configToml;
  final String? clientName;
  final String? clientVersion;
  final String? clientTitle;
  final String? codexSelfExe;
  final bool? experimentalApi;

  CodexClientConfig({
    this.codexHome,
    this.configToml,
    this.clientName,
    this.clientVersion,
    this.clientTitle,
    this.codexSelfExe,
    this.experimentalApi,
  });

  /// Serializes to the JSON configuration expected by `codex_runtime_create`.
  String toJson() => jsonEncode({
        if (codexHome != null) 'codexHome': codexHome,
        if (configToml != null) 'configToml': configToml,
        if (clientName != null) 'clientName': clientName,
        if (clientVersion != null) 'clientVersion': clientVersion,
        if (clientTitle != null) 'clientTitle': clientTitle,
        if (codexSelfExe != null) 'codexSelfExe': codexSelfExe,
        if (experimentalApi != null) 'experimentalApi': experimentalApi,
      });
}

/// High-level Dart client for an in-process Codex runtime.
///
/// The client owns the native runtime handle and polls the event stream on a
/// [Timer] in the calling isolate. This avoids background-isolate FFI
/// concurrency issues and is appropriate for Flutter's main thread.
class CodexClient {
  final CodexFfi _ffi;
  final CodexRuntimeHandle _handle;
  final StreamController<String> _eventController;
  Timer? _poller;

  /// Stream of JSON-encoded app-server events and responses.
  Stream<String> get events => _eventController.stream;

  CodexClient._(this._ffi, this._handle)
      : _eventController = StreamController<String>.broadcast();

  /// Loads the native `codex_ffi` library at [libraryPath], starts the runtime
  /// with [config], and begins polling for events.
  static Future<CodexClient> start(
      String libraryPath, CodexClientConfig config) async {
    final ffi = CodexFfi.load(libraryPath);
    final handle = ffi.createRuntime(config.toJson());
    final client = CodexClient._(ffi, handle);
    client._startPolling();
    return client;
  }

  void _startPolling() {
    _poller = Timer.periodic(const Duration(milliseconds: 16), (_) {
      try {
        final event = _ffi.pollEvent(_handle, 0);
        if (event != null) {
          _eventController.add(event);
        }
      } on CodexFfiException catch (e) {
        _eventController.addError(e);
      }
    });
  }

  /// Sends a JSON-RPC request. The response is delivered as an event on
  /// [events].
  void sendRequest(String json) => _ffi.sendRequest(_handle, json);

  /// Sends a JSON-RPC notification.
  void sendNotification(String json) => _ffi.sendNotification(_handle, json);

  /// Responds to a server request with a JSON-RPC response or error object.
  void respondToServerRequest(String json) =>
      _ffi.respondToServerRequest(_handle, json);

  /// Stops the event poller and destroys the native runtime.
  void destroy() {
    _poller?.cancel();
    _ffi.destroyRuntime(_handle);
    _eventController.close();
  }
}
