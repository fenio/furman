// Detect platform once at startup
const isMac = navigator.platform?.startsWith('Mac') ?? false;

export const platform = {
  isMac,
  /** Modifier symbol: ⌘ on macOS, Ctrl+ on Linux */
  mod: isMac ? '\u2318' : 'Ctrl+',
  /** Shift symbol: ⇧ on macOS, Shift+ on Linux */
  shift: isMac ? '\u21E7' : 'Shift+',
  /** Alt/Option symbol: ⌥ on macOS, Alt+ on Linux */
  alt: isMac ? '\u2325' : 'Alt+',
  /** Backspace symbol: ⌫ on macOS, Del on Linux */
  backspace: isMac ? '\u232B' : 'Del',
};
