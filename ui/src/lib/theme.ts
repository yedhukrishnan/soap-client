// Light / Dark / System appearance. The choice is made in the app menu and stored by the
// backend (src-tauri/src/appearance.rs); it is mirrored in localStorage so the first paint
// already has the right colours. The page is coloured through `data-theme` on <html>
// rather than only `prefers-color-scheme`, because not every Linux web view reports the
// desktop's dark preference.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Appearance = "system" | "light" | "dark";

const KEY = "soap-client.appearance";
const media = window.matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();

const valid = (v: unknown): v is Appearance => v === "system" || v === "light" || v === "dark";
const stored = localStorage.getItem(KEY);
let appearance: Appearance = valid(stored) ? stored : "system";

export const isDark = () => appearance === "dark" || (appearance === "system" && media.matches);

/** Call `f` whenever the effective theme may have changed. */
export function onThemeChange(f: () => void) {
  listeners.add(f);
}

function apply() {
  const root = document.documentElement;
  root.dataset.theme = isDark() ? "dark" : "light";
  for (const f of listeners) f();
}

function set(a: Appearance) {
  if (!valid(a)) return;
  appearance = a;
  localStorage.setItem(KEY, a);
  apply();
}

media.addEventListener("change", apply);
apply();
listen<Appearance>("appearance", (e) => set(e.payload));
invoke<Appearance>("appearance_get").then(set, () => {});
