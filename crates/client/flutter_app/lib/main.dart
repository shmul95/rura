import 'dart:async';
import 'dart:io';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'frb/api.dart';
import 'frb/frb_generated.dart';

// App color palette
const kPrimary = Color(0xFFF06543);  // f06543
const kSecondary = Color(0xFF33CCC7); // 33ccc7
const kTertiary = Color(0xFFF09D51); // f09d51
const kBackground = Color(0xFFE0DFD5); // e0dfd5
const kDark = Color(0xFF313638);      // 313638

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    // Light scheme
    const lightScheme = ColorScheme(
      brightness: Brightness.light,
      primary: kPrimary,
      onPrimary: Colors.white,
      secondary: kSecondary,
      onSecondary: Colors.black,
      tertiary: kTertiary,
      onTertiary: Colors.black,
      error: Color(0xFFB00020),
      onError: Colors.white,
      background: kBackground,
      onBackground: kDark,
      surface: Colors.white,
      onSurface: kDark,
    );

    final lightTheme = ThemeData(
      useMaterial3: true,
      colorScheme: lightScheme,
      scaffoldBackgroundColor: lightScheme.background,
      appBarTheme: const AppBarTheme(
        backgroundColor: kPrimary,
        foregroundColor: Colors.white,
        centerTitle: false,
      ),
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          backgroundColor: kPrimary,
          foregroundColor: Colors.white,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          textStyle: const TextStyle(fontWeight: FontWeight.w600),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: kPrimary,
          side: const BorderSide(color: kPrimary, width: 1.4),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          textStyle: const TextStyle(fontWeight: FontWeight.w600),
        ),
      ),
      inputDecorationTheme: const InputDecorationTheme(
        filled: true,
        fillColor: Colors.white,
        labelStyle: TextStyle(color: kDark),
        hintStyle: TextStyle(color: Color(0x99313638)),
        border: OutlineInputBorder(),
        focusedBorder: OutlineInputBorder(
          borderSide: BorderSide(color: kPrimary, width: 1.8),
        ),
      ),
      floatingActionButtonTheme: const FloatingActionButtonThemeData(
        backgroundColor: kSecondary,
        foregroundColor: Colors.black,
      ),
      dividerTheme: DividerThemeData(color: kDark.withOpacity(0.12), thickness: 1),
      textTheme: const TextTheme().apply(
        bodyColor: kDark,
        displayColor: kDark,
      ),
    );

    // Dark scheme
    const darkScheme = ColorScheme(
      brightness: Brightness.dark,
      primary: kPrimary,
      onPrimary: Colors.white,
      secondary: kSecondary,
      onSecondary: Colors.black,
      tertiary: kTertiary,
      onTertiary: Colors.black,
      error: Color(0xFFCF6679),
      onError: Colors.black,
      background: kDark,
      onBackground: kBackground,
      surface: Color(0xFF202325),
      onSurface: kBackground,
    );

    final darkTheme = ThemeData(
      useMaterial3: true,
      colorScheme: darkScheme,
      scaffoldBackgroundColor: darkScheme.background,
      appBarTheme: const AppBarTheme(
        backgroundColor: kPrimary,
        foregroundColor: Colors.white,
        centerTitle: false,
      ),
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          backgroundColor: kPrimary,
          foregroundColor: Colors.white,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          textStyle: const TextStyle(fontWeight: FontWeight.w600),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: kSecondary,
          side: const BorderSide(color: kSecondary, width: 1.4),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          textStyle: const TextStyle(fontWeight: FontWeight.w600),
        ),
      ),
      inputDecorationTheme: const InputDecorationTheme(
        filled: true,
        fillColor: Color(0xFF2B2F31),
        labelStyle: TextStyle(color: kBackground),
        hintStyle: TextStyle(color: Color(0x99E0DFD5)),
        border: OutlineInputBorder(),
        focusedBorder: OutlineInputBorder(
          borderSide: BorderSide(color: kPrimary, width: 1.8),
        ),
      ),
      floatingActionButtonTheme: const FloatingActionButtonThemeData(
        backgroundColor: kSecondary,
        foregroundColor: Colors.black,
      ),
      dividerTheme: DividerThemeData(color: kBackground.withOpacity(0.12), thickness: 1),
      textTheme: const TextTheme().apply(
        bodyColor: kBackground,
        displayColor: kBackground,
      ),
    );

    return MaterialApp(
      title: 'Rura Client',
      theme: lightTheme,
      darkTheme: darkTheme,
      themeMode: ThemeMode.system,
      home: const HomePage(),
    );
  }
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});
  @override
  State<HomePage> createState() => _HomePageState();
}

class SessionConfig {
  final String host;
  final int port;
  final String caPem;
  final String passphrase;
  final String password;
  const SessionConfig({
    required this.host,
    required this.port,
    required this.caPem,
    required this.passphrase,
    required this.password,
  });
}

class _HomePageState extends State<HomePage> {
  final _host = TextEditingController(text: 'localhost');
  final _port = TextEditingController(text: '8443');
  final _certPath = TextEditingController(text: '../../../certs/ca.crt');
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
              limit: BigInt.from(200),
            )
          : await loginAndFetchHistoryTls(
              host: host,
              port: port,
              caPem: caPem,
              passphrase: pass,
              password: pwd,
              limit: BigInt.from(200),
            );

      if (!bundle.success) {
        setState(() => _status = bundle.message);
        return;
      }

      if (!mounted) return;
      final session = SessionConfig(
        host: host,
        port: port,
        caPem: caPem,
        passphrase: pass,
        password: pwd,
      );
      Navigator.of(context).push(
        MaterialPageRoute(
          builder: (_) => ChatListPage(bundle: bundle, session: session),
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
            Text(_status, style: Theme.of(context).textTheme.bodyMedium),
          ],
        ),
      ),
    );
  }
}

class ChatListPage extends StatelessWidget {
  final HistoryBundle bundle;
  final SessionConfig session;
  const ChatListPage({super.key, required this.bundle, required this.session});

  @override
  Widget build(BuildContext context) => _ChatListScaffold(bundle: bundle, session: session);

  static Future<int?> _promptForUserId(BuildContext context) async {
    final ctrl = TextEditingController();
    return showDialog<int>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('New chat'),
        content: TextField(
          controller: ctrl,
          keyboardType: TextInputType.number,
          decoration: const InputDecoration(labelText: 'User id'),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          ElevatedButton(
            onPressed: () {
              final v = int.tryParse(ctrl.text.trim());
              Navigator.pop(ctx, v);
            },
            child: const Text('Start'),
          ),
        ],
      ),
    );
  }
}

class _ChatListScaffold extends StatefulWidget {
  final HistoryBundle bundle;
  final SessionConfig session;
  const _ChatListScaffold({required this.bundle, required this.session});
  @override
  State<_ChatListScaffold> createState() => _ChatListScaffoldState();
}

class _ChatListScaffoldState extends State<_ChatListScaffold> {
  late final int _selfId;
  late final Map<int, List<HistoryMessage>> _groups;
  StreamSubscription<String>? _sub;
  final _incoming = StreamController<HistoryMessage>.broadcast();

  @override
  void initState() {
    super.initState();
    _selfId = widget.bundle.userId ?? 0;
    _groups = <int, List<HistoryMessage>>{};
    for (final m in widget.bundle.messages) {
      final peer = m.fromUserId == _selfId ? m.toUserId : m.fromUserId;
      _groups.putIfAbsent(peer, () => []).add(m);
    }
    _startStream();
  }

  void _startStream() {
    final s = widget.session;
    final stream = openMessageStreamTls(
      host: s.host,
      port: s.port,
      caPem: s.caPem,
      passphrase: s.passphrase,
      password: s.password,
    );
    _sub = stream.listen((data) {
      try {
        final map = jsonDecode(data) as Map;
        final from = map['from_user_id'] as int;
        final body = map['body'] as String;
        final msg = HistoryMessage(
          id: 0,
          fromUserId: from,
          toUserId: _selfId,
          body: body,
          timestamp: DateTime.now().toIso8601String(),
          saved: false,
        );
        _incoming.add(msg);
        final peer = from;
        setState(() {
          _groups.putIfAbsent(peer, () => []);
          _groups[peer]!.add(msg);
        });
      } catch (_) {
        // ignore malformed event
      }
    }, onError: (_) {});
  }

  @override
  void dispose() {
    _sub?.cancel();
    _incoming.close();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final items = _groups.entries.toList()
      ..sort((a, b) {
        final at = DateTime.tryParse(a.value.last.timestamp) ?? DateTime(0);
        final bt = DateTime.tryParse(b.value.last.timestamp) ?? DateTime(0);
        return bt.compareTo(at);
      });
    return Scaffold(
      appBar: AppBar(title: const Text('Chats')),
      body: ListView.separated(
        itemCount: items.length,
        separatorBuilder: (_, __) => const Divider(height: 1),
        itemBuilder: (context, index) {
          final peerId = items[index].key;
          final msgs = items[index].value;
          final last = msgs.last;
          return ListTile(
            leading: const CircleAvatar(
              backgroundColor: kSecondary,
              foregroundColor: Colors.black,
              child: Icon(Icons.person),
            ),
            title: Text('User $peerId'),
            subtitle: Text(last.body, maxLines: 1, overflow: TextOverflow.ellipsis),
            trailing: Text(_formatTime(last.timestamp), style: Theme.of(context).textTheme.bodySmall),
            onTap: () {
              Navigator.of(context).push(
                MaterialPageRoute(
                  builder: (_) => ChatPage(
                    session: widget.session,
                    selfUserId: _selfId,
                    peerUserId: peerId,
                    initial: msgs,
                    inbound: _incoming.stream,
                  ),
                ),
              );
            },
          );
        },
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: () async {
          final peer = await ChatListPage._promptForUserId(context);
          if (peer == null) return;
          Navigator.of(context).push(
            MaterialPageRoute(
              builder: (_) => ChatPage(
                session: widget.session,
                selfUserId: _selfId,
                peerUserId: peer,
                initial: const [],
                inbound: _incoming.stream,
              ),
            ),
          );
        },
        child: const Icon(Icons.chat),
      ),
    );
  }
}

class ChatPage extends StatefulWidget {
  final SessionConfig session;
  final int selfUserId;
  final int peerUserId;
  final List<HistoryMessage> initial;
  final Stream<HistoryMessage>? inbound;
  const ChatPage({super.key, required this.session, required this.selfUserId, required this.peerUserId, required this.initial, this.inbound});

  @override
  State<ChatPage> createState() => _ChatPageState();
}

class _ChatPageState extends State<ChatPage> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  bool _sending = false;
  late List<HistoryMessage> _messages;
  StreamSubscription<HistoryMessage>? _inSub;

  @override
  void initState() {
    super.initState();
    _messages = List.of(widget.initial);
    _inSub = widget.inbound?.listen((m) {
      if (m.fromUserId == widget.peerUserId) {
        setState(() => _messages.add(m));
        if (_scroll.hasClients) {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (_scroll.hasClients) {
              _scroll.jumpTo(_scroll.position.maxScrollExtent + 80);
            }
          });
        }
      }
    });
  }

  Future<void> _send() async {
    final text = _input.text.trim();
    if (text.isEmpty) return;
    setState(() => _sending = true);
    try {
      await sendDirectMessageOverStream(
        userId: widget.selfUserId,
        toUserId: widget.peerUserId,
        body: text,
        saved: false,
      );
      final now = DateTime.now().toIso8601String();
      setState(() {
        _messages.add(HistoryMessage(
          id: 0,
          fromUserId: widget.selfUserId,
          toUserId: widget.peerUserId,
          body: text,
          timestamp: now,
          saved: false,
        ));
        _input.clear();
      });
      await Future.delayed(const Duration(milliseconds: 50));
      if (_scroll.hasClients) {
        _scroll.jumpTo(_scroll.position.maxScrollExtent + 80);
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  void dispose() {
    _inSub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final self = widget.selfUserId;
    final msgs = _messages.where((m) => m.fromUserId == widget.peerUserId || m.toUserId == widget.peerUserId).toList();
    return Scaffold(
      appBar: AppBar(
        title: Text('User ${widget.peerUserId}'),
      ),
      body: Column(
        children: [
          Expanded(
            child: ListView.builder(
              controller: _scroll,
              padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
              itemCount: msgs.length,
              itemBuilder: (context, index) {
                final m = msgs[index];
                final fromSelf = m.fromUserId == self;
                return Align(
                  alignment: fromSelf ? Alignment.centerRight : Alignment.centerLeft,
                  child: Container(
                    margin: const EdgeInsets.symmetric(vertical: 4),
                    padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
                    constraints: BoxConstraints(maxWidth: MediaQuery.of(context).size.width * 0.7),
                    decoration: BoxDecoration(
                      color: fromSelf ? kPrimary : kSecondary,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Column(
                      crossAxisAlignment: fromSelf ? CrossAxisAlignment.end : CrossAxisAlignment.start,
                      children: [
                        Text(
                          m.body,
                          style: TextStyle(
                            color: fromSelf ? Colors.white : Colors.black,
                          ),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          _formatTime(m.timestamp),
                          style: Theme.of(context)
                              .textTheme
                              .bodySmall
                              ?.copyWith(color: fromSelf ? Colors.white70 : const Color(0xCC000000)),
                        ),
                      ],
                    ),
                  ),
                );
              },
            ),
          ),
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.all(8),
              child: Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: _input,
                      decoration: const InputDecoration(
                        hintText: 'Type a message',
                        border: OutlineInputBorder(),
                        isDense: true,
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  IconButton(
                    onPressed: _sending ? null : _send,
                    icon: _sending
                        ? const SizedBox(
                            width: 18,
                            height: 18,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.send),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

String _two(int x) => x.toString().padLeft(2, '0');
String _formatTime(String iso) {
  final dt = DateTime.tryParse(iso);
  if (dt == null) return '';
  final now = DateTime.now();
  if (dt.year == now.year && dt.month == now.month && dt.day == now.day) {
    return '${_two(dt.hour)}:${_two(dt.minute)}';
  }
  return '${dt.year}-${_two(dt.month)}-${_two(dt.day)}';
}
