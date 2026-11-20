<script lang="ts">
  import { app, type KeychainNotice } from "../lib/state.svelte";

  let { notice }: { notice: KeychainNotice } = $props();

  const mac = $derived(app.secretBackend?.name.includes("macOS") ?? false);
  const SERVICE = "dev.soapclient.app";

  function focus(n: HTMLElement) {
    n.focus();
  }
</script>

<div class="backdrop" role="presentation">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="kc-title"
    tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && notice.resolve(false)}
  >
    <h3 id="kc-title">🔑 Secrets are stored in your {mac ? "Keychain" : "system keyring"}</h3>

    <p>
      SOAP Client keeps the values of <b>secret variables</b> (passwords, tokens, API keys) in
      {mac ? "the macOS Keychain" : "your desktop keyring (GNOME Keyring / KWallet, via Secret Service)"}. They are
      never written to project files or the autosaved session, and are only sent to the endpoints of the requests
      that use them.
    </p>

    {#if mac}
      <h4>What you'll see next</h4>
      <p>
        All of a project's secrets are kept in a single Keychain entry, read at most once per app launch. macOS may
        ask for permission, possibly with your Mac login password:
      </p>
      <blockquote>“SOAP Client” wants to use your confidential information stored in “{SERVICE}” in your keychain.</blockquote>
      <ul>
        <li><b>Always Allow</b>: recommended; you won't be asked again.</li>
        <li><b>Allow</b>: works this time; you'll be asked again next time.</li>
        <li><b>Deny</b>: the secret can't be used and the request won't be sent.</li>
      </ul>
    {:else}
      <h4>What you'll see next</h4>
      <p>If your keyring is locked, your desktop may ask you to unlock it with your login password.</p>
    {/if}

    <h4>What the app can access</h4>
    <ul>
      <li>
        <b>Only its own entries</b>, all stored under <code>{SERVICE}</code>. The service name is fixed in the app,
        so it never asks for your other {mac ? "Keychain" : "keyring"} items: website and Wi-Fi passwords, or other
        apps' credentials.
      </li>
      <li>
        Other apps' items stay protected by {mac ? "macOS" : "your keyring"}: no app can read them silently, and
        {mac ? "macOS" : "the system"} would show a prompt naming the item. If a prompt ever names anything other
        than <code>{SERVICE}</code>, choose <b>Deny</b>.
      </li>
      <li>
        You can view or delete these entries in
        {mac ? "Keychain Access (search for " : "your keyring manager, e.g. Seahorse (search for "}<code>{SERVICE}</code>).
      </li>
      {#if mac}
        <li class="muted">
          Unsigned development builds may be asked again after every rebuild; signed builds keep your choice.
        </li>
      {/if}
    </ul>

    <div class="actions">
      {#if notice.mode === "consent"}
        <button class="secondary" onclick={() => notice.resolve(false)}>Cancel</button>
        <button class="primary" use:focus onclick={() => notice.resolve(true)}>Continue</button>
      {:else}
        <button class="primary" use:focus onclick={() => notice.resolve(true)}>OK</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 80;
  }
  .dialog {
    width: min(560px, 92vw);
    max-height: 90vh;
    overflow: auto;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
    padding: 20px 22px;
    line-height: 1.5;
    user-select: text;
    -webkit-user-select: text;
  }
  h3 {
    margin: 0 0 10px;
  }
  h4 {
    margin: 16px 0 4px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  p {
    margin: 6px 0;
  }
  blockquote {
    margin: 8px 0;
    padding: 8px 12px;
    border-left: 3px solid var(--accent);
    background: var(--hover);
    border-radius: 0 6px 6px 0;
    font-size: 12.5px;
  }
  ul {
    margin: 4px 0;
    padding-left: 20px;
  }
  li {
    margin: 3px 0;
  }
  .muted {
    color: var(--muted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }
</style>
