<script lang="ts">
  import { plural, shortcut } from "../lib/platform";
  import { app, basename } from "../lib/state.svelte";
  import type { Interface, Operation, WsdlEntry } from "../lib/types";

  let filter = $state("");
  let menuOpen = $state(false);
  let renamingProject = $state(false);
  let renamingId = $state<string | null>(null);
  /** "<wsdlId>/<interface>" being renamed. */
  let renamingIface = $state<string | null>(null);
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
        <button onclick={() => menu(() => app.openProject())}>Open…<kbd>{shortcut("O")}</kbd></button>
        <button onclick={() => menu(() => app.save())}>Save<kbd>{shortcut("S")}</kbd></button>
        <button onclick={() => menu(() => app.save(true))}>Save As…<kbd>{shortcut("S", true)}</kbd></button>
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

  <div class="envrow">
    <select
      value={app.project.activeEnvironment ?? ""}
      onchange={(e) => app.setActiveEnvironment(e.currentTarget.value || null)}
      title="Active environment"
    >
      <option value="">No environment</option>
      {#each app.project.environments as env (env.id)}
        <option value={env.id}>{env.name}</option>
      {/each}
    </select>
    <button class="vars" title="Variables & Environments" onclick={() => (app.varsOpen = true)}>{"{{x}}"}</button>
  </div>

  <div class="filter">
    <input placeholder="Filter operations" bind:value={filter} />
  </div>

  <div class="tree">
    {#each app.project.wsdls as w (w.id)}
      <div class="row group" title={`${w.source}${w.imports?.length ? `\n+ ${plural(w.imports.length, "imported document")}` : ""}`}>
        <span class="label caption">{basename(w.source.replace(/\?wsdl$/i, ""))}</span>
        <span class="actions">
          <button class="act danger" title="Remove WSDL" onclick={() => app.removeWsdl(w.id)}>×</button>
        </span>
      </div>
      {#each w.document?.interfaces ?? [] as i (i.name)}
        {@const label = app.interfaceLabel(w.id, i.name)}
        {@const ikey = `${w.id}/${i.name}`}
        {@const filtering = filter.trim() !== ""}
        {@const open = filtering || !app.isCollapsed(w.id, i.name)}
        {#if renamingIface === ikey}
          <div class="row l0">
            <span class="chev">{open ? "▾" : "▸"}</span>
            <input
              class="rename"
              value={label}
              placeholder={i.name}
              use:focusSelect
              onkeydown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
                if (e.key === "Escape") renamingIface = null;
              }}
              onblur={(e) => {
                if (renamingIface === ikey) app.renameInterface(w.id, i.name, e.currentTarget.value);
                renamingIface = null;
              }}
            />
          </div>
        {:else}
          <div
            class="row l0 iface"
            role="button"
            tabindex="0"
            aria-expanded={open}
            title={label === i.name ? `${i.name} (double-click to rename)` : `${label}\nBinding: ${i.name}`}
            onclick={() => !filtering && app.toggleCollapsed(w.id, i.name)}
            ondblclick={() => (renamingIface = ikey)}
            onkeydown={(e) => {
              if ((e.key === "Enter" || e.key === " ") && !filtering) {
                e.preventDefault();
                app.toggleCollapsed(w.id, i.name);
              } else if (e.key === "F2") renamingIface = ikey;
            }}
          >
            <span class="chev">{open ? "▾" : "▸"}</span>
            <span class="label strong">{label}</span>
            <span class="badge">SOAP {i.soapVersion}</span>
            <!-- Count slot is always reserved so badges line up whether expanded or not. -->
            <span class="count" class:invisible={open} title="Operations">{i.operations.length}</span>
            <span class="actions">
              <button
                class="act"
                title="Rename"
                onclick={(e) => {
                  e.stopPropagation();
                  renamingIface = ikey;
                }}>✎</button
              >
            </span>
          </div>
        {/if}
        {#each open ? i.operations.filter(matches) : [] as o (o.name)}
          {@const reqs = app.requestsFor(w.id, i.name, o.name)}
          {@const key = opKey(w, i, o)}
          <button class="row l1 op" title={o.documentation ?? o.name} onclick={() => clickOperation(w, i, o)}>
            <span class="chev">{reqs.length ? (expanded[key] ? "▾" : "▸") : ""}</span>
            <span class="label">{o.name}</span>
            {#if reqs.length}<span class="count">{reqs.length}</span>{/if}
          </button>
          {#if expanded[key]}
            {#each reqs as r (r.id)}
              {#if renamingId === r.id}
                <div class="row l2">
                  <span class="chev"></span>
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
                </div>
              {:else}
                <div
                  class="row l2 req"
                  class:active={app.activeId === r.id}
                  role="button"
                  tabindex="0"
                  title="Double-click to rename"
                  onclick={() => app.openRequest(r.id)}
                  ondblclick={() => (renamingId = r.id)}
                  onkeydown={(e) => {
                    if (e.key === "Enter") app.openRequest(r.id);
                    else if (e.key === "F2") renamingId = r.id;
                  }}
                >
                  <span class="chev"></span>
                  <span class="label">{r.name}</span>
                  <span class="actions">
                    <button
                      class="act"
                      title="Duplicate request ({shortcut('D')})"
                      onclick={(e) => {
                        e.stopPropagation();
                        app.duplicating = r.id;
                      }}>⧉</button
                    >
                    <button
                      class="act danger"
                      title="Delete request"
                      onclick={(e) => {
                        e.stopPropagation();
                        app.deleteRequest(r.id);
                      }}>×</button
                    >
                  </span>
                </div>
              {/if}
            {/each}
            <button class="row l2 new" onclick={() => app.newRequest(w, i, o)}>
              <span class="chev"></span>
              <span class="label">+ New request</span>
            </button>
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
    min-height: 0;
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
  .envrow {
    display: flex;
    gap: 6px;
    padding: 8px 8px 0;
  }
  .envrow select {
    flex: 1;
    min-width: 0;
  }
  .vars {
    font-family: var(--mono);
    font-size: 11px;
    padding: 0 8px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: var(--accent);
  }
  .vars:hover {
    background: var(--hover);
  }
  .filter {
    padding: 8px;
  }
  .filter input {
    width: 100%;
  }
  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 6px 8px;
  }

  /* One row style for every tree level: same height, hover and right-hand column. */
  .row {
    --indent: 14px;
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 24px;
    padding: 0 6px 0 calc(4px + var(--level, 0) * var(--indent));
    border-radius: 5px;
    text-align: left;
    color: var(--fg);
    cursor: default;
    min-width: 0;
  }
  .row.l0 {
    --level: 0;
  }
  .row.l1 {
    --level: 1;
  }
  .row.l2 {
    --level: 2;
  }
  .row:not(.group):hover {
    background: var(--hover);
  }
  .row.active,
  .row.active:hover {
    background: var(--selected);
  }
  .row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .chev {
    flex: 0 0 10px;
    margin-right: -2px;
    color: var(--muted);
    font-size: 10px;
    text-align: center;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .label.strong {
    font-weight: 600;
  }

  .row.group {
    height: 22px;
    margin-top: 10px;
    padding-left: 4px;
  }
  .row.group:first-child {
    margin-top: 4px;
  }
  .caption {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--muted);
  }

  .badge,
  .count {
    flex-shrink: 0;
    font-size: 10px;
    line-height: 16px;
    border-radius: 4px;
    color: var(--muted);
  }
  .badge {
    padding: 0 4px;
    border: 1px solid var(--border-strong);
    line-height: 14px;
  }
  .count {
    min-width: 20px;
    padding: 0 5px;
    text-align: center;
    background: var(--hover);
    border-radius: 8px;
  }
  .invisible {
    visibility: hidden;
  }

  .row.new .label {
    color: var(--muted);
  }
  .row.new:hover .label {
    color: var(--accent);
  }

  /* Hover actions float over the right edge instead of taking layout space. */
  .actions {
    position: absolute;
    right: 2px;
    top: 50%;
    transform: translateY(-50%);
    display: none;
    gap: 2px;
    padding-left: 6px;
    border-radius: 5px;
    background: linear-gradient(var(--hover), var(--hover)), var(--bg-side);
  }
  .row.group .actions {
    background: var(--bg-side);
  }
  .row.active .actions {
    background: linear-gradient(var(--selected), var(--selected)), var(--bg-side);
  }
  .row:hover .actions,
  .row:focus-within .actions {
    display: flex;
  }
  .act {
    width: 20px;
    height: 20px;
    border-radius: 4px;
    color: var(--muted);
    line-height: 20px;
    text-align: center;
  }
  .act:hover {
    background: var(--hover);
    color: var(--accent);
  }
  .act.danger:hover {
    color: var(--danger);
  }

  .rename {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 2px 6px !important;
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
