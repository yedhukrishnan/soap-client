<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import DuplicateDialog from "./components/DuplicateDialog.svelte";
  import EndpointVarDialog from "./components/EndpointVarDialog.svelte";
  import ImportWsdlDialog from "./components/ImportWsdlDialog.svelte";
  import KeychainNotice from "./components/KeychainNotice.svelte";
  import RequestPane from "./components/RequestPane.svelte";
  import RequestTabBar from "./components/RequestTabBar.svelte";
  import ResponsePane from "./components/ResponsePane.svelte";
  import SecretPromptDialog from "./components/SecretPromptDialog.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import VariablesDialog from "./components/VariablesDialog.svelte";
  import { app } from "./lib/state.svelte";

  let sidebarWidth = $state(Number(localStorage.getItem("soap-client.sidebar")) || 260);
  let splitPct = $state(Number(localStorage.getItem("soap-client.split")) || 50);
  let mainEl = $state<HTMLElement>();

  const win = getCurrentWindow();

  $effect(() => {
    win.setTitle(`${app.project.name}${app.dirty ? " •" : ""} — SOAP Client`);
  });

  // Autosave the session whenever the project, tabs or selection change.
  $effect(() => {
    void [app.revision, app.dirty, app.activeId, app.openTabs.length, app.path];
    app.scheduleSave();
  });

  onMount(() => {
    app.restoreSession();
    // Nothing is lost on close: flush the session, then close.
    const unlisten = win.onCloseRequested(async (e) => {
      e.preventDefault();
      await app.saveSessionNow();
      await win.destroy();
    });
    return () => unlisten.then((f) => f());
  });

  function onKey(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    if (!mod) return;
    const k = e.key.toLowerCase();
    if (k === "s") {
      e.preventDefault();
      app.save(e.shiftKey);
    } else if (k === "o") {
      e.preventDefault();
      app.openProject();
    } else if (k === "d" && app.activeId) {
      e.preventDefault();
      app.duplicating = app.activeId;
    } else if (k === "enter" && app.activeId) {
      e.preventDefault();
      app.send(app.activeId);
    }
  }

  /** Generic pointer-drag helper for the two splitters. */
  function drag(onMove: (e: PointerEvent) => void, save: () => void) {
    return (down: PointerEvent) => {
      down.preventDefault();
      const target = down.currentTarget as HTMLElement;
      target.setPointerCapture(down.pointerId);
      const move = (e: PointerEvent) => onMove(e);
      const up = () => {
        target.removeEventListener("pointermove", move);
        target.removeEventListener("pointerup", up);
        save();
      };
      target.addEventListener("pointermove", move);
      target.addEventListener("pointerup", up);
    };
  }

  const dragSidebar = drag(
    (e) => (sidebarWidth = Math.min(520, Math.max(180, e.clientX))),
    () => localStorage.setItem("soap-client.sidebar", String(sidebarWidth)),
  );
  const dragSplit = drag(
    (e) => {
      if (!mainEl) return;
      const r = mainEl.getBoundingClientRect();
      splitPct = Math.min(85, Math.max(15, ((e.clientY - r.top) / r.height) * 100));
    },
    () => localStorage.setItem("soap-client.split", String(splitPct)),
  );
</script>

<svelte:window onkeydown={onKey} />

<div class="layout" style="grid-template-columns: {sidebarWidth}px 4px 1fr">
  <Sidebar />
  <div class="vsplit" role="separator" aria-orientation="vertical" onpointerdown={dragSidebar}></div>

  <main>
    <RequestTabBar />
    {#if app.active}
      {@const req = app.active}
      <div class="stack" bind:this={mainEl} style="grid-template-rows: {splitPct}% 5px 1fr">
        <RequestPane {req} />
        <div class="hsplit" role="separator" aria-orientation="horizontal" onpointerdown={dragSplit}></div>
        <ResponsePane {req} />
      </div>
    {:else if !app.restored}
      <div class="welcome"><span class="spinner"></span></div>
    {:else}
      <div class="welcome">
        <h2>SOAP Client</h2>
        {#if app.project.wsdls.length}
          <p>Pick an operation in the sidebar to create a request.</p>
        {:else}
          <p>Import a WSDL to browse its operations and generate sample requests.</p>
          <div class="actions">
            <button class="primary" onclick={() => (app.importOpen = true)}>Import WSDL…</button>
            <button class="secondary" onclick={() => app.openProject()}>Open Project…</button>
          </div>
        {/if}
        <p class="keys"><kbd>⌘↵</kbd> send · <kbd>⌘D</kbd> duplicate · <kbd>⌘S</kbd> save · <kbd>⌘O</kbd> open</p>
      </div>
    {/if}
  </main>
</div>

{#if app.importOpen}
  <ImportWsdlDialog />
{/if}

{#if app.varsOpen}
  <VariablesDialog />
{/if}

{#if app.secretPrompt}
  <SecretPromptDialog prompt={app.secretPrompt} />
{/if}

{#if app.endpointPrompt}
  {#key app.endpointPrompt}
    <EndpointVarDialog reqId={app.endpointPrompt} />
  {/key}
{/if}

{#if app.keychainNotice}
  <KeychainNotice notice={app.keychainNotice} />
{/if}

{#if app.duplicating}
  {#key app.duplicating}
    <DuplicateDialog id={app.duplicating} />
  {/key}
{/if}

<style>
  .layout {
    display: grid;
    height: 100vh;
  }
  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }
  .stack {
    flex: 1;
    display: grid;
    min-height: 0;
  }
  .vsplit {
    cursor: col-resize;
    background: var(--border);
    width: 1px;
    margin: 0 1.5px;
  }
  .vsplit:hover {
    background: var(--accent);
    width: 4px;
    margin: 0;
  }
  .hsplit {
    cursor: row-resize;
    background: var(--bg-side);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  .hsplit:hover {
    background: var(--accent);
  }
  .welcome {
    margin: auto;
    text-align: center;
    color: var(--muted);
  }
  .welcome h2 {
    color: var(--fg);
    margin-bottom: 4px;
  }
  .actions {
    display: flex;
    gap: 8px;
    justify-content: center;
    margin: 18px 0;
  }
  .keys {
    font-size: 12px;
    margin-top: 28px;
  }
</style>
