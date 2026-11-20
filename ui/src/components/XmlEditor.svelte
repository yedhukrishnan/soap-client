<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { currentTheme, modelFor, monaco } from "../lib/monaco";

  interface Props {
    value: string;
    /** Editable editors keep a per-key model (undo history survives tab switches). */
    modelKey?: string;
    readOnly?: boolean;
    wordWrap?: boolean;
    onChange?: (v: string) => void;
    onSend?: () => void;
  }
  let { value, modelKey, readOnly = false, wordWrap = false, onChange, onSend }: Props = $props();

  let el: HTMLDivElement;
  let editor: monaco.editor.IStandaloneCodeEditor | undefined;
  let ownModel: monaco.editor.ITextModel | undefined;
  let applying = false;

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
