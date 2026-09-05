import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:openai_codex/openai_codex.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

void main() {
  runApp(const CodexDesktopExampleApp());
}

/// Locates the `codex_ffi` shared library for the running process.
///
/// In a packaged build the DLL is expected next to the executable. During
/// development we fall back to the Rust `target/debug` build.
String _findLibrary() {
  final exeDir = File(Platform.resolvedExecutable).parent.path;
  final libName = Platform.isWindows
      ? 'codex_ffi.dll'
      : Platform.isMacOS
      ? 'libcodex_ffi.dylib'
      : 'libcodex_ffi.so';

  // Search the locations a packaged desktop build places the library.
  final candidates = <String>[
    p.join(exeDir, libName),
    // Linux bundle: <bundle>/lib
    p.join(exeDir, 'lib', libName),
    // macOS bundle: <app>.app/Contents/Frameworks
    p.normalize(p.join(exeDir, '..', 'Frameworks', libName)),
  ];
  for (final candidate in candidates) {
    if (File(candidate).existsSync()) {
      return candidate;
    }
  }

  final projectRoot = _findProjectRoot(exeDir);
  return p.normalize(
    p.join(projectRoot, 'codex-rs', 'target', 'debug', libName),
  );
}

String _findProjectRoot(String exeDir) {
  var dir = Directory(exeDir);
  // Walk up until we see `codex-rs` and `sdk`.
  for (var i = 0; i < 10; i++) {
    if (Directory(p.join(dir.path, 'codex-rs')).existsSync() &&
        Directory(p.join(dir.path, 'sdk')).existsSync()) {
      return dir.path;
    }
    final parent = dir.parent;
    if (parent.path == dir.path) {
      break;
    }
    dir = parent;
  }
  return exeDir;
}

class CodexDesktopExampleApp extends StatelessWidget {
  const CodexDesktopExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Codex FFI Desktop Example',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple),
        useMaterial3: true,
      ),
      home: const CodexExamplePage(),
    );
  }
}

class CodexExamplePage extends StatefulWidget {
  const CodexExamplePage({super.key});

  @override
  State<CodexExamplePage> createState() => _CodexExamplePageState();
}

class _CodexExamplePageState extends State<CodexExamplePage> {
  CodexClient? _client;
  final _events = <String>[];
  final _eventScroll = ScrollController();

  final _clientNameController = TextEditingController(text: 'flutter_desktop');
  final _clientVersionController = TextEditingController(text: '0.0.1');
  final _requestController = TextEditingController(
    text: const JsonEncoder().convert({
      'method': 'server/diagnostics',
      'id': 1,
      'params': {},
    }),
  );

  bool _starting = false;
  String? _status;

  @override
  void dispose() {
    _client?.destroy();
    _clientNameController.dispose();
    _clientVersionController.dispose();
    _requestController.dispose();
    _eventScroll.dispose();
    super.dispose();
  }

  Future<void> _startRuntime() async {
    setState(() {
      _starting = true;
      _status = 'Loading library...';
      _events.clear();
    });

    final libraryPath = _findLibrary();
    if (!File(libraryPath).existsSync()) {
      setState(() {
        _status =
            'Library not found: $libraryPath\nBuild with: cargo build -p codex-ffi';
        _starting = false;
      });
      return;
    }

    late final String codexHome;
    try {
      final docs = await getApplicationDocumentsDirectory();
      codexHome = p.join(docs.path, 'codex_desktop_example');
      await Directory(codexHome).create(recursive: true);
    } on Exception catch (e) {
      setState(() {
        _status = 'Failed to get documents directory: $e';
        _starting = false;
      });
      return;
    }

    try {
      final client = await CodexClient.start(
        libraryPath,
        CodexClientConfig(
          codexHome: codexHome,
          clientName: _clientNameController.text,
          clientVersion: _clientVersionController.text,
          experimentalApi: true,
        ),
      );

      client.events.listen(
        (event) {
          setState(() {
            _events.add(event);
            _scrollToBottom();
          });
        },
        onError: (Object e) {
          setState(() {
            _status = 'Event stream error: $e';
          });
        },
      );

      setState(() {
        _client = client;
        _status =
            'Runtime started. Library: $libraryPath\nCodex home: $codexHome';
        _starting = false;
      });
    } on CodexFfiException catch (e) {
      setState(() {
        _status = 'Failed to start runtime: $e';
        _starting = false;
      });
    }
  }

  void _stopRuntime() {
    _client?.destroy();
    setState(() {
      _client = null;
      _status = 'Runtime stopped';
    });
  }

  void _sendRequest() {
    final client = _client;
    if (client == null) {
      setState(() => _status = 'Start the runtime first');
      return;
    }

    final text = _requestController.text;
    try {
      jsonDecode(text);
    } on FormatException catch (e) {
      setState(() => _status = 'Invalid JSON: $e');
      return;
    }

    try {
      client.sendRequest(text);
      setState(() => _status = 'Sent request: $text');
    } on CodexFfiException catch (e) {
      setState(() => _status = 'Request failed: $e');
    }
  }

  void _sendNotification() {
    final client = _client;
    if (client == null) {
      setState(() => _status = 'Start the runtime first');
      return;
    }
    try {
      client.sendNotification(_requestController.text);
      setState(() => _status = 'Sent notification: ${_requestController.text}');
    } on CodexFfiException catch (e) {
      setState(() => _status = 'Notification failed: $e');
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_eventScroll.hasClients) {
        _eventScroll.animateTo(
          _eventScroll.position.maxScrollExtent,
          duration: const Duration(milliseconds: 200),
          curve: Curves.easeOut,
        );
      }
    });
  }

  String _prettyEvent(String event) {
    try {
      final value = jsonDecode(event);
      const encoder = JsonEncoder.withIndent('  ');
      return encoder.convert(value);
    } on FormatException {
      return event;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Codex FFI Desktop Example')),
      body: Padding(
        padding: const EdgeInsets.all(16.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _clientNameController,
                    decoration: const InputDecoration(labelText: 'Client name'),
                    enabled: _client == null,
                  ),
                ),
                const SizedBox(width: 16),
                Expanded(
                  child: TextField(
                    controller: _clientVersionController,
                    decoration: const InputDecoration(
                      labelText: 'Client version',
                    ),
                    enabled: _client == null,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Row(
              children: [
                ElevatedButton(
                  onPressed: _client == null && !_starting
                      ? _startRuntime
                      : null,
                  child: _starting
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Text('Start runtime'),
                ),
                const SizedBox(width: 8),
                ElevatedButton(
                  onPressed: _client != null ? _stopRuntime : null,
                  child: const Text('Stop runtime'),
                ),
              ],
            ),
            const SizedBox(height: 16),
            TextField(
              controller: _requestController,
              decoration: const InputDecoration(
                labelText: 'JSON request / notification',
                alignLabelWithHint: true,
              ),
              maxLines: 5,
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                ElevatedButton(
                  onPressed: _client != null ? _sendRequest : null,
                  child: const Text('Send request'),
                ),
                const SizedBox(width: 8),
                ElevatedButton(
                  onPressed: _client != null ? _sendNotification : null,
                  child: const Text('Send notification'),
                ),
              ],
            ),
            const SizedBox(height: 8),
            if (_status != null)
              Container(
                padding: const EdgeInsets.all(8),
                color: Colors.grey.shade100,
                child: SelectableText(
                  _status!,
                  style: const TextStyle(fontFamily: 'Consolas', fontSize: 12),
                ),
              ),
            const SizedBox(height: 8),
            const Text(
              'Events:',
              style: TextStyle(fontWeight: FontWeight.bold),
            ),
            Expanded(
              child: Container(
                padding: const EdgeInsets.all(8),
                color: Colors.black,
                child: ListView.builder(
                  controller: _eventScroll,
                  itemCount: _events.length,
                  itemBuilder: (context, index) {
                    return SelectableText(
                      _prettyEvent(_events[index]),
                      style: const TextStyle(
                        fontFamily: 'Consolas',
                        fontSize: 11,
                        color: Colors.lightGreen,
                      ),
                    );
                  },
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
