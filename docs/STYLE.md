Rura Color Palette and Theming

Overview
- This document defines the brand color palette and how it is applied across the app in both light and dark modes.
- The Flutter client is themed using Material 3 with explicit light and dark `ColorScheme`s.

Palette
- Primary: #F06543 (kPrimary)
- Secondary: #33CCC7 (kSecondary)
- Tertiary: #F09D51 (kTertiary)
- Background (light): #E0DFD5 (kBackground)
- Dark Base: #313638 (kDark)
- Surfaces:
  - Light surface: #FFFFFF
  - Dark surface: #202325
- Error:
  - Light: #B00020
  - Dark: #CF6679

Roles and Usage
- Primary (#F06543)
  - Primary actions (ElevatedButton), AppBar background, sent chat bubbles.
  - On primary text: white.
- Secondary (#33CCC7)
  - Accents, FloatingActionButton, received chat bubbles, outlined button accents.
  - On secondary text: black.
- Tertiary (#F09D51)
  - Subtle accents and highlights (optional).
- Background / Surface
  - Light mode: background #E0DFD5, surface #FFFFFF.
  - Dark mode: background #313638, surface #202325.
- Text colors
  - Light mode: default text #313638.
  - Dark mode: default text #E0DFD5.

Light Mode Mapping
- ColorScheme
  - primary: #F06543, onPrimary: #FFFFFF
  - secondary: #33CCC7, onSecondary: #000000
  - tertiary: #F09D51, onTertiary: #000000
  - background: #E0DFD5, onBackground: #313638
  - surface: #FFFFFF, onSurface: #313638
  - error: #B00020, onError: #FFFFFF
- Components
  - AppBar: primary bg with white text
  - ElevatedButton: primary bg, white text
  - OutlinedButton: primary border, primary text
  - Inputs: white fill, primary focused border
  - FAB: secondary bg, black icon/text
  - Dividers: #313638 at 12% opacity
  - Chat bubbles: sent = primary with white text; received = secondary with black text

Dark Mode Mapping
- ColorScheme
  - primary: #F06543, onPrimary: #FFFFFF
  - secondary: #33CCC7, onSecondary: #000000
  - tertiary: #F09D51, onTertiary: #000000
  - background: #313638, onBackground: #E0DFD5
  - surface: #202325, onSurface: #E0DFD5
  - error: #CF6679, onError: #000000
- Components
  - AppBar: primary bg with white text
  - ElevatedButton: primary bg, white text
  - OutlinedButton: secondary border, secondary text
  - Inputs: #2B2F31 fill, primary focused border
  - FAB: secondary bg, black icon/text
  - Dividers: #E0DFD5 at 12% opacity
  - Chat bubbles: sent = primary with white text; received = secondary with black text

Accessibility
- Maintain WCAG contrast ratios:
  - Body text: 4.5:1
  - Large text (≥18 pt or ≥14 pt bold): 3:1
- Always use white text on primary backgrounds, black text on secondary backgrounds for consistent contrast.
- Avoid placing low-opacity text on patterned or image backgrounds.

Flutter Implementation
- Color constants (used in code):
  - kPrimary = Color(0xFFF06543)
  - kSecondary = Color(0xFF33CCC7)
  - kTertiary = Color(0xFFF09D51)
  - kBackground = Color(0xFFE0DFD5)
  - kDark = Color(0xFF313638)
- Theming is wired in `crates/client/flutter_app/lib/main.dart` via `theme` and `darkTheme` with `ThemeMode.system`.
- Example (simplified):

```dart
const kPrimary = Color(0xFFF06543);
const kSecondary = Color(0xFF33CCC7);
const kTertiary = Color(0xFFF09D51);
const kBackground = Color(0xFFE0DFD5);
const kDark = Color(0xFF313638);

final lightTheme = ThemeData(colorScheme: const ColorScheme(
  brightness: Brightness.light,
  primary: kPrimary, onPrimary: Colors.white,
  secondary: kSecondary, onSecondary: Colors.black,
  tertiary: kTertiary, onTertiary: Colors.black,
  background: kBackground, onBackground: kDark,
  surface: Colors.white, onSurface: kDark,
  error: Color(0xFFB00020), onError: Colors.white,
));

final darkTheme = ThemeData(colorScheme: const ColorScheme(
  brightness: Brightness.dark,
  primary: kPrimary, onPrimary: Colors.white,
  secondary: kSecondary, onSecondary: Colors.black,
  tertiary: kTertiary, onTertiary: Colors.black,
  background: kDark, onBackground: kBackground,
  surface: Color(0xFF202325), onSurface: kBackground,
  error: Color(0xFFCF6679), onError: Colors.black,
));

MaterialApp(
  theme: lightTheme,
  darkTheme: darkTheme,
  themeMode: ThemeMode.system,
  home: const HomePage(),
);
```

Notes
- The server crate is headless; theming only affects the Flutter client.
- Tertiary is reserved for subtle highlights; avoid overuse.
- For custom components, derive colors from the current `Theme.of(context).colorScheme` to respect both modes.

