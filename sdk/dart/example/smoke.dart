import 'dart:convert';
import 'dart:io';

import 'package:openai_codex/openai_codex.dart';
import 'package:path/path.dart' as p;

/// Minimal command-line smoke test for the `openai_codex` Dart SDK.
///
/// Usage:
///   dart run example/smoke.dart <path-to-codex_ffi-shared-library>
///
/// If no library path is supplied, the script looks for the debug build next to
/// the `sdk/dart` package in this repo.
void main(List<String> args) async {
  final libraryPath = args.isNotEmpty
      ? args.first
      : p.normalize(p.join(
          Platform.script.toFilePath(),
          '..',
          '..',
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

  if (!File(libraryPath).existsSync()) {
    stderr.writeln('codex_ffi library not found at $libraryPath');
    stderr.writeln('Build it first with: cargo build -p codex-ffi');
    exit(1);
  }

  final codexHome = Directory.systemTemp.createTempSync('codex_dart_smoke_');

  final client = await CodexClient.start(
    libraryPath,
    CodexClientConfig(
      codexHome: codexHome.path,
      clientName: 'openai_codex_smoke',
      clientVersion: '0.0.1',
      experimentalApi: true,
    ),
  );

  client.events.listen((event) {
    final value = jsonDecode(event);
    const encoder = JsonEncoder.withIndent('  ');
    print(encoder.convert(value));
  });

  client.sendRequest(jsonEncode({
    'method': 'server/diagnostics',
    'id': 1,
    'params': {},
  }));

  // Wait for a few events to print, then exit.
  await Future.delayed(const Duration(seconds: 2));
  client.destroy();
  codexHome.deleteSync(recursive: true);
}
