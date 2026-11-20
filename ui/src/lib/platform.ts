// Platform-specific wording: shortcuts and the name of the secret store.

export const isMac = /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);

/** Display a shortcut: shortcut("S", true) -> "⇧⌘S" on macOS, "Ctrl+Shift+S" elsewhere. */
export function shortcut(key: string, shift = false): string {
  if (isMac) return `${shift ? "⇧" : ""}⌘${key === "Enter" ? "↵" : key}`;
  return `Ctrl+${shift ? "Shift+" : ""}${key}`;
}

/** Where secret values are kept, as users know it. */
export const secretStore = isMac ? "Keychain" : "system keyring";

/** "1 request", "2 requests". */
export function plural(n: number, word: string, many = `${word}s`): string {
  return `${n} ${n === 1 ? word : many}`;
}
