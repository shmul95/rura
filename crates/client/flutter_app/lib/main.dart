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
  // Default to the local CA certificate used to sign the server leaf
  final _certPath = TextEditingController(text: '../../../certs/ca.crt');
  final _passphrase = TextEditingController(text: 'alice');
  final _password = TextEditingController(text: 'secret');
  String _status = 'Ready';
  bool _isRegister = false;

  Future<void> _login() async {
    setState(() => _status = 'Logging in...');
    try {
      final host = _host.text.trim();
      final port = int.tryParse(_port.text.trim()) ?? 8443;
      final caPem = await File(_certPath.text.trim()).readAsString();
      final pass = _passphrase.text;
      final pwd = _password.text;
      final resp = _isRegister
          ? await registerTls(
              host: host,
              port: port,
              caPem: caPem,
              passphrase: pass,
              password: pwd,
            )
          : await loginTls(
              host: host,
              port: port,
              caPem: caPem,
              passphrase: pass,
              password: pwd,
            );
      setState(() => _status =
          'success=${resp.success} user_id=${resp.userId ?? 'null'} msg=${resp.message}');
    } catch (e) {
      setState(() => _status = 'Login failed: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Rura Client Login')),
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
            const SizedBox(height: 12),
            Row(children: [
              ChoiceChip(
                label: const Text('Login'),
                selected: !_isRegister,
                onSelected: (_) => setState(() => _isRegister = false),
              ),
              const SizedBox(width: 8),
              ChoiceChip(
                label: const Text('Register'),
                selected: _isRegister,
                onSelected: (_) => setState(() => _isRegister = true),
              ),
            ]),
            const SizedBox(height: 16),
            Row(
              children: [
                ElevatedButton.icon(
                  onPressed: _login,
                  icon: const Icon(Icons.login),
                  label: Text(_isRegister ? 'Register' : 'Login'),
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
