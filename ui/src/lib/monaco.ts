// Monaco setup: only the base editor worker is needed (XML uses a Monarch tokenizer).
import * as monaco from "monaco-editor";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };

const dark = window.matchMedia("(prefers-color-scheme: dark)");
export const currentTheme = () => (dark.matches ? "vs-dark" : "vs");
dark.addEventListener("change", () => monaco.editor.setTheme(currentTheme()));

/** One text model per request so each tab keeps its own undo history. */
const models = new Map<string, monaco.editor.ITextModel>();

export function modelFor(key: string, initial: string): monaco.editor.ITextModel {
  let m = models.get(key);
  if (!m || m.isDisposed()) {
    m = monaco.editor.createModel(initial, "xml");
    models.set(key, m);
  }
  return m;
}

export function disposeModel(key: string) {
  models.get(key)?.dispose();
  models.delete(key);
}

export { monaco };
