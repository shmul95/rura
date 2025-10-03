#!/usr/bin/env bash
set -euo pipefail

# Scaffolds a minimal Flutter app inside this repo at crates/client/flutter_app
# and wires flutter_rust_bridge bindings to the Rust client crate.
#
# Requirements: Flutter SDK + Dart in PATH; network available for pub add.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT_DIR/crates/client/flutter_app"
RUST_INPUT="$ROOT_DIR/crates/client/src/lib.rs"
DART_OUTPUT="$APP_DIR/lib/bridge_generated.dart"
RUST_OUTPUT="$ROOT_DIR/crates/client/src/bridge_generated.rs"

echo "Creating Flutter app at: $APP_DIR"
rm -rf "$APP_DIR"
flutter create --platforms=linux,macos,windows "$APP_DIR"

pushd "$APP_DIR" >/dev/null

echo "Adding flutter_rust_bridge + ffi to Flutter app"
flutter pub add flutter_rust_bridge ffi

echo "Writing login + history navigation main.dart"
cat > lib/main.dart <<'DART'
import 'dart:io';
import 'package:flutter/material.dart';
import 'frb/api.dart';
import 'frb/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Rura Client',
      theme: ThemeData(colorSchemeSeed: Colors.blue, useMaterial3: true),
      home: const HomePage(),
    );
  }
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});
  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final _host = TextEditingController(text: 'localhost');
  final _port = TextEditingController(text: '8443');
  final _certPath = TextEditingController(text: '../../server/server.crt');
  final _passphrase = TextEditingController(text: 'alice');
  final _password = TextEditingController(text: 'secret');
  String _status = 'Ready';

  Future<void> _authAndShowHistory({required bool register}) async {
    setState(() => _status = register ? 'Registering...' : 'Logging in...');
    try {
      final host = _host.text.trim();
      final port = int.tryParse(_port.text.trim()) ?? 8443;
      final caPem = await File(_certPath.text.trim()).readAsString();
      final pass = _passphrase.text;
      final pwd = _password.text;

      final bundle = register
          ? await registerAndFetchHistoryTls(
              host: host,
              port: port,
              caPem: caPem,
              passphrase: pass,
              password: pwd,
              limit: 200,
            )
          : await loginAndFetchHistoryTls(
              host: host,
              port: port,
              caPem: caPem,
              passphrase: pass,
              password: pwd,
              limit: 200,
            );

      if (!bundle.success) {
        setState(() => _status = bundle.message);
        return;
      }

      if (!mounted) return;
      Navigator.of(context).push(
        MaterialPageRoute(
          builder: (_) => HistoryPage(bundle: bundle),
        ),
      );
    } catch (e) {
      setState(() => _status = '${register ? 'Register' : 'Login'} failed: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Rura Client')),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            TextField(controller: _host, decoration: const InputDecoration(labelText: 'Host')),
            TextField(controller: _port, decoration: const InputDecoration(labelText: 'Port')),
            TextField(controller: _certPath, decoration: const InputDecoration(labelText: 'Cert PEM path')),
            const SizedBox(height: 12),
            TextField(controller: _passphrase, decoration: const InputDecoration(labelText: 'Passphrase')),
            TextField(controller: _password, decoration: const InputDecoration(labelText: 'Password'), obscureText: true),
            const SizedBox(height: 16),
            Row(
              children: [
                ElevatedButton.icon(
                  onPressed: () => _authAndShowHistory(register: false),
                  icon: const Icon(Icons.login),
                  label: const Text('Login'),
                ),
                const SizedBox(width: 12),
                OutlinedButton.icon(
                  onPressed: () => _authAndShowHistory(register: true),
                  icon: const Icon(Icons.app_registration),
                  label: const Text('Register'),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Text(_status),
          ],
        ),
      ),
    );
  }
}

class HistoryPage extends StatelessWidget {
  final HistoryBundle bundle;
  const HistoryPage({super.key, required this.bundle});

  @override
  Widget build(BuildContext context) {
    final msgs = bundle.messages;
    return Scaffold(
      appBar: AppBar(title: Text('History (total: ${msgs.length})')),
      body: ListView.separated(
        itemCount: msgs.length,
        separatorBuilder: (_, __) => const Divider(height: 1),
        itemBuilder: (context, index) {
          final m = msgs[index];
          return ListTile(
            title: Text(m.body),
            subtitle: Text('from ${m.fromUserId} → ${m.toUserId} • ${m.timestamp}'),
            trailing: m.saved == true ? const Icon(Icons.bookmark, size: 18) : null,
          );
        },
      ),
    );
  }
}
DART

popd >/dev/null

echo "Running FRB codegen"
"$ROOT_DIR/scripts/frb_codegen.sh" "$APP_DIR" "$ROOT_DIR"

cat <<'INFO'

Done.

Next steps:
1) Build Rust client (if not already):
   cargo build -p rura_client
2) Set dynamic library path so Flutter finds librura_client:
   Linux:  export LD_LIBRARY_PATH="$ROOT_DIR/target/debug:">${LD_LIBRARY_PATH:-}
   macOS:  export DYLD_LIBRARY_PATH="$ROOT_DIR/target/debug:">${DYLD_LIBRARY_PATH:-}
   Windows: set PATH to include %REPO%\target\debug
3) Run the app:
   cd "$APP_DIR" && flutter run -d linux   # or -d macos / -d windows

To hook up the Rust hello() call, import 'bridge_generated.dart' in main.dart
and call the generated hello() function when pressing the button.
INFO
