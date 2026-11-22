<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { currentTheme, modelFor, monaco } from "../lib/monaco";
  import type { EffectiveVar } from "../lib/state.svelte";
  import type { Issue } from "../lib/types";

  interface Props {
    value: string;
    /** Editable editors keep a per-key model (undo history survives tab switches). */
    modelKey?: string;
    readOnly?: boolean;
    wordWrap?: boolean;
    onChange?: (v: string) => void;
    onSend?: () => void;
    /** When given, {{placeholders}} are highlighted (unknown ones underlined). */
    vars?: Map<string, EffectiveVar>;
    /** Validation findings shown as squiggles; applied only while the text is still `issuesFor`. */
    issues?: Issue[];
    issuesFor?: string;
  }
  let { value, modelKey, readOnly = false, wordWrap = false, onChange, onSend, vars, issues, issuesFor }: Props =
    $props();

  let el: HTMLDivElement;
  let editor: monaco.editor.IStandaloneCodeEditor | undefined;
  let ownModel: monaco.editor.ITextModel | undefined;
  let applying = false;
  let decorations: monaco.editor.IEditorDecorationsCollection | undefined;

  /** Escape Markdown so untrusted text renders literally in Monaco hovers. */
  const md = (s: string) => s.replace(/[\\`*_{}[\]()#+\-.!|<>~]/g, "\\$&");

  const PLACEHOLDER = /\{\{\s*([A-Za-z_][A-Za-z0-9_.-]*)\s*\}\}/g;

  function decorate() {
    const model = editor?.getModel();
    if (!editor || !model) return;
    decorations ??= editor.createDecorationsCollection();
    if (!vars) return decorations.clear();
    const text = model.getValue();
    const list: monaco.editor.IModelDeltaDecoration[] = [];
    for (const m of text.matchAll(PLACEHOLDER)) {
      const start = model.getPositionAt(m.index!);
      const end = model.getPositionAt(m.index! + m[0].length);
      const v = vars.get(m[1]);
      // Values come from project files (possibly someone else's): escape Markdown.
      const hover = !v
        ? `Undefined variable ${md(m[1])}: define it in Variables & Environments`
        : v.secret
          ? `${md(v.name)}: secret (${v.scope === "project" ? "project" : "environment"} variable)`
          : `${md(v.name)} = ${md(v.value)}`;
      list.push({
        range: new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column),
        options: { inlineClassName: v ? "var-ok" : "var-missing", hoverMessage: { value: hover } },
      });
    }
    decorations.set(list);
  }

  const MARKER_OWNER = "soap-validation";

  function mark() {
    const model = editor?.getModel();
    if (!model) return;
    // Results for older text would point at the wrong places; Monaco moves existing
    // markers along with edits, so keep them until fresh results arrive.
    if (issuesFor !== undefined && model.getValue() !== issuesFor) return;
    const markers: monaco.editor.IMarkerData[] = (issues ?? [])
      .filter((i) => i.start != null && i.end != null)
      .map((i) => {
        const s = model.getPositionAt(i.start!);
        const e = model.getPositionAt(Math.max(i.end!, i.start! + 1));
        return {
          severity: i.severity === "error" ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning,
          message: i.message,
          startLineNumber: s.lineNumber,
          startColumn: s.column,
          endLineNumber: e.lineNumber,
          endColumn: e.column,
        };
      });
    monaco.editor.setModelMarkers(model, MARKER_OWNER, markers);
  }

  /** Select a range (UTF-16 offsets) and scroll it into view. */
  export function reveal(start: number, end: number) {
    const model = editor?.getModel();
    if (!editor || !model) return;
    const s = model.getPositionAt(start);
    const e = model.getPositionAt(end);
    const range = new monaco.Range(s.lineNumber, s.column, e.lineNumber, e.column);
    editor.setSelection(range);
    editor.revealRangeInCenterIfOutsideViewport(range);
    editor.focus();
  }

  onMount(() => {
    editor = monaco.editor.create(el, {
      language: "xml",
      theme: currentTheme(),
      readOnly,
      automaticLayout: true,
      minimap: { enabled: false },
      fontSize: 13,
      scrollBeyondLastLine: false,
      wordWrap: wordWrap ? "on" : "off",
      tabSize: 2,
      folding: true,
      renderLineHighlight: readOnly ? "none" : "line",
      fixedOverflowWidgets: true,
    });
    if (!modelKey) {
      ownModel = monaco.editor.createModel(value, "xml");
      editor.setModel(ownModel);
    }
    editor.onDidChangeModelContent(() => {
      if (!applying) onChange?.(editor!.getValue());
      decorate();
    });
    editor.onDidChangeModel(() => {
      decorate();
      mark();
    });
    editor.addAction({
      id: "soap.send",
      label: "Send Request",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
      run: () => onSend?.(),
    });
  });

  // Swap in the right model when the request (tab) changes.
  $effect(() => {
    const key = modelKey;
    if (editor && key) editor.setModel(modelFor(key, untrack(() => value)));
  });

  // Apply external value changes (format, reset, new response) without echoing them back.
  $effect(() => {
    const v = value;
    if (!editor) return;
    const model = editor.getModel();
    if (model && model.getValue() !== v) {
      applying = true;
      if (readOnly) model.setValue(v);
      else editor.executeEdits("external", [{ range: model.getFullModelRange(), text: v }]);
      editor.pushUndoStop();
      applying = false;
    }
  });

  $effect(() => {
    editor?.updateOptions({ wordWrap: wordWrap ? "on" : "off" });
  });

  $effect(() => {
    void vars;
    decorate();
  });

  $effect(() => {
    void issues;
    void issuesFor;
    mark();
  });

  onDestroy(() => {
    editor?.dispose();
    ownModel?.dispose();
  });
</script>

<!--
  "monaco-component" makes Monaco's theme variables (--vscode-menu-background etc.) exist on this
  container. Monaco renders the right-click menu in a shadow root attached here, outside
  .monaco-editor, and it would otherwise inherit no colours (transparent menu).
-->
<div class="editor monaco-component" bind:this={el}></div>

<style>
  .editor {
    width: 100%;
    height: 100%;
    min-height: 0;
  }
</style>
