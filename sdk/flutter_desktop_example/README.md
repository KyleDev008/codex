# flutter_desktop_example

A minimal Flutter desktop app that embeds the in-process Codex app-server
through `openai_codex` and the `codex_ffi` Rust cdylib.

## Build

1. Build the Rust FFI cdylib from the workspace root:

   ```sh
   cargo build -p codex-ffi
   ```

   For a Release build use `--release` and ensure the corresponding platform
   CMake config points at `target/release`.

2. Get Dart/Flutter dependencies:

   ```sh
   cd sdk/flutter_desktop_example
   flutter pub get
   ```

3. Run on the desired desktop platform:

   ```sh
   flutter run -d windows
   # or -d macos, -d linux
   ```

   The desktop builds copy the `codex_ffi` cdylib automatically:
   - Windows: `windows/CMakeLists.txt` copies `codex_ffi.dll` next to the exe.
   - Linux: `linux/CMakeLists.txt` copies `libcodex_ffi.so` into `bundle/lib`.
   - macOS: `Runner.xcscheme` adds a post-build step that copies
     `libcodex_ffi.dylib` into `<app>.app/Contents/Frameworks`.

## Use

- Fill in **Client name** and **Client version**.
- Tap **Start runtime**. The app will locate the cdylib, create a Codex home
  directory under the documents folder, and start the in-process server.
- The **Events** pane will show JSON-RPC events from the server.
- Edit the **JSON request / notification** field and tap **Send request** or
  **Send notification**. The default request is `server/diagnostics`.

## Architecture

- `lib/main.dart` uses `package:openai_codex` to load `codex_ffi` and drive
  the C ABI.
- `CodexClient.start()` returns a stream of JSON strings from
  `codex_poll_event_json`.
- Requests and notifications are passed straight through the JSON wire so any
  app-server method can be exercised from the UI without regenerating models.
