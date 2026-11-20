<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { currentTheme, modelFor, monaco } from "../lib/monaco";
  import type { EffectiveVar } from "../lib/state.svelte";

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
  }
  let { value, modelKey, readOnly = false, wordWrap = false, onChange, onSend, vars }: Props = $props();

  let el: HTMLDivElement;
  let editor: monaco.editor.IStandaloneCodeEditor | undefined;
  let ownModel: monaco.editor.ITextModel | undefined;
  let applying = false;
  let decorations: monaco.editor.IEditorDecorationsCollection | undefined;

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
      const hover = !v
        ? `Undefined variable \`${m[1]}\` — define it in Variables & Environments`
        : v.secret
          ? `\`${v.name}\` · secret (from ${v.scope === "project" ? "Project" : "environment"})`
          : `\`${v.name}\` = \`${v.value}\``;
      list.push({
        range: new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column),
        options: { inlineClassName: v ? "var-ok" : "var-missing", hoverMessage: { value: hover } },
      });
    }
    decorations.set(list);
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
    editor.onDidChangeModel(() => decorate());
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

  onDestroy(() => {
    editor?.dispose();
    ownModel?.dispose();
  });
</script>

<div class="editor" bind:this={el}></div>

<style>
  .editor {
    width: 100%;
    height: 100%;
    min-height: 0;
  }
</style>
