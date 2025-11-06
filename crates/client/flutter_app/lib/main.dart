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

// Compile-time flag passed via `flutter run --dart-define=REQUIRE_E2EE=true`
const bool kRequireE2EE = bool.fromEnvironment('REQUIRE_E2EE', defaultValue: true);

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
  final _host = TextEditingController(text: '127.0.0.1');
  final _port = TextEditingController(text: '8443');
  final _certPath = TextEditingController(text: '../../certs/ca.crt');
  final _password = TextEditingController(text: 'secret');
  String _status = 'Ready';

  Future<void> _authAndShowHistory({required bool register}) async {
    setState(() => _status = register ? 'Registering...' : 'Logging in...');
    try {
      final host = _host.text.trim();
      final port = int.tryParse(_port.text.trim()) ?? 8443;
      final caPem = await File(_certPath.text.trim()).readAsString();
      final pass = '';
      final pwd = _password.text;

      // Stream-first login: open the persistent stream (this logs in inside Rust)
      final rawStream = register
          ? openMessageStreamRegisterTls(
              host: host,
              port: port,
              caPem: caPem,
              passphrase: pass,
              password: pwd,
            )
          : openMessageStreamTls(
              host: host,
              port: port,
              caPem: caPem,
              passphrase: pass,
              password: pwd,
            );
      // Convert to broadcast so we can await first() and also listen() later
      final stream = rawStream.asBroadcastStream();

      // Wait for the initial auth_ok event to get user_id
      final first = await stream.first.timeout(const Duration(seconds: 5));
      final firstMap = jsonDecode(first) as Map;
      if (firstMap['type'] != 'auth_ok') {
        setState(() => _status = 'Unexpected first event from stream');
        return;
      }
      final userId = firstMap['user_id'] as int;

      // Load initial history from local cache to avoid re-login overwriting
      // the active stream route on the server.
      final history = await loadLocalHistory(limit: BigInt.from(500));
      final bundle = HistoryBundle(
        success: true,
        message: 'OK',
        userId: userId,
        messages: history,
      );

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
          builder: (_) => ChatListPage(bundle: bundle, session: session, incoming: stream),
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
            if (kRequireE2EE)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Text(
                  'E2EE enforced: messages must be opaque envelopes',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            // Server connection (optional): host/port/CA for online mode
            TextField(controller: _host, decoration: const InputDecoration(labelText: 'Server host (e.g., 127.0.0.1)')),
            const SizedBox(height: 8),
            TextField(controller: _port, decoration: const InputDecoration(labelText: 'Server port (e.g., 8443)'), keyboardType: TextInputType.number),
            const SizedBox(height: 8),
            TextField(controller: _certPath, decoration: const InputDecoration(labelText: 'CA cert path (e.g., certs/ca.crt)')),
            const SizedBox(height: 12),
            Row(children: [
              ElevatedButton.icon(
                onPressed: () => _authAndShowHistory(register: false),
                icon: const Icon(Icons.login),
                label: const Text('Login (Server)'),
              ),
              const SizedBox(width: 12),
              OutlinedButton.icon(
                onPressed: () => _authAndShowHistory(register: true),
                icon: const Icon(Icons.person_add_alt_1),
                label: const Text('Register (Server)'),
              ),
            ]),
            const Divider(height: 24),
            // Only ask for password to unlock local DB
            TextField(controller: _password, decoration: const InputDecoration(labelText: 'Password'), obscureText: true),
            const SizedBox(height: 16),
            Row(children: [
              ElevatedButton.icon(
                onPressed: _unlockAndShowHistory,
                icon: const Icon(Icons.lock_open),
                label: const Text('Unlock Local'),
              ),
              const SizedBox(width: 12),
              OutlinedButton.icon(
                onPressed: _registerLocal,
                icon: const Icon(Icons.app_registration),
                label: const Text('Register Local'),
              ),
            ]),
            const SizedBox(height: 16),
            Text(_status, style: Theme.of(context).textTheme.bodyMedium),
          ],
        ),
      ),
    );
  }
}

extension on Stream<String> {
  Stream<String> asEmptyBroadcast() => const Stream<String>.empty().asBroadcastStream();
}

extension on Stream<String>? {
  Stream<String> orEmptyBroadcast() =>
      (this ?? const Stream<String>.empty()).asBroadcastStream();
}

// Derive a stable numeric id from a base64 identity for local storage grouping.
int idToNumeric(String id) {
  try {
    final bytes = base64.decode(id);
    if (bytes.length >= 8) {
      var v = 0;
      for (var i = 0; i < 8; i++) {
        v = (v << 8) | (bytes[i] & 0xFF);
      }
      return v & 0x7FFFFFFFFFFFFFFF; // positive 63-bit
    }
  } catch (_) {}
  return id.hashCode;
}

class ChatListPage extends StatelessWidget {
  final HistoryBundle bundle;
  final SessionConfig session;
  final Stream<String>? incoming;
  const ChatListPage({super.key, required this.bundle, required this.session, this.incoming});

  @override
  Widget build(BuildContext context) => _ChatListScaffold(bundle: bundle, session: session, incoming: incoming);

  static Future<dynamic> _promptForUserId(BuildContext context) async {
    final idCtrl = TextEditingController();
    final pkCtrl = TextEditingController();
    final nickCtrl = TextEditingController();
    return showDialog<dynamic>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Add contact'),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: idCtrl,
                keyboardType: TextInputType.text,
                decoration: const InputDecoration(labelText: 'Recipient ID (base64)'),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: pkCtrl,
                keyboardType: TextInputType.text,
                decoration: const InputDecoration(labelText: 'Recipient Public Key (base64)'),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: nickCtrl,
                keyboardType: TextInputType.text,
                decoration: const InputDecoration(labelText: 'Surname (who is this person?)'),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          ElevatedButton(
            onPressed: () {
              final rid = idCtrl.text.trim();
              final pk = pkCtrl.text.trim();
              final nk = nickCtrl.text.trim();
              if (rid.isNotEmpty && pk.isNotEmpty) {
                Navigator.pop(ctx, { 'rid': rid, 'pk': pk, 'nk': nk });
              } else {
                Navigator.pop(ctx, null);
              }
            },
            child: const Text('Add'),
          ),
        ],
      ),
    );
  }
}

class _ChatListScaffold extends StatefulWidget {
  final HistoryBundle bundle;
  final SessionConfig session;
  final Stream<String>? incoming;
  const _ChatListScaffold({required this.bundle, required this.session, this.incoming});
  @override
  State<_ChatListScaffold> createState() => _ChatListScaffoldState();
}

class _ChatListScaffoldState extends State<_ChatListScaffold> {
  late final int _selfId;
  late final Map<int, List<HistoryMessage>> _groups;
  StreamSubscription<String>? _sub;
  final _incoming = StreamController<HistoryMessage>.broadcast();
  Map<int, String> _nicknames = {};
  final Map<int, String> _identityByPeer = {};

  String? _reverseIdentityFor(int peer) => _identityByPeer[peer];

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

  Future<void> _reloadFromLocal() async {
    try {
      final list = await loadLocalHistory(limit: BigInt.from(500));
      final groups = <int, List<HistoryMessage>>{};
      for (final m in list) {
        final peer = m.fromUserId == _selfId ? m.toUserId : m.fromUserId;
        groups.putIfAbsent(peer, () => []).add(m);
      }
      setState(() => _groups = groups);
    } catch (_) {}
  }

  void _startStream() {
    final stream = widget.incoming ?? openMessageStreamTls(
      host: widget.session.host,
      port: widget.session.port,
      caPem: widget.session.caPem,
      passphrase: widget.session.passphrase,
      password: widget.session.password,
    );
    _sub = stream.listen((data) async {
      try {
        final map = jsonDecode(data) as Map;
        if (map['type'] == 'auth_ok') {
          // Already handled by HomePage; ignore here
          return;
        }
        if (map.containsKey('from_identity')) {
          // Identity-based events are handled in ChatIdentityPage
          return;
        }
        final from = map['from_user_id'] as int;
        final bodyRaw = map['body'] as String;
        final body = _decodeEnvelope(bodyRaw);
        final now = DateTime.now().toIso8601String();
        // Persist to local cache
        await appendLocalMessage(
          fromUserId: from,
          toUserId: _selfId,
          body: body,
          timestamp: now,
        );
        final msg = HistoryMessage(
          id: 0,
          fromUserId: from,
          toUserId: _selfId,
          body: body,
          timestamp: now,
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
    // Merge conversations from messages with manually added contacts (no messages yet)
    final Map<int, List<HistoryMessage>> merged = {}..addAll(_groups);
    for (final peer in _nicknames.keys) {
      merged.putIfAbsent(peer, () => <HistoryMessage>[]);
    }
    final items = merged.entries.toList()
      ..sort((a, b) {
        final at = a.value.isNotEmpty
            ? DateTime.tryParse(a.value.last.timestamp) ?? DateTime(0)
            : DateTime(0);
        final bt = b.value.isNotEmpty
            ? DateTime.tryParse(b.value.last.timestamp) ?? DateTime(0)
            : DateTime(0);
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
          final last = msgs.isNotEmpty ? msgs.last : null;
          return ListTile(
            leading: const CircleAvatar(
              backgroundColor: kSecondary,
              foregroundColor: Colors.black,
              child: Icon(Icons.person),
            ),
            title: Text(_nicknames[peerId] ?? 'User $peerId'),
            subtitle: Text(last?.body ?? '', maxLines: 1, overflow: TextOverflow.ellipsis),
            trailing: Text(
              last != null ? _formatTime(last.timestamp) : '',
              style: Theme.of(context).textTheme.bodySmall,
            ),
            onTap: () async {
              // If we have an identity for this peer (because we added the contact), open identity chat.
              final rid = _reverseIdentityFor(peerId);
              if (rid != null) {
                await Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => ChatIdentityPage(
                      session: widget.session,
                      selfUserId: _selfId,
                      recipientId: rid,
                      recipientPubKey: '',
                      incomingRaw: widget.incoming ?? openMessageStreamTls(
                        host: widget.session.host,
                        port: widget.session.port,
                        caPem: widget.session.caPem,
                        passphrase: widget.session.passphrase,
                        password: widget.session.password,
                      ),
                    ),
                  ),
                );
                await _reloadFromLocal();
              } else {
                await Navigator.of(context).push(
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
              }
            },
          );
        },
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: () async {
          final sel = await ChatListPage._promptForUserId(context);
          if (sel == null) return;
          if (sel is Map && sel['rid'] is String && sel['pk'] is String) {
            final rid = sel['rid'] as String;
            final pk = sel['pk'] as String;
            final nk = (sel['nk'] as String?)?.trim();
            try { await addContact(userId: rid, pubkey: pk); } catch (_) {}
            final peer = idToNumeric(rid);
            setState(() {
              if ((nk?.isEmpty ?? true) == false) {
                _nicknames[peer] = nk!;
              }
              _identityByPeer[peer] = rid;
              _groups.putIfAbsent(peer, () => <HistoryMessage>[]);
            });
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text('Contact added: ' + (_nicknames[peer] ?? rid.substring(0, 10) + '…'))),
            );
          } else if (sel is int) {
            await Navigator.of(context).push(
              MaterialPageRoute(
                builder: (_) => ChatPage(
                  session: widget.session,
                  selfUserId: _selfId,
                  peerUserId: sel,
                  initial: const [],
                  inbound: _incoming.stream,
                ),
              ),
            );
            await _reloadFromLocal();
          }
        },
        child: const CircleAvatar(
          backgroundColor: kSecondary,
          foregroundColor: Colors.black,
          child: Icon(Icons.person_add_alt_1),
        ),
      ),
    );
  }
}

class ChatIdentityPage extends StatefulWidget {
  final SessionConfig session;
  final int selfUserId;
  final String recipientId;
  final String recipientPubKey;
  final Stream<String> incomingRaw;
  const ChatIdentityPage({super.key, required this.session, required this.selfUserId, required this.recipientId, required this.recipientPubKey, required this.incomingRaw});
  @override
  State<ChatIdentityPage> createState() => _ChatIdentityPageState();
}

class _ChatIdentityPageState extends State<ChatIdentityPage> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  bool _sending = false;
  final List<HistoryMessage> _messages = [];
  StreamSubscription<String>? _sub;
  // Use top-level idToNumeric()

  @override
  void initState() {
    super.initState();
    _sub = widget.incomingRaw.listen((data) async {
      try {
        final map = jsonDecode(data) as Map;
        if (map['type'] == 'auth_ok') return;
        final fromId = (map['from_identity'] ?? '').toString();
        final bodyRaw = map['body'] as String? ?? '';
        if (fromId == widget.recipientId) {
          final body = _decodeEnvelope(bodyRaw);
          final now = DateTime.now().toIso8601String();
          final peer = idToNumeric(widget.recipientId);
          await appendLocalMessage(
            fromUserId: peer,
            toUserId: widget.selfUserId,
            body: body,
            timestamp: now,
          );
          setState(() => _messages.add(HistoryMessage(
            id: 0,
            fromUserId: peer,
            toUserId: widget.selfUserId,
            body: body,
            timestamp: now,
          )));
          if (_scroll.hasClients) {
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (_scroll.hasClients) {
                _scroll.jumpTo(_scroll.position.maxScrollExtent + 80);
              }
            });
          }
        }
      } catch (_) {}
    });
    // No automatic contact handshake; require manual information exchange.
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }

  Future<void> _send() async {
    final text = _input.text.trim();
    if (text.isEmpty) return;
    setState(() => _sending = true);
    try {
      // If the user typed plaintext, wrap into a v1 envelope (dev-only transport wrapper)
      String body = text;
      if (!text.startsWith('v1:')) {
        final b64 = base64.encode(utf8.encode(text));
        const eph = 'UGxhaW5FcGg='; // "PlainEph"
        const nonce = 'Tm9uY2U='; // "Nonce"
        body = 'v1:$eph:$nonce:$b64';
      }
      try {
        await sendDirectMessageOverStreamToIdentity(
          userId: widget.selfUserId,
          toIdentity: widget.recipientId,
          body: body,
        );
      } catch (e) {
        final msg = e.toString();
        final host = widget.session.host.trim();
        final port = widget.session.port;
        if (msg.contains('No active stream session for user') && host.isNotEmpty && port > 0) {
          // Attempt to open a stream session on-demand, then retry once.
          final stream = openMessageStreamTls(
            host: host,
            port: port,
            caPem: widget.session.caPem,
            passphrase: widget.session.passphrase,
            password: widget.session.password,
          );
          _sub?.cancel();
          _sub = stream.listen((data) async {
            try {
              final map = jsonDecode(data) as Map;
              if (map['type'] == 'auth_ok') return;
              final fromId = (map['from_identity'] ?? '').toString();
              final bodyRaw = map['body'] as String? ?? '';
              if (fromId == widget.recipientId) {
                final body = _decodeEnvelope(bodyRaw);
                final now = DateTime.now().toIso8601String();
                final peer = idToNumeric(widget.recipientId);
                await appendLocalMessage(
                  fromUserId: peer,
                  toUserId: widget.selfUserId,
                  body: body,
                  timestamp: now,
                );
                setState(() => _messages.add(HistoryMessage(
                  id: 0,
                  fromUserId: peer,
                  toUserId: widget.selfUserId,
                  body: body,
                  timestamp: now,
                )));
              }
            } catch (_) {}
          });
          await Future.delayed(const Duration(milliseconds: 150));
          await sendDirectMessageOverStreamToIdentity(
            userId: widget.selfUserId,
            toIdentity: widget.recipientId,
            body: body,
          );
        } else {
          // Show a friendly error, but do not crash UI
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Send failed: ' + msg)),
          );
          return;
        }
      }
      final now = DateTime.now().toIso8601String();
      final peer = idToNumeric(widget.recipientId);
      await appendLocalMessage(
        fromUserId: widget.selfUserId,
        toUserId: peer,
        body: text,
        timestamp: now,
      );
      final msg = HistoryMessage(
        id: 0,
        fromUserId: widget.selfUserId,
        toUserId: peer,
        body: text,
        timestamp: now,
      );
      setState(() {
        _messages.add(msg);
        _input.clear();
      });
      if (_scroll.hasClients) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_scroll.hasClients) {
            _scroll.jumpTo(_scroll.position.maxScrollExtent + 80);
          }
        });
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final ridShort = widget.recipientId.length > 10 ? widget.recipientId.substring(0, 10) + '…' : widget.recipientId;
    return Scaffold(
      appBar: AppBar(title: Text('Chat: $ridShort')),
      body: Column(
        children: [
          Expanded(
            child: ListView.builder(
              controller: _scroll,
              padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
              itemCount: _messages.length,
              itemBuilder: (context, index) {
                final m = _messages[index];
                final fromSelf = m.fromUserId == widget.selfUserId;
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
                          style: TextStyle(color: fromSelf ? Colors.white : Colors.black),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          _formatTime(m.timestamp),
                          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: fromSelf ? Colors.white70 : const Color(0xCC000000)),
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
                        ? const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2))
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
      // If the user typed plaintext, wrap it into a v1 envelope so the server
      // accepts it under E2EE enforcement. NOTE: This is a transport wrapper
      // only and NOT real encryption. See docs/E2EE.md to implement real crypto.
      String body = text;
      if (!text.startsWith('v1:')) {
        final b64 = base64.encode(utf8.encode(text));
        // static placeholders for ephemeral pub and nonce (dev only)
        const eph = 'UGxhaW5FcGg='; // "PlainEph"
        const nonce = 'Tm9uY2U=';    // "Nonce"
        body = 'v1:$eph:$nonce:$b64';
      }

      await sendDirectMessageOverStream(
        userId: widget.selfUserId,
        toUserId: widget.peerUserId,
        body: body,
      );
      final now = DateTime.now().toIso8601String();
      // Persist to local cache (sender side) as plaintext
      await appendLocalMessage(
        fromUserId: widget.selfUserId,
        toUserId: widget.peerUserId,
        body: text,
        timestamp: now,
      );
      setState(() {
        _messages.add(HistoryMessage(
          id: 0,
          fromUserId: widget.selfUserId,
          toUserId: widget.peerUserId,
          body: text,
          timestamp: now,
        ));
        _input.clear();
      });
      await Future.delayed(const Duration(milliseconds: 50));
      if (_scroll.hasClients) {
        _scroll.jumpTo(_scroll.position.maxScrollExtent + 80);
      }
    } catch (e) {
      // Show a friendly error (e.g., when E2EE is enforced and body is not an envelope)
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Send failed: $e')),
        );
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

extension _OfflineNav on _HomePageState {
  Future<void> _unlockAndShowHistory() async {
    setState(() => _status = 'Unlocking local...');
    try {
      final pwd = _password.text;
      // Use offline branch: host empty, port 0
      final bundle = await loginAndLoadLocalHistoryTls(
        host: '',
        port: 0,
        caPem: '',
        passphrase: '',
        password: pwd,
        limit: BigInt.from(500),
      );
      if (!mounted) return;
      // No stream in offline mode
      final session = SessionConfig(host: '', port: 0, caPem: '', passphrase: '', password: pwd);
      Navigator.of(context).push(
        MaterialPageRoute(
          builder: (_) => ChatListPage(bundle: bundle, session: session, incoming: const Stream<String>.empty()),
        ),
      );
      setState(() => _status = 'Unlocked');
    } catch (e) {
      setState(() => _status = 'Unlock failed: $e');
    }
  }

  Future<void> _registerLocal() async {
    setState(() => _status = 'Registering local...');
    try {
      final pwd = _password.text;
      final bundle = await registerAndLoadLocalHistoryTls(
        host: '',
        port: 0,
        caPem: '',
        passphrase: '',
        password: pwd,
        limit: BigInt.from(500),
      );
      
      // TEMPORARY: Print the generated account ID
      try {
        final accountId = await getAccountId();
        print('TEMPORARY: $accountId');
      } catch (e) {
        print('TEMPORARY: Failed to get account ID: $e');
      }
      
      if (!mounted) return;
      final session = SessionConfig(host: '', port: 0, caPem: '', passphrase: '', password: pwd);
      Navigator.of(context).push(
        MaterialPageRoute(
          builder: (_) => ChatListPage(bundle: bundle, session: session, incoming: const Stream<String>.empty()),
        ),
      );
      setState(() => _status = 'Registered');
    } catch (e) {
      setState(() => _status = 'Register failed: $e');
    }
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

String _decodeEnvelope(String body) {
  if (body.startsWith('v1:')) {
    final parts = body.split(':');
    if (parts.length == 4) {
      try {
        return utf8.decode(base64.decode(parts[3]));
      } catch (_) {
        return body;
      }
    }
  }
  return body;
}
