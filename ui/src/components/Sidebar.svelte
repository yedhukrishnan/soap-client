<script lang="ts">
  import { app, basename } from "../lib/state.svelte";
  import type { Interface, Operation, WsdlEntry } from "../lib/types";

  let filter = $state("");
  let menuOpen = $state(false);
  let renamingProject = $state(false);
  let renamingId = $state<string | null>(null);
  let expanded = $state<Record<string, boolean>>({});

  const opKey = (w: WsdlEntry, i: Interface, o: Operation) => `${w.id}/${i.name}/${o.name}`;

  const matches = (o: Operation) => o.name.toLowerCase().includes(filter.trim().toLowerCase());

  function clickOperation(w: WsdlEntry, i: Interface, o: Operation) {
    const reqs = app.requestsFor(w.id, i.name, o.name);
    const key = opKey(w, i, o);
    if (reqs.length === 0) {
      app.newRequest(w, i, o);
      expanded[key] = true;
    } else {
      expanded[key] = !expanded[key];
      if (expanded[key]) app.openRequest(reqs[reqs.length - 1].id);
    }
  }

  function menu(action: () => unknown) {
    menuOpen = false;
    action();
  }

  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<svelte:window onclick={() => (menuOpen = false)} />

<aside class="sidebar">
  <div class="project-row">
    {#if renamingProject}
      <input
        class="rename"
        value={app.project.name}
        use:focusSelect
        onkeydown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") renamingProject = false;
        }}
        onblur={(e) => {
          const v = e.currentTarget.value.trim();
          if (renamingProject && v && v !== app.project.name) {
            app.project.name = v;
            app.markDirty();
          }
          renamingProject = false;
        }}
      />
    {:else}
      <button
        class="project-btn"
        title="Project menu"
        onclick={(e) => {
          e.stopPropagation();
          menuOpen = !menuOpen;
        }}
      >
        <span class="burger">☰</span>
        <span class="name">{app.project.name}</span>
        {#if app.dirty}<span class="dot" title="Unsaved changes">●</span>{/if}
        <span class="caret">▾</span>
      </button>
    {/if}
    {#if menuOpen}
      <div class="menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
        <button onclick={() => menu(() => app.newProject())}>New Project</button>
        <button onclick={() => menu(() => app.openProject())}>Open…<kbd>⌘O</kbd></button>
        <button onclick={() => menu(() => app.save())}>Save<kbd>⌘S</kbd></button>
        <button onclick={() => menu(() => app.save(true))}>Save As…<kbd>⇧⌘S</kbd></button>
        <button onclick={() => menu(() => (renamingProject = true))}>Rename Project</button>
        {#if app.recent.length}
          <div class="sep">Recent</div>
          {#each app.recent as p (p)}
            <button class="recent" title={p} onclick={() => menu(() => app.openProject(p))}>{basename(p)}</button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  <div class="filter">
    <input placeholder="Filter operations" bind:value={filter} />
  </div>

  <div class="tree">
    {#each app.project.wsdls as w (w.id)}
      <div class="wsdl" title={`${w.source}${w.imports?.length ? `\n+ ${w.imports.length} imported document(s)` : ""}`}>
        <span class="wsdl-name">{basename(w.source.replace(/\?wsdl$/i, ""))}</span>
        <button class="icon" title="Remove WSDL" onclick={() => app.removeWsdl(w.id)}>×</button>
      </div>
      {#each w.document?.interfaces ?? [] as i (i.name)}
        <div class="iface">
          {i.name}
          <span class="badge">SOAP {i.soapVersion}</span>
        </div>
        {#each i.operations.filter(matches) as o (o.name)}
          {@const reqs = app.requestsFor(w.id, i.name, o.name)}
          {@const key = opKey(w, i, o)}
          <button class="op" title={o.documentation ?? o.name} onclick={() => clickOperation(w, i, o)}>
            <span class="chev">{reqs.length ? (expanded[key] ? "▾" : "▸") : ""}</span>
            <span class="op-name">{o.name}</span>
            {#if reqs.length}<span class="count">{reqs.length}</span>{/if}
          </button>
          {#if expanded[key]}
            {#each reqs as r (r.id)}
              <div class="req" class:active={app.activeId === r.id}>
                {#if renamingId === r.id}
                  <input
                    class="rename"
                    value={r.name}
                    use:focusSelect
                    onkeydown={(e) => {
                      if (e.key === "Enter") e.currentTarget.blur();
                      if (e.key === "Escape") renamingId = null;
                    }}
                    onblur={(e) => {
                      const v = e.currentTarget.value.trim();
                      if (renamingId && v && v !== r.name) {
                        r.name = v;
                        app.markDirty();
                      }
                      renamingId = null;
                    }}
                  />
                {:else}
                  <button
                    class="req-name"
                    onclick={() => app.openRequest(r.id)}
                    ondblclick={() => (renamingId = r.id)}
                    title="Double-click to rename">{r.name}</button
                  >
                  <button class="icon dup" title="Duplicate request (⌘D)" onclick={() => (app.duplicating = r.id)}>⧉</button>
                  <button class="icon" title="Delete request" onclick={() => app.deleteRequest(r.id)}>×</button>
                {/if}
              </div>
            {/each}
            <button class="req new" onclick={() => app.newRequest(w, i, o)}>+ New request</button>
          {/if}
        {/each}
      {/each}
    {:else}
      <div class="empty">
        No WSDLs yet.<br />Import one to get started.
      </div>
    {/each}
  </div>

  <button class="import" onclick={() => (app.importOpen = true)}>+ Import WSDL</button>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    min-width: 0;
  }
  .project-row {
    position: relative;
    padding: 8px;
    border-bottom: 1px solid var(--border);
  }
  .project-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 8px;
    border-radius: 6px;
    font-weight: 600;
    text-align: left;
  }
  .project-btn:hover {
    background: var(--hover);
  }
  .project-btn .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .burger,
  .caret {
    color: var(--muted);
  }
  .dot {
    color: var(--accent);
    font-size: 10px;
  }
  .menu {
    position: absolute;
    left: 8px;
    right: 8px;
    top: 44px;
    z-index: 20;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .menu button {
    display: flex;
    justify-content: space-between;
    padding: 6px 10px;
    border-radius: 5px;
    text-align: left;
  }
  .menu button:hover {
    background: var(--accent);
    color: white;
  }
  .menu kbd {
    color: inherit;
    opacity: 0.6;
    font-family: inherit;
  }
  .menu .sep {
    padding: 6px 10px 2px;
    font-size: 11px;
    color: var(--muted);
    text-transform: uppercase;
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  .menu .recent {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: block;
  }
  .filter {
    padding: 8px;
  }
  .filter input {
    width: 100%;
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding: 0 4px 8px;
  }
  .wsdl {
    display: flex;
    align-items: center;
    padding: 8px 6px 2px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--muted);
  }
  .wsdl-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .iface {
    padding: 6px 6px 2px;
    font-weight: 600;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .badge {
    font-size: 10px;
    font-weight: 500;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 4px;
  }
  .op {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 3px 6px;
    border-radius: 5px;
    text-align: left;
  }
  .op:hover,
  .req:hover {
    background: var(--hover);
  }
  .chev {
    width: 12px;
    color: var(--muted);
    font-size: 10px;
  }
  .op-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 11px;
    color: var(--muted);
    background: var(--hover);
    border-radius: 8px;
    padding: 0 6px;
  }
  .req {
    display: flex;
    align-items: center;
    padding: 1px 6px 1px 28px;
    border-radius: 5px;
    width: 100%;
    text-align: left;
  }
  .req.active {
    background: var(--selected);
  }
  .req-name {
    flex: 1;
    text-align: left;
    padding: 2px 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .req.new {
    color: var(--muted);
    padding-top: 3px;
    padding-bottom: 3px;
  }
  .icon {
    visibility: hidden;
    color: var(--muted);
    padding: 0 4px;
    border-radius: 4px;
  }
  .req:hover .icon,
  .wsdl:hover .icon {
    visibility: visible;
  }
  .icon:hover {
    color: var(--danger);
  }
  .icon.dup:hover {
    color: var(--accent);
  }
  .rename {
    width: 100%;
  }
  .empty {
    color: var(--muted);
    text-align: center;
    padding: 32px 8px;
    line-height: 1.6;
  }
  .import {
    margin: 8px;
    padding: 8px;
    border: 1px dashed var(--border-strong);
    border-radius: 6px;
    color: var(--accent);
    font-weight: 500;
  }
  .import:hover {
    background: var(--hover);
  }
</style>
