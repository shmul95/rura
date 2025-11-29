import 'dart:async';
import 'dart:io';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:video_player/video_player.dart';
import 'dart:typed_data';
import 'frb/api.dart';
import 'package:flutter/services.dart' show rootBundle;
import 'frb/frb_generated.dart';

// App color palette
const kPrimary = Color(0xFFF06543); // f06543
const kSecondary = Color(0xFF33CCC7); // 33ccc7
const kTertiary = Color(0xFFF09D51); // f09d51
const kBackground = Color(0xFFE0DFD5); // e0dfd5
const kDark = Color(0xFF313638); // 313638

// Compile-time flag passed via `flutter run --dart-define=REQUIRE_E2EE=true`
const bool kRequireE2EE =
    bool.fromEnvironment('REQUIRE_E2EE', defaultValue: true);

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
      dividerTheme:
          DividerThemeData(color: kDark.withOpacity(0.12), thickness: 1),
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
      dividerTheme:
          DividerThemeData(color: kBackground.withOpacity(0.12), thickness: 1),
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
  final _password = TextEditingController(text: 'secret');
  String _status = 'Ready';
  bool _hasLocal = false;

  @override
  void initState() {
    super.initState();
    _detectLocal();
  }

  Future<void> _detectLocal() async {
    try {
      // Prefer env override if present, else default to ../data (same as Rust side)
      final envDir = Platform.environment['RURA_CLIENT_DATA_DIR'];
      final dir = envDir != null && envDir.trim().isNotEmpty
          ? Directory(envDir)
          : Directory('../data');
      final exists = await dir.exists();
      if (mounted) setState(() => _hasLocal = exists);
    } catch (_) {
      if (mounted) setState(() => _hasLocal = false);
    }
  }

  Future<void> _authAndShowHistory({required bool register}) async {
    setState(() => _status = register ? 'Registering...' : 'Logging in...');
    try {
      final host = _host.text.trim();
      final port = int.tryParse(_port.text.trim()) ?? 8443;
      // Load CA from bundled assets
      final caPem = await rootBundle.loadString('assets/ca.crt');
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
          builder: (_) =>
              ChatListPage(bundle: bundle, session: session, incoming: stream),
        ),
      );
      // Re-detect local storage for next time
      _detectLocal();
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
            TextField(
                controller: _host,
                decoration: const InputDecoration(
                    labelText: 'Server host (e.g., 127.0.0.1)')),
            const SizedBox(height: 8),
            TextField(
                controller: _port,
                decoration: const InputDecoration(
                    labelText: 'Server port (e.g., 8443)'),
                keyboardType: TextInputType.number),
            const SizedBox(height: 8),
            const SizedBox(height: 12),
            // Password to unlock local encrypted DB (used in both login and register flows)
            TextField(
                controller: _password,
                decoration: const InputDecoration(labelText: 'Password'),
                obscureText: true),
            const SizedBox(height: 16),
            // Single action button: Login if local data exists; otherwise Register
            SizedBox(
              width: double.infinity,
              child: ElevatedButton.icon(
                onPressed: () => _authAndShowHistory(register: !_hasLocal),
                icon: Icon(_hasLocal ? Icons.login : Icons.person_add_alt_1),
                label: Text(_hasLocal ? 'Login (Server)' : 'Register (Server)'),
              ),
            ),
            const SizedBox(height: 8),
            Text(
              _hasLocal
                  ? 'Existing local data found. Login will reuse it.'
                  : 'No local data found. Register will create it.',
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const SizedBox(height: 16),
            Text(_status, style: Theme.of(context).textTheme.bodyMedium),
          ],
        ),
      ),
    );
  }
}

extension on Stream<String> {
  Stream<String> asEmptyBroadcast() =>
      const Stream<String>.empty().asBroadcastStream();
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
  const ChatListPage(
      {super.key, required this.bundle, required this.session, this.incoming});

  @override
  Widget build(BuildContext context) =>
      _ChatListScaffold(bundle: bundle, session: session, incoming: incoming);

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
                decoration:
                    const InputDecoration(labelText: 'Recipient ID (base64)'),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: pkCtrl,
                keyboardType: TextInputType.text,
                decoration: const InputDecoration(
                    labelText: 'Recipient Public Key (base64)'),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: nickCtrl,
                keyboardType: TextInputType.text,
                decoration: const InputDecoration(
                    labelText: 'Surname (who is this person?)'),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          ElevatedButton(
            onPressed: () {
              final rid = idCtrl.text.trim();
              final pk = pkCtrl.text.trim();
              final nk = nickCtrl.text.trim();
              if (rid.isNotEmpty && pk.isNotEmpty) {
                Navigator.pop(ctx, {'rid': rid, 'pk': pk, 'nk': nk});
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
  const _ChatListScaffold(
      {required this.bundle, required this.session, this.incoming});
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
    // Load any saved nicknames/identity mappings from local storage
    _loadNicknames();
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
    final stream = widget.incoming ??
        openMessageStreamTls(
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
        if (map['type'] == 'call_invite') {
          final raw = map['data'];
          final payload = raw is String ? jsonDecode(raw) as Map : raw as Map;
          final toUser = payload['to_user_id'] as int?;
          if (toUser == _selfId) {
            final fromUser = payload['from_user_id'] as int? ?? 0;
            final media = payload['media'] as Map?;
            final wantsVideo = (media?['video_enabled'] ?? false) as bool;
            final callId = payload['call_id']?.toString() ?? '';
            if (callId.isNotEmpty && mounted) {
              final peerName = _nicknames[fromUser] ??
                  _identityByPeer[fromUser] ??
                  fromUser.toString();
              // Simple incoming call sheet
              // ignore: use_build_context_synchronously
              showModalBottomSheet<void>(
                context: context,
                builder: (ctx) => SafeArea(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Incoming ${wantsVideo ? 'video' : 'audio'} call',
                          style: Theme.of(ctx).textTheme.titleMedium,
                        ),
                        const SizedBox(height: 8),
                        Text(peerName),
                        const SizedBox(height: 16),
                        Row(
                          mainAxisAlignment: MainAxisAlignment.end,
                          children: [
                            TextButton(
                              onPressed: () async {
                                Navigator.of(ctx).pop();
                                try {
                                  await rejectCall(
                                      userId: _selfId,
                                      callId: callId,
                                      busy: false);
                                } catch (_) {}
                              },
                              child: const Text('Decline'),
                            ),
                            const SizedBox(width: 8),
                            ElevatedButton(
                              onPressed: () async {
                                Navigator.of(ctx).pop();
                                try {
                                  await acceptCall(
                                    userId: _selfId,
                                    callId: callId,
                                    enableVideo: wantsVideo,
                                  );
                                } catch (e) {
                                  if (mounted) {
                                    ScaffoldMessenger.of(context).showSnackBar(
                                      SnackBar(
                                          content: Text('Accept failed: $e')),
                                    );
                                  }
                                }
                              },
                              child: const Text('Accept'),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                ),
              );
            }
          }
          return;
        }
        if (map['type'] == 'media_complete') {
          // Save media message into conversation and show inline in UI using a body marker
          final isIdentity = map.containsKey('from_identity');
          final from = isIdentity
              ? idToNumeric((map['from_identity'] ?? '').toString())
              : (map['from_user_id'] as int);
          if (isIdentity) {
            final rid = (map['from_identity'] ?? '').toString();
            if (rid.isNotEmpty) {
              setState(() => _identityByPeer[from] = rid);
            }
          }
          final now = DateTime.now().toIso8601String();
          final filePath = (map['file_path'] ?? '').toString();
          String body;
          final mime = (map['mime'] ?? '').toString();
          if (filePath.isNotEmpty && File(filePath).existsSync()) {
            body = (mime.startsWith('image/') ? 'IMG:' : 'FILE:') + filePath;
          } else {
            // Fallback: reconstruct from data_b64 when file_path missing
            final b64 = (map['data_b64'] ?? '').toString();
            if (b64.isNotEmpty) {
              try {
                final bytes = base64.decode(b64);
                final name = (map['name'] ?? 'image').toString();
                final saved =
                    await _saveBytesToData(bytes, mime, suggestedName: name);
                body = (mime.startsWith('image/') ? 'IMG:' : 'FILE:') + saved;
              } catch (_) {
                body = mime.startsWith('image/')
                    ? '[image received]'
                    : '[file received]';
              }
            } else {
              body = mime.startsWith('image/')
                  ? '[image received]'
                  : '[file received]';
            }
          }
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
              timestamp: now);
          _incoming.add(msg);
          setState(() {
            _groups.putIfAbsent(from, () => []);
            _groups[from]!.add(msg);
          });
          return;
        }
        // Support both numeric and identity-based events
        final bool isIdentity = map.containsKey('from_identity');
        final int from = isIdentity
            ? idToNumeric((map['from_identity'] ?? '').toString())
            : (map['from_user_id'] as int);
        if (isIdentity) {
          final rid = (map['from_identity'] ?? '').toString();
          if (rid.isNotEmpty) {
            setState(() => _identityByPeer[from] = rid);
          }
        }
        final bodyRaw = map['body'] as String? ?? '';
        final body = await _decryptEnvelope(bodyRaw);
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

  // ----- Nicknames persistence (simple local JSON next to encrypted DB) -----
  File _nicknamesFile() {
    try {
      final envDir = Platform.environment['RURA_CLIENT_DATA_DIR'];
      final dir = envDir != null && envDir.trim().isNotEmpty
          ? Directory(envDir)
          : Directory('../data');
      return File('${dir.path}/nicknames.json');
    } catch (_) {
      return File('../data/nicknames.json');
    }
  }

  Future<void> _loadNicknames() async {
    try {
      final f = _nicknamesFile();
      if (!await f.exists()) return;
      final raw = await f.readAsString();
      final map = jsonDecode(raw);
      if (map is Map) {
        final nicks = map['nicknames'];
        final idmap = map['identities'];
        final loadedNicks = <int, String>{};
        final loadedIds = <int, String>{};
        if (nicks is Map) {
          for (final e in nicks.entries) {
            final k = int.tryParse(e.key.toString());
            final v = e.value?.toString();
            if (k != null && v != null && v.isNotEmpty) {
              loadedNicks[k] = v;
            }
          }
        }
        if (idmap is Map) {
          for (final e in idmap.entries) {
            final k = int.tryParse(e.key.toString());
            final v = e.value?.toString();
            if (k != null && v != null && v.isNotEmpty) {
              loadedIds[k] = v;
            }
          }
        }
        if (mounted) {
          setState(() {
            _nicknames = loadedNicks;
            _identityByPeer.addAll(loadedIds);
          });
        }
      }
    } catch (_) {
      // Ignore parse errors to avoid breaking UI
    }
  }

  Future<void> _saveNicknames() async {
    try {
      final f = _nicknamesFile();
      final dir = f.parent;
      if (!await dir.exists()) {
        await dir.create(recursive: true);
      }
      final data = <String, dynamic>{
        'nicknames': _nicknames.map((k, v) => MapEntry(k.toString(), v)),
        'identities': _identityByPeer.map((k, v) => MapEntry(k.toString(), v)),
      };
      await f.writeAsString(jsonEncode(data));
    } catch (_) {
      // Best-effort persistence; ignore failures silently
    }
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
            title: Text(_nicknames[peerId] ??
                _identityByPeer[peerId] ??
                peerId.toString()),
            subtitle: Text(_previewText(last?.body ?? '')),
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
                      recipientName: _nicknames[peerId],
                      inbound: _incoming.stream,
                      incomingRaw: widget.incoming ??
                          openMessageStreamTls(
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
                      peerName: _nicknames[peerId],
                      inbound: _incoming.stream,
                    ),
                  ),
                );
                await _reloadFromLocal();
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
            try {
              await addContact(userId: rid, pubkey: pk);
            } catch (_) {}
            final peer = idToNumeric(rid);
            setState(() {
              if ((nk?.isEmpty ?? true) == false) {
                _nicknames[peer] = nk!;
              }
              _identityByPeer[peer] = rid;
              _groups.putIfAbsent(peer, () => <HistoryMessage>[]);
            });
            // Persist nickname and identity mapping for future sessions
            unawaited(_saveNicknames());
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                  content: Text('Contact added: ' +
                      (_nicknames[peer] ?? rid.substring(0, 10) + '…'))),
            );
          } else if (sel is int) {
            await Navigator.of(context).push(
              MaterialPageRoute(
                builder: (_) => ChatPage(
                  session: widget.session,
                  selfUserId: _selfId,
                  peerUserId: sel,
                  initial: const [],
                  peerName: _nicknames[sel],
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
  final Stream<HistoryMessage>? inbound;
  final String? recipientName;
  const ChatIdentityPage(
      {super.key,
      required this.session,
      required this.selfUserId,
      required this.recipientId,
      required this.recipientPubKey,
      required this.incomingRaw,
      this.inbound,
      this.recipientName});
  @override
  State<ChatIdentityPage> createState() => _ChatIdentityPageState();
}

class _ChatIdentityPageState extends State<ChatIdentityPage> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  bool _sending = false;
  final List<HistoryMessage> _messages = [];
  StreamSubscription<String>? _sub;
  StreamSubscription<HistoryMessage>? _inSub;
  CallState? _callState;
  bool _callBusy = false;
  String? _displayName;
  // Use top-level idToNumeric()

  @override
  void initState() {
    super.initState();
    _displayName =
        (widget.recipientName != null && widget.recipientName!.isNotEmpty)
            ? widget.recipientName
            : widget.recipientId;
    // Load existing conversation from local DB so the view is not empty
    _loadFromLocal();
    _loadCallState();
    // Prefer processed inbound HistoryMessage stream for live refresh
    _inSub = widget.inbound?.listen((m) {
      final peer = idToNumeric(widget.recipientId);
      if (m.fromUserId == peer) {
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
    if (widget.inbound == null) {
      // Fallback only when processed inbound is unavailable
      _sub = widget.incomingRaw.listen((data) async {
        try {
          final map = jsonDecode(data) as Map;
          if (map['type'] == 'auth_ok') return;
          final fromId = (map['from_identity'] ?? '').toString();
          final bodyRaw = map['body'] as String? ?? '';
          if (fromId == widget.recipientId) {
            final body = await _decryptEnvelope(bodyRaw);
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
    }
    // No automatic contact handshake; require manual information exchange.
  }

  Future<void> _loadFromLocal() async {
    try {
      final peer = idToNumeric(widget.recipientId);
      final list = await loadLocalHistory(limit: BigInt.from(1000));
      final conv = list
          .where((m) =>
              (m.fromUserId == peer && m.toUserId == widget.selfUserId) ||
              (m.fromUserId == widget.selfUserId && m.toUserId == peer))
          .toList();
      conv.sort((a, b) => a.timestamp.compareTo(b.timestamp));
      // Debug print to terminal
      // ignore: avoid_print
      print('[ChatIdentity] Loaded ${conv.length} messages with $peer');
      for (final m in conv) {
        // ignore: avoid_print
        print('[${m.timestamp}] ${m.fromUserId} -> ${m.toUserId}: ${m.body}');
      }
      if (!mounted) return;
      setState(() {
        _messages.clear();
        _messages.addAll(conv);
      });
      // Scroll to bottom
      if (_scroll.hasClients) {
        await Future<void>.delayed(const Duration(milliseconds: 16));
        if (_scroll.hasClients) {
          _scroll.jumpTo(_scroll.position.maxScrollExtent + 40);
        }
      }
    } catch (e) {
      // ignore: avoid_print
      print('[ChatIdentity] Failed to load local history: $e');
    }
  }

  Future<void> _loadCallState() async {
    try {
      final state = await getCurrentCallState();
      if (!mounted) return;
      final remoteNumeric = idToNumeric(widget.recipientId);
      if (state != null && state.remoteUserId == remoteNumeric) {
        setState(() => _callState = state);
      } else {
        setState(() => _callState = null);
      }
    } catch (_) {
      // Ignore call state load errors in UI
    }
  }

  @override
  void dispose() {
    _sub?.cancel();
    _inSub?.cancel();
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
        if (msg.contains('No active stream session for user') &&
            host.isNotEmpty &&
            port > 0) {
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
                final body = await _decryptEnvelope(bodyRaw);
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

  String _guessMime(String name) {
    final lower = name.toLowerCase();
    if (lower.endsWith('.jpg') || lower.endsWith('.jpeg')) return 'image/jpeg';
    if (lower.endsWith('.png')) return 'image/png';
    if (lower.endsWith('.gif')) return 'image/gif';
    if (lower.endsWith('.webp')) return 'image/webp';
    if (lower.endsWith('.bmp')) return 'image/bmp';
    if (lower.endsWith('.heic') || lower.endsWith('.heif')) return 'image/heic';
    if (lower.endsWith('.mp4')) return 'video/mp4';
    if (lower.endsWith('.mov')) return 'video/quicktime';
    if (lower.endsWith('.webm')) return 'video/webm';
    if (lower.endsWith('.mkv')) return 'video/x-matroska';
    if (lower.endsWith('.mp3')) return 'audio/mpeg';
    if (lower.endsWith('.wav')) return 'audio/wav';
    if (lower.endsWith('.ogg')) return 'audio/ogg';
    if (lower.endsWith('.pdf')) return 'application/pdf';
    if (lower.endsWith('.txt')) return 'text/plain';
    return 'application/octet-stream';
  }

  Future<void> _pickAndSendFile() async {
    // In-app fallback file chooser (no external plugin). Starts at user's home.
    if (_sending) return;
    setState(() => _sending = true);
    try {
      final picked = await _browseForFile(context);
      if (picked == null) return;
      final bytes = picked.$1;
      final name = picked.$2;
      final mime = _guessMime(name);
      // Call FRB to send chunked media over WebRTC
      await sendMediaToIdentity(
        userId: widget.selfUserId,
        toIdentity: widget.recipientId,
        mime: mime,
        name: name,
        bytes: bytes,
        chunkSize: BigInt.from(12 * 1024),
      );
      // Locally reflect the sent file in our conversation immediately.
      final savedPath =
          await _saveBytesToData(bytes, mime, suggestedName: name);
      if (savedPath.isNotEmpty) {
        final now = DateTime.now().toIso8601String();
        final peer = idToNumeric(widget.recipientId);
        final isImg = mime.startsWith('image/');
        final msg = HistoryMessage(
          id: 0,
          fromUserId: widget.selfUserId,
          toUserId: peer,
          body: (isImg ? 'IMG:' : 'FILE:') + savedPath,
          timestamp: now,
        );
        await appendLocalMessage(
          fromUserId: msg.fromUserId,
          toUserId: msg.toUserId,
          body: msg.body,
          timestamp: msg.timestamp,
        );
        if (mounted) {
          setState(() => _messages.add(msg));
          if (_scroll.hasClients) {
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (_scroll.hasClients) {
                _scroll.jumpTo(_scroll.position.maxScrollExtent + 120);
              }
            });
          }
        }
      }
    } catch (e) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('File send failed: $e')),
      );
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<(Uint8List, String)?> _browseForFile(BuildContext context) async {
    Directory start = _defaultImagesDir() ?? Directory.current;

    Directory current = start;
    String? selectedPath;
    return showDialog<(Uint8List, String)?>(
      context: context,
      builder: (ctx) {
        return StatefulBuilder(builder: (ctx, setState) {
          final entries =
              current.listSync().whereType<FileSystemEntity>().where((e) {
            final name = _basename(e.path);
            // Hide dot entries in the file picker (files and directories)
            return name.isNotEmpty && !name.startsWith('.');
          }).toList()
                ..sort((a, b) => a is Directory && b is! Directory
                    ? -1
                    : a is! Directory && b is Directory
                        ? 1
                        : a.path.compareTo(b.path));
          return AlertDialog(
            title: Text('Choose file — ${_basename(current.path)}'),
            content: SizedBox(
              width: 600,
              height: 400,
              child: ListView.builder(
                itemCount: entries.length,
                itemBuilder: (_, i) {
                  final e = entries[i];
                  final name = _basename(e.path);
                  final isDir = e is Directory;
                  return ListTile(
                    leading: Icon(isDir ? Icons.folder : Icons.attach_file),
                    title: Text(name.isEmpty ? e.path : name),
                    onTap: () async {
                      if (isDir) {
                        try {
                          current = Directory(e.path);
                          setState(() {});
                        } catch (_) {}
                      } else {
                        selectedPath = e.path;
                        try {
                          final bytes = await File(e.path).readAsBytes();
                          if (ctx.mounted) Navigator.of(ctx).pop((bytes, name));
                        } catch (err) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                                content: Text('Failed to read file: $err')),
                          );
                        }
                      }
                    },
                  );
                },
              ),
            ),
            actions: [
              TextButton(
                onPressed: () async {
                  // Go up one directory if possible
                  final parent = current.parent;
                  if (parent.path != current.path) {
                    current = parent;
                    setState(() {});
                  }
                },
                child: const Text('Up'),
              ),
              TextButton(
                onPressed: () => Navigator.of(ctx).pop(null),
                child: const Text('Cancel'),
              ),
            ],
          );
        });
      },
    );
  }

  Future<void> _startCall({required bool enableVideo}) async {
    if (_callBusy) return;
    setState(() => _callBusy = true);
    try {
      final remoteNumeric = idToNumeric(widget.recipientId);
      final state = await startCall(
        userId: widget.selfUserId,
        remoteUserId: remoteNumeric,
        enableVideo: enableVideo,
      );
      if (!mounted) return;
      setState(() => _callState = state);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
            content: Text(
                enableVideo ? 'Video call started' : 'Audio call started')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Call failed: $e')),
      );
    } finally {
      if (mounted) setState(() => _callBusy = false);
    }
  }

  Future<void> _endCurrentCall() async {
    final current = _callState;
    if (current == null || _callBusy) return;
    setState(() => _callBusy = true);
    try {
      await endCall(userId: widget.selfUserId, callId: current.callId);
      if (!mounted) return;
      setState(() => _callState = null);
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Call ended')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('End call failed: $e')),
      );
    } finally {
      if (mounted) setState(() => _callBusy = false);
    }
  }

  // Start in user's home directory for file picker.
  Directory? _defaultImagesDir() {
    try {
      final home =
          Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
      if (home != null && home.isNotEmpty) {
        return Directory(home);
      }
    } catch (_) {}
    return null;
  }

  String _basename(String path) {
    if (path.isEmpty) return path;
    var p = path;
    while (p.endsWith('/') || p.endsWith('\\')) {
      if (p.length <= 1) break;
      p = p.substring(0, p.length - 1);
    }
    final idx = p.lastIndexOf(RegExp(r'[\\/]'));
    return idx >= 0 ? p.substring(idx + 1) : p;
  }

  @override
  Widget build(BuildContext context) {
    final title = _displayName ?? widget.recipientId;
    final remoteNumeric = idToNumeric(widget.recipientId);
    return Scaffold(
      appBar: AppBar(
        title: GestureDetector(
          onTap: _promptRename,
          child: Text(title),
        ),
        actions: [
          if (_callState == null) ...[
            IconButton(
              tooltip: 'Audio call',
              onPressed:
                  _callBusy ? null : () => _startCall(enableVideo: false),
              icon: const Icon(Icons.call),
            ),
            IconButton(
              tooltip: 'Video call',
              onPressed: _callBusy ? null : () => _startCall(enableVideo: true),
              icon: const Icon(Icons.videocam),
            ),
          ] else
            IconButton(
              tooltip: 'Hang up',
              onPressed: _callBusy ? null : _endCurrentCall,
              icon: const Icon(Icons.call_end),
            ),
        ],
      ),
      body: Column(
        children: [
          if (_callState != null)
            Container(
              width: double.infinity,
              color: Colors.black.withOpacity(0.05),
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              child: Row(
                children: [
                  Icon(
                    _callState!.videoEnabled ? Icons.videocam : Icons.call,
                    color: Theme.of(context).colorScheme.primary,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      _callState!.status == CallStatus.connected
                          ? 'In call'
                          : 'Calling…',
                      style: Theme.of(context).textTheme.bodyMedium,
                    ),
                  ),
                  TextButton(
                    onPressed: _callBusy ? null : _endCurrentCall,
                    child: const Text('End'),
                  ),
                ],
              ),
            ),
          Expanded(
            child: ListView.builder(
              controller: _scroll,
              padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
              itemCount: _messages.length,
              itemBuilder: (context, index) {
                final m = _messages[index];
                final fromSelf = m.fromUserId == widget.selfUserId;
                return Align(
                  alignment:
                      fromSelf ? Alignment.centerRight : Alignment.centerLeft,
                  child: Container(
                    margin: const EdgeInsets.symmetric(vertical: 4),
                    padding:
                        const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
                    constraints: BoxConstraints(
                        maxWidth: MediaQuery.of(context).size.width * 0.7),
                    decoration: BoxDecoration(
                      color: fromSelf ? kPrimary : kSecondary,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Column(
                      crossAxisAlignment: fromSelf
                          ? CrossAxisAlignment.end
                          : CrossAxisAlignment.start,
                      children: [
                        _renderMessageBody(m, fromSelf),
                        const SizedBox(height: 4),
                        Text(
                          _formatTime(m.timestamp),
                          style: Theme.of(context)
                              .textTheme
                              .bodySmall
                              ?.copyWith(
                                  color: fromSelf
                                      ? Colors.white70
                                      : const Color(0xCC000000)),
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
                  IconButton(
                    tooltip: 'Attach',
                    onPressed: _sending ? null : _pickAndSendFile,
                    icon: const Icon(Icons.attach_file),
                  ),
                  Expanded(
                    child: TextField(
                      controller: _input,
                      textInputAction: TextInputAction.send,
                      onSubmitted: (_) {
                        if (!_sending) {
                          // ignore: discarded_futures
                          _send();
                        }
                      },
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
                            child: CircularProgressIndicator(strokeWidth: 2))
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

  Future<void> _promptRename() async {
    final ctrl =
        TextEditingController(text: _displayName ?? widget.recipientId);
    final newName = await showDialog<String?>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Set nickname'),
        content: TextField(
          controller: ctrl,
          decoration: const InputDecoration(hintText: 'Enter a nickname'),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(ctx).pop(null),
              child: const Text('Cancel')),
          ElevatedButton(
              onPressed: () => Navigator.of(ctx).pop(ctrl.text.trim()),
              child: const Text('Save')),
        ],
      ),
    );
    if (newName == null) return;
    try {
      await setContactNickname(
          userId: widget.recipientId,
          nickname: newName.isEmpty ? null : newName);
      await _updateNicknameFile(idToNumeric(widget.recipientId), newName);
      if (mounted)
        setState(() =>
            _displayName = newName.isEmpty ? widget.recipientId : newName);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(const SnackBar(content: Text('Nickname updated')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to update: $e')));
      }
    }
  }

  Future<void> _updateNicknameFile(int peerNumeric, String newName) async {
    try {
      // Mirror ChatListPage local JSON structure
      final envDir = Platform.environment['RURA_CLIENT_DATA_DIR'];
      final baseDir = envDir != null && envDir.trim().isNotEmpty
          ? Directory(envDir)
          : Directory('../data');
      if (!baseDir.existsSync()) {
        await baseDir.create(recursive: true);
      }
      final f = File('${baseDir.path}/nicknames.json');
      Map<String, dynamic> data = {
        'nicknames': <String, String>{},
        'identities': <String, String>{}
      };
      if (await f.exists()) {
        try {
          final raw = await f.readAsString();
          final parsed = jsonDecode(raw);
          if (parsed is Map)
            data = parsed.map((k, v) => MapEntry(k.toString(), v));
        } catch (_) {}
      }
      final nicks = (data['nicknames'] as Map?)
              ?.map((k, v) => MapEntry(k.toString(), v.toString())) ??
          <String, String>{};
      final ids = (data['identities'] as Map?)
              ?.map((k, v) => MapEntry(k.toString(), v.toString())) ??
          <String, String>{};
      nicks[peerNumeric.toString()] = newName;
      ids.putIfAbsent(peerNumeric.toString(), () => widget.recipientId);
      data['nicknames'] = nicks;
      data['identities'] = ids;
      await f.writeAsString(jsonEncode(data));
    } catch (_) {
      // best effort
    }
  }
}

class ChatPage extends StatefulWidget {
  final SessionConfig session;
  final int selfUserId;
  final int peerUserId;
  final List<HistoryMessage> initial;
  final Stream<HistoryMessage>? inbound;
  final String? peerName;
  const ChatPage(
      {super.key,
      required this.session,
      required this.selfUserId,
      required this.peerUserId,
      required this.initial,
      this.inbound,
      this.peerName});

  @override
  State<ChatPage> createState() => _ChatPageState();
}

class _ChatPageState extends State<ChatPage> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  bool _sending = false;
  late List<HistoryMessage> _messages;
  StreamSubscription<HistoryMessage>? _inSub;
  CallState? _callState;
  bool _callBusy = false;

  @override
  void initState() {
    super.initState();
    _messages = List.of(widget.initial);
    // Replace with full history for this peer from local DB
    _loadFromLocal();
    _loadCallState();
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

  Future<void> _loadFromLocal() async {
    try {
      final list = await loadLocalHistory(limit: BigInt.from(1000));
      final conv = list
          .where((m) =>
              (m.fromUserId == widget.peerUserId &&
                  m.toUserId == widget.selfUserId) ||
              (m.fromUserId == widget.selfUserId &&
                  m.toUserId == widget.peerUserId))
          .toList();
      conv.sort((a, b) => a.timestamp.compareTo(b.timestamp));
      // Debug print to terminal
      // ignore: avoid_print
      print('[Chat] Loaded ${conv.length} messages with ${widget.peerUserId}');
      for (final m in conv) {
        // ignore: avoid_print
        print('[${m.timestamp}] ${m.fromUserId} -> ${m.toUserId}: ${m.body}');
      }
      if (!mounted) return;
      setState(() {
        _messages = conv;
      });
      if (_scroll.hasClients) {
        await Future<void>.delayed(const Duration(milliseconds: 16));
        if (_scroll.hasClients) {
          _scroll.jumpTo(_scroll.position.maxScrollExtent + 40);
        }
      }
    } catch (e) {
      // ignore: avoid_print
      print('[Chat] Failed to load local history: $e');
    }
  }

  Future<void> _loadCallState() async {
    try {
      final state = await getCurrentCallState();
      if (!mounted) return;
      if (state != null && state.remoteUserId == widget.peerUserId) {
        setState(() => _callState = state);
      } else {
        setState(() => _callState = null);
      }
    } catch (_) {
      // Ignore call state load errors in UI
    }
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
        const nonce = 'Tm9uY2U='; // "Nonce"
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

  Future<void> _startCall({required bool enableVideo}) async {
    if (_callBusy) return;
    setState(() => _callBusy = true);
    try {
      final state = await startCall(
        userId: widget.selfUserId,
        remoteUserId: widget.peerUserId,
        enableVideo: enableVideo,
      );
      if (!mounted) return;
      setState(() => _callState = state);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
            content: Text(
                enableVideo ? 'Video call started' : 'Audio call started')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Call failed: $e')),
      );
    } finally {
      if (mounted) setState(() => _callBusy = false);
    }
  }

  Future<void> _endCurrentCall() async {
    final current = _callState;
    if (current == null || _callBusy) return;
    setState(() => _callBusy = true);
    try {
      await endCall(userId: widget.selfUserId, callId: current.callId);
      if (!mounted) return;
      setState(() => _callState = null);
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Call ended')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('End call failed: $e')),
      );
    } finally {
      if (mounted) setState(() => _callBusy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final self = widget.selfUserId;
    final msgs = _messages
        .where((m) =>
            m.fromUserId == widget.peerUserId ||
            m.toUserId == widget.peerUserId)
        .toList();
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.peerName?.isNotEmpty == true
            ? widget.peerName!
            : 'User ${widget.peerUserId}'),
        actions: [
          if (_callState == null) ...[
            IconButton(
              tooltip: 'Audio call',
              onPressed:
                  _callBusy ? null : () => _startCall(enableVideo: false),
              icon: const Icon(Icons.call),
            ),
            IconButton(
              tooltip: 'Video call',
              onPressed: _callBusy ? null : () => _startCall(enableVideo: true),
              icon: const Icon(Icons.videocam),
            ),
          ] else
            IconButton(
              tooltip: 'Hang up',
              onPressed: _callBusy ? null : _endCurrentCall,
              icon: const Icon(Icons.call_end),
            ),
        ],
      ),
      body: Column(
        children: [
          if (_callState != null)
            Container(
              width: double.infinity,
              color: Colors.black.withOpacity(0.05),
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              child: Row(
                children: [
                  Icon(
                    _callState!.videoEnabled ? Icons.videocam : Icons.call,
                    color: Theme.of(context).colorScheme.primary,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      _callState!.status == CallStatus.connected
                          ? 'In call'
                          : 'Calling…',
                      style: Theme.of(context).textTheme.bodyMedium,
                    ),
                  ),
                  TextButton(
                    onPressed: _callBusy ? null : _endCurrentCall,
                    child: const Text('End'),
                  ),
                ],
              ),
            ),
          Expanded(
            child: ListView.builder(
              controller: _scroll,
              padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
              itemCount: msgs.length,
              itemBuilder: (context, index) {
                final m = msgs[index];
                final fromSelf = m.fromUserId == self;
                return Align(
                  alignment:
                      fromSelf ? Alignment.centerRight : Alignment.centerLeft,
                  child: Container(
                    margin: const EdgeInsets.symmetric(vertical: 4),
                    padding:
                        const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
                    constraints: BoxConstraints(
                        maxWidth: MediaQuery.of(context).size.width * 0.7),
                    decoration: BoxDecoration(
                      color: fromSelf ? kPrimary : kSecondary,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Column(
                      crossAxisAlignment: fromSelf
                          ? CrossAxisAlignment.end
                          : CrossAxisAlignment.start,
                      children: [
                        _renderMessageBody(m, fromSelf),
                        const SizedBox(height: 4),
                        Text(
                          _formatTime(m.timestamp),
                          style: Theme.of(context)
                              .textTheme
                              .bodySmall
                              ?.copyWith(
                                  color: fromSelf
                                      ? Colors.white70
                                      : const Color(0xCC000000)),
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
                      textInputAction: TextInputAction.send,
                      onSubmitted: (_) {
                        if (!_sending) {
                          // ignore: discarded_futures
                          _send();
                        }
                      },
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
      final session = SessionConfig(
          host: '', port: 0, caPem: '', passphrase: '', password: pwd);
      Navigator.of(context).push(
        MaterialPageRoute(
          builder: (_) => ChatListPage(
              bundle: bundle,
              session: session,
              incoming: const Stream<String>.empty()),
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
      final session = SessionConfig(
          host: '', port: 0, caPem: '', passphrase: '', password: pwd);
      Navigator.of(context).push(
        MaterialPageRoute(
          builder: (_) => ChatListPage(
              bundle: bundle,
              session: session,
              incoming: const Stream<String>.empty()),
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

Future<String> _decryptEnvelope(String body) async {
  if (body.startsWith('v1:')) {
    try {
      return await decryptMessageFromEnvelope(envelope: body);
    } catch (_) {
      final parts = body.split(':');
      if (parts.length == 4) {
        try {
          return utf8.decode(base64.decode(parts[3]));
        } catch (_) {
          return body;
        }
      }
    }
  }
  return body;
}

Widget _renderMessageBody(HistoryMessage m, bool fromSelf) {
  final body = m.body;
  if (body.startsWith('IMG:')) {
    final path = body.substring(4);
    final f = File(path);
    if (f.existsSync()) {
      return ClipRRect(
        borderRadius: BorderRadius.circular(8),
        child: Image.file(
          f,
          width: 240,
          fit: BoxFit.cover,
          errorBuilder: (_, __, ___) => Text('[image] ${path.split('/').last}',
              style: TextStyle(color: fromSelf ? Colors.white : Colors.black)),
        ),
      );
    }
    return Text('[image missing] ${path.split('/').last}',
        style: TextStyle(color: fromSelf ? Colors.white : Colors.black));
  }
  if (body.startsWith('FILE:')) {
    final path = body.substring(5);
    final lower = path.toLowerCase();
    final isVideo = lower.endsWith('.mp4') ||
        lower.endsWith('.mov') ||
        lower.endsWith('.webm') ||
        lower.endsWith('.mkv');
    if (isVideo && File(path).existsSync()) {
      return SizedBox(
        width: 240,
        height: 160,
        child: _InlineVideoPlayer(filePath: path),
      );
    }
    final name = path.split('/').isNotEmpty ? path.split('/').last : path;
    return InkWell(
      onTap: () => _openFilePath(path),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.attach_file,
              size: 18, color: fromSelf ? Colors.white : Colors.black),
          const SizedBox(width: 6),
          Flexible(
            child: Text(name,
                style: TextStyle(color: fromSelf ? Colors.white : Colors.black),
                overflow: TextOverflow.ellipsis),
          ),
        ],
      ),
    );
  }
  if (body.startsWith('FILE:')) {
    final path = body.substring(5);
    final name = path.split('/').isNotEmpty ? path.split('/').last : path;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(Icons.attach_file,
            size: 18, color: fromSelf ? Colors.white : Colors.black),
        const SizedBox(width: 6),
        Flexible(
          child: Text(name,
              style: TextStyle(color: fromSelf ? Colors.white : Colors.black),
              overflow: TextOverflow.ellipsis),
        ),
      ],
    );
  }
  return Text(body,
      style: TextStyle(color: fromSelf ? Colors.white : Colors.black));
}

Future<String> _saveBytesToData(Uint8List bytes, String mime,
    {String? suggestedName}) async {
  try {
    // Mirror Rust path logic: ../data/images|videos|files under app dir
    Directory dir;
    if (mime.startsWith('image/')) {
      dir = Directory('../data/images');
    } else if (mime.startsWith('video/')) {
      dir = Directory('../data/videos');
    } else {
      dir = Directory('../data/files');
    }
    if (!dir.existsSync()) {
      await dir.create(recursive: true);
    }
    var name = suggestedName ?? 'file';
    name = name.replaceAll(RegExp(r'[^A-Za-z0-9._-]'), '_');
    if (name.isEmpty) name = 'file';
    var path = '${dir.path}/$name';
    var file = File(path);
    if (await file.exists()) {
      int i = 1;
      while (await File('${dir.path}/${name}_$i').exists()) {
        i++;
        if (i > 1000) break;
      }
      path = '${dir.path}/${name}_$i';
      file = File(path);
    }
    await file.writeAsBytes(bytes);
    try {
      return file.resolveSymbolicLinksSync();
    } catch (_) {
      return file.path;
    }
  } catch (_) {
    return '';
  }
}

String _previewText(String body) {
  if (body.startsWith('IMG:')) return '[image]';
  if (body.startsWith('FILE:'))
    return '[file] ' + (body.split('/').isNotEmpty ? body.split('/').last : '');
  return body;
}

Future<void> _openFilePath(String path) async {
  try {
    if (Platform.isLinux) {
      await Process.run('xdg-open', [path]);
    } else if (Platform.isMacOS) {
      await Process.run('open', [path]);
    } else if (Platform.isWindows) {
      await Process.run('cmd', ['/c', 'start', '', path]);
    }
  } catch (_) {
    // ignore
  }
}

class _InlineVideoPlayer extends StatefulWidget {
  final String filePath;
  const _InlineVideoPlayer({required this.filePath});
  @override
  State<_InlineVideoPlayer> createState() => _InlineVideoPlayerState();
}

class _InlineVideoPlayerState extends State<_InlineVideoPlayer> {
  late VideoPlayerController _controller;
  bool _ready = false;
  bool _error = false;

  @override
  void initState() {
    super.initState();
    _controller = VideoPlayerController.file(File(widget.filePath))
      ..initialize().then((_) {
        if (mounted) setState(() => _ready = true);
      }).catchError((_) {
        if (mounted) setState(() => _error = true);
      });
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _toggle() {
    if (!_ready) return;
    setState(() {
      if (_controller.value.isPlaying) {
        _controller.pause();
      } else {
        _controller.play();
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    if (_error) return const Text('[video unsupported]');
    if (!_ready)
      return const Center(
          child: SizedBox(
              width: 20,
              height: 20,
              child: CircularProgressIndicator(strokeWidth: 2)));
    return Stack(
      fit: StackFit.expand,
      children: [
        FittedBox(
          fit: BoxFit.cover,
          child: SizedBox(
            width: _controller.value.size.width,
            height: _controller.value.size.height,
            child: VideoPlayer(_controller),
          ),
        ),
        Positioned.fill(
          child: Material(
            color: Colors.transparent,
            child: InkWell(
              onTap: _toggle,
              child: Center(
                child: Container(
                  decoration: BoxDecoration(
                      color: Colors.black45,
                      borderRadius: BorderRadius.circular(24)),
                  padding: const EdgeInsets.all(8),
                  child: Icon(
                    _controller.value.isPlaying
                        ? Icons.pause
                        : Icons.play_arrow,
                    color: Colors.white,
                    size: 28,
                  ),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
