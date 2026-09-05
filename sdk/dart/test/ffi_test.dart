import 'dart:convert';
import 'dart:io';

import 'package:openai_codex/openai_codex.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

String _findLibrary() {
  // From `sdk/dart`, the built cdylib is at `../../codex-rs/target/debug`.
  final candidate = p.normalize(p.join(
    Directory.current.path,
    '..',
    '..',
    'codex-rs',
    'target',
    'debug',
    Platform.isWindows
        ? 'codex_ffi.dll'
        : Platform.isMacOS
            ? 'libcodex_ffi.dylib'
            : 'libcodex_ffi.so',
  ));
  if (!File(candidate).existsSync()) {
    throw StateError('codex_ffi library not found at $candidate; '
        'build it with `cargo build -p codex-ffi` first.');
  }
  return candidate;
}

void main() {
  test('Dart client creates runtime and receives a response', () async {
    final codexHome = Directory.systemTemp.createTempSync('codex_dart_test_');

    final libraryPath = _findLibrary();
    final config = CodexClientConfig(
      codexHome: codexHome.path,
      clientName: 'openai_codex_dart_test',
      clientVersion: '0.0.1',
      experimentalApi: true,
    );

    final client = await CodexClient.start(libraryPath, config);

    // Send a content-free diagnostic request and collect its response.
    client.sendRequest(jsonEncode({
      'method': 'server/diagnostics',
      'id': 1,
      'params': {},
    }));

    final response = await client.events.firstWhere((e) {
      try {
        final value = jsonDecode(e) as Map<String, dynamic>;
        return value['id'] == 1;
      } on FormatException {
        return false;
      }
    }).timeout(Duration(seconds: 30));

    final responseJson = jsonDecode(response) as Map<String, dynamic>;
    expect(responseJson['id'], 1);
    expect(responseJson['result'], isNotNull);

    client.destroy();
    codexHome.deleteSync(recursive: true);
  });
}
