// Application state (Svelte 5 runes). The project is owned by the frontend and
// autosaved as a session; secret variable values only ever live in the Rust backend
// (OS keychain) — the frontend can set, delete and check them, never read them.
import { ask, message, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { disposeModel } from "./monaco";
import type {
  AssertionResult, Environment, Interface, Operation, Project, ResponseData, SavedRequest, SecretBackend,
  VarBinding, Variable, WsdlEntry,
} from "./types";

export interface RunState {
  sending: boolean;
  callId?: string;
  response?: ResponseData;
  error?: string;
  results?: AssertionResult[];
}

const RECENT_KEY = "soap-client.recent";
const PROJECT_FILTER = [{ name: "SOAP Client project", extensions: ["json"] }];

export const uid = () => crypto.randomUUID();

const emptyProject = (): Project => ({
  version: 1,
  id: uid(),
  name: "Untitled",
  variables: [],
  environments: [],
  activeEnvironment: null,
  wsdls: [],
  requests: [],
});

/** "project" for project-level variables, otherwise an environment id. */
export type Scope = "project" | string;

export const VAR_NAME = /^[A-Za-z_][A-Za-z0-9_.-]*$/;

export interface EffectiveVar {
  name: string;
  secret: boolean;
  value: string;
  scope: Scope;
  key: string;
}

/** A pending "move this literal into a secret variable" action. */
export interface SecretPrompt {
  suggestedName: string;
  value: string;
  /** Called with the placeholder (e.g. "{{password}}") once the secret is stored. */
  apply: (placeholder: string) => void;
}

const AUTOSAVE_MS = 700;
const KEYCHAIN_ACK = "soap-client.keychain-ack";

export class KeychainDeclined extends Error {
  constructor() {
    super("Keychain access was not allowed, so secret variables can't be used for this action.");
  }
}

/** The explanatory note shown before the OS keychain is first used. */
export interface KeychainNotice {
  /** "consent" gates an action; "info" is just the note, reopened from the Variables dialog. */
  mode: "consent" | "info";
  resolve: (ok: boolean) => void;
}

export function basename(p: string) {
  return p.split(/[\\/]/).pop() || p;
}

class AppState {
  project = $state<Project>(emptyProject());
  path = $state<string | null>(null);
  dirty = $state(false);
  openTabs = $state<string[]>([]);
  activeId = $state<string | null>(null);
  runs = $state<Record<string, RunState>>({});
  recent = $state<string[]>(JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]"));
  importOpen = $state(false);
  /** Id of the request whose "Duplicate" name dialog is open. */
  duplicating = $state<string | null>(null);
  varsOpen = $state(false);
  secretPrompt = $state<SecretPrompt | null>(null);
  secretBackend = $state<SecretBackend | null>(null);
  /** keychain key -> has a stored value */
  secretStatus = $state<Record<string, boolean>>({});
  keychainNotice = $state<KeychainNotice | null>(null);
  private consentPending: Promise<boolean> | null = null;
  /** Bumped on every edit; drives autosave. */
  revision = $state(0);
  restored = $state(false);
  private saveTimer: ReturnType<typeof setTimeout> | undefined;

  active = $derived(this.project.requests.find((r) => r.id === this.activeId) ?? null);
  activeEnvironment = $derived(
    this.project.environments.find((e) => e.id === this.project.activeEnvironment) ?? null,
  );

  /** Project variables overridden by the active environment's. */
  effectiveVars = $derived.by(() => {
    const out = new Map<string, EffectiveVar>();
    const add = (vars: Variable[], scope: Scope) => {
      for (const v of vars) {
        if (!VAR_NAME.test(v.name)) continue;
        out.set(v.name, { name: v.name, secret: v.secret, value: v.value, scope, key: this.secretKey(scope, v.name) });
      }
    };
    add(this.project.variables, "project");
    if (this.activeEnvironment) add(this.activeEnvironment.variables, this.activeEnvironment.id);
    return out;
  });

  markDirty() {
    this.dirty = true;
    this.revision++;
  }

  request(id: string) {
    return this.project.requests.find((r) => r.id === id);
  }

  /** Resolve the WSDL operation a saved request was created from. */
  operationOf(req: SavedRequest): { wsdl: WsdlEntry; iface: Interface; op: Operation } | null {
    const wsdl = this.project.wsdls.find((w) => w.id === req.wsdlId);
    const iface = wsdl?.document?.interfaces.find((i) => i.name === req.interfaceName);
    const op = iface?.operations.find((o) => o.name === req.operationName);
    return wsdl && iface && op ? { wsdl, iface, op } : null;
  }

  requestsFor(wsdlId: string, ifaceName: string, opName: string) {
    return this.project.requests.filter(
      (r) => r.wsdlId === wsdlId && r.interfaceName === ifaceName && r.operationName === opName,
    );
  }

  // ---- requests & tabs ----

  newRequest(wsdl: WsdlEntry, iface: Interface, op: Operation) {
    const existing = this.requestsFor(wsdl.id, iface.name, op.name);
    let n = existing.length + 1;
    while (existing.some((r) => r.name === `Request ${n}`)) n++;
    const req: SavedRequest = {
      id: uid(),
      name: `Request ${n}`,
      wsdlId: wsdl.id,
      interfaceName: iface.name,
      operationName: op.name,
      spec: {
        endpoint: iface.endpoints[0] ?? "",
        soapVersion: iface.soapVersion,
        soapAction: op.soapAction,
        body: op.sampleRequest,
        headers: [],
        auth: { kind: "none" },
        timeoutSecs: 60,
        acceptInvalidCerts: false,
      },
      assertions: [],
    };
    this.project.requests.push(req);
    this.markDirty();
    this.openRequest(req.id);
    return req;
  }

  /** "+" in the tab bar: another request for the active tab's operation. */
  newRequestLikeActive() {
    const cur = this.active && this.operationOf(this.active);
    if (cur) this.newRequest(cur.wsdl, cur.iface, cur.op);
  }

  openRequest(id: string) {
    if (!this.openTabs.includes(id)) this.openTabs.push(id);
    this.activeId = id;
  }

  closeTab(id: string) {
    const i = this.openTabs.indexOf(id);
    if (i < 0) return;
    this.openTabs.splice(i, 1);
    if (this.activeId === id) this.activeId = this.openTabs[Math.min(i, this.openTabs.length - 1)] ?? null;
  }

  /** Validation message for a proposed name, or null if it is fine. */
  nameError(req: SavedRequest, name: string): string | null {
    const n = name.trim();
    if (!n) return "Name cannot be empty";
    const clash = this.requestsFor(req.wsdlId, req.interfaceName, req.operationName).some((r) => r.name === n);
    return clash ? `"${n}" already exists for ${req.operationName}` : null;
  }

  /** Copy a request (body, headers, auth, assertions) under a new name and open it. */
  duplicateRequest(id: string, name: string) {
    const src = this.request(id);
    if (!src || this.nameError(src, name)) return;
    const copy: SavedRequest = {
      ...structuredClone($state.snapshot(src)),
      id: uid(),
      name: name.trim(),
    };
    copy.assertions = copy.assertions.map((a) => ({ ...a, id: uid() }));
    const i = this.project.requests.findIndex((r) => r.id === id);
    this.project.requests.splice(i + 1, 0, copy);
    this.markDirty();
    this.openRequest(copy.id);
  }

  async deleteRequest(id: string) {
    const req = this.request(id);
    if (!req) return;
    if (!(await ask(`Delete "${req.operationName} · ${req.name}"?`, { title: "Delete request", kind: "warning" })))
      return;
    this.closeTab(id);
    this.project.requests = this.project.requests.filter((r) => r.id !== id);
    delete this.runs[id];
    disposeModel(id);
    this.markDirty();
  }

  // ---- WSDLs ----

  async importWsdl(source: string) {
    const entry = await api.importWsdl(source);
    this.project.wsdls.push(entry);
    if (this.project.name === "Untitled" && this.project.wsdls.length === 1) {
      this.project.name = entry.document?.interfaces[0]?.name.replace(/Soap(12)?$/, "") || "Untitled";
    }
    this.markDirty();
  }

  async removeWsdl(id: string) {
    const w = this.project.wsdls.find((x) => x.id === id);
    if (!w) return;
    const reqs = this.project.requests.filter((r) => r.wsdlId === id);
    const extra = reqs.length ? ` and its ${reqs.length} saved request(s)` : "";
    if (!(await ask(`Remove ${basename(w.source)}${extra} from the project?`, { title: "Remove WSDL", kind: "warning" })))
      return;
    for (const r of reqs) {
      this.closeTab(r.id);
      delete this.runs[r.id];
      disposeModel(r.id);
    }
    this.project.requests = this.project.requests.filter((r) => r.wsdlId !== id);
    this.project.wsdls = this.project.wsdls.filter((x) => x.id !== id);
    this.markDirty();
  }

  // ---- sending ----

  async send(id: string) {
    const req = this.request(id);
    if (!req || this.runs[id]?.sending) return;
    const callId = uid();
    const usesSecret = [...this.effectiveVars.values()].some(
      (v) => v.secret && JSON.stringify(req.spec).match(new RegExp(`\\{\\{\\s*${v.name.replace(/[.]/g, "\\.")}\\s*\\}\\}`)),
    );
    if (usesSecret && !(await this.ensureKeychainConsent())) {
      this.runs[id] = { sending: false, error: new KeychainDeclined().message };
      return;
    }
    this.runs[id] = { ...this.runs[id], sending: true, callId, error: undefined };
    try {
      const response = await api.sendRequest(callId, $state.snapshot(req.spec), this.bindings());
      const enabled = req.assertions.filter((a) => a.enabled);
      const results = enabled.length ? await api.runAssertions($state.snapshot(enabled), response) : [];
      this.runs[id] = { sending: false, response, results };
    } catch (e) {
      this.runs[id] = { sending: false, error: String(e) };
    }
  }

  async cancel(id: string) {
    const callId = this.runs[id]?.callId;
    if (callId) await api.cancelRequest(callId);
  }

  /** Re-evaluate assertions against the last response (after editing them). */
  async rerunAssertions(id: string) {
    const req = this.request(id);
    const run = this.runs[id];
    if (!req || !run?.response) return;
    const enabled = req.assertions.filter((a) => a.enabled);
    run.results = enabled.length ? await api.runAssertions($state.snapshot(enabled), $state.snapshot(run.response)) : [];
  }

  // ---- project files ----

  private async confirmDiscard() {
    return !this.dirty || (await ask("You have unsaved changes. Discard them?", { title: "Unsaved changes", kind: "warning" }));
  }

  private reset(project: Project, path: string | null) {
    for (const r of this.project.requests) disposeModel(r.id);
    this.project = project;
    this.path = path;
    this.dirty = false;
    this.openTabs = [];
    this.activeId = null;
    this.runs = {};
    this.secretStatus = {};
    this.revision++;
  }

  async newProject() {
    if (await this.confirmDiscard()) this.reset(emptyProject(), null);
  }

  async openProject(path?: string) {
    if (!(await this.confirmDiscard())) return;
    const chosen = path ?? (await openDialog({ filters: PROJECT_FILTER, multiple: false, directory: false }));
    if (!chosen) return;
    try {
      const project = await api.openProject(chosen);
      this.reset(project, chosen);
      this.remember(chosen);
      if (project.requests[0]) this.openRequest(project.requests[0].id);
    } catch (e) {
      await message(String(e), { title: "Could not open project", kind: "error" });
      if (path) this.forget(path);
    }
  }

  async save(saveAs = false) {
    let path = this.path;
    if (!path || saveAs) {
      path = await saveDialog({
        filters: PROJECT_FILTER,
        defaultPath: `${this.project.name.replace(/[^\w.-]+/g, "-")}.soapproj.json`,
      });
      if (!path) return false;
    }
    try {
      await api.saveProject(path, $state.snapshot(this.project));
      this.path = path;
      this.dirty = false;
      this.remember(path);
      return true;
    } catch (e) {
      await message(String(e), { title: "Could not save project", kind: "error" });
      return false;
    }
  }

  private remember(path: string) {
    this.recent = [path, ...this.recent.filter((p) => p !== path)].slice(0, 8);
    localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
  }

  private forget(path: string) {
    this.recent = this.recent.filter((p) => p !== path);
    localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
  }

  // ---- variables, environments & secrets ----

  secretKey(scope: Scope, name: string) {
    return `${this.project.id}:${scope}:${name}`;
  }

  varsOf(scope: Scope): Variable[] {
    if (scope === "project") return this.project.variables;
    return this.project.environments.find((e) => e.id === scope)?.variables ?? [];
  }

  scopeName(scope: Scope) {
    return scope === "project" ? "Project" : (this.project.environments.find((e) => e.id === scope)?.name ?? "?");
  }

  /** What the backend needs to resolve {{vars}} for a send. */
  bindings(): VarBinding[] {
    return [...this.effectiveVars.values()].map((v) =>
      v.secret ? { name: v.name, secretKey: v.key } : { name: v.name, value: v.value },
    );
  }

  async refreshSecretStatus() {
    const keys: string[] = [];
    for (const scope of ["project", ...this.project.environments.map((e) => e.id)])
      for (const v of this.varsOf(scope)) if (v.secret) keys.push(this.secretKey(scope, v.name));
    if (!keys.length || !this.keychainAcknowledged()) return;
    try {
      const status = await api.secretStatus(keys);
      keys.forEach((k, i) => (this.secretStatus[k] = status[i]));
    } catch (e) {
      console.error(e);
    }
  }

  // ---- keychain consent ----

  keychainAcknowledged() {
    return !this.secretBackend?.persistent || localStorage.getItem(KEYCHAIN_ACK) === "1";
  }

  /** Show the keychain note once before the first keychain access. False if the user cancels. */
  ensureKeychainConsent(): Promise<boolean> {
    if (this.keychainAcknowledged()) return Promise.resolve(true);
    this.consentPending ??= new Promise<boolean>((resolve) => {
      this.keychainNotice = {
        mode: "consent",
        resolve: (ok) => {
          if (ok) localStorage.setItem(KEYCHAIN_ACK, "1");
          this.keychainNotice = null;
          this.consentPending = null;
          resolve(ok);
        },
      };
    });
    return this.consentPending;
  }

  showKeychainInfo() {
    this.keychainNotice = { mode: "info", resolve: () => (this.keychainNotice = null) };
  }

  private async requireKeychain() {
    if (!(await this.ensureKeychainConsent())) throw new KeychainDeclined();
  }

  async setSecretValue(scope: Scope, name: string, value: string) {
    const key = this.secretKey(scope, name);
    await this.requireKeychain();
    await api.secretSet(key, value);
    this.secretStatus[key] = true;
  }

  /** Toggle a variable between plain and secret, moving its value accordingly. */
  async setSecret(scope: Scope, v: Variable, secret: boolean) {
    if (v.secret === secret) return;
    await this.requireKeychain();
    const key = this.secretKey(scope, v.name);
    if (secret) {
      if (v.value) await this.setSecretValue(scope, v.name, v.value);
      v.value = "";
    } else {
      await this.requireKeychain();
      await api.secretDelete([key]);
      delete this.secretStatus[key];
    }
    v.secret = secret;
    this.markDirty();
  }

  async renameVariable(scope: Scope, v: Variable, name: string) {
    if (name === v.name) return;
    if (v.secret) {
      const from = this.secretKey(scope, v.name);
      const to = this.secretKey(scope, name);
      await this.requireKeychain();
      await api.secretRename(from, to);
      this.secretStatus[to] = this.secretStatus[from] ?? false;
      delete this.secretStatus[from];
    }
    v.name = name;
    this.markDirty();
  }

  async removeVariable(scope: Scope, v: Variable) {
    if (v.secret) {
      await this.requireKeychain();
      await api.secretDelete([this.secretKey(scope, v.name)]);
    }
    const list = this.varsOf(scope);
    list.splice(list.indexOf(v), 1);
    this.markDirty();
  }

  addEnvironment(name: string) {
    const env: Environment = { id: uid(), name, variables: [] };
    this.project.environments.push(env);
    this.markDirty();
    return env;
  }

  async removeEnvironment(id: string) {
    const env = this.project.environments.find((e) => e.id === id);
    if (!env) return;
    if (!(await ask(`Delete environment "${env.name}" and its ${env.variables.length} variable(s)?`, { title: "Delete environment", kind: "warning" })))
      return false;
    const keys = env.variables.filter((v) => v.secret).map((v) => this.secretKey(id, v.name));
    if (keys.length) {
      await this.requireKeychain();
      await api.secretDelete(keys);
    }
    this.project.environments = this.project.environments.filter((e) => e.id !== id);
    if (this.project.activeEnvironment === id) this.project.activeEnvironment = null;
    this.markDirty();
    return true;
  }

  setActiveEnvironment(id: string | null) {
    this.project.activeEnvironment = id;
    this.markDirty();
  }

  /** Store `value` as secret variable `name` in `scope` (creating or updating it). */
  async storeSecretVariable(scope: Scope, name: string, value: string) {
    await this.requireKeychain(); // before touching the project, so Cancel changes nothing
    const list = this.varsOf(scope);
    let v = list.find((x) => x.name === name);
    if (!v) {
      list.push({ name, value: "", secret: true });
      v = list[list.length - 1];
    } else if (!v.secret) {
      v.secret = true;
      v.value = "";
    }
    await this.setSecretValue(scope, name, value);
    this.markDirty();
  }

  // ---- session autosave ----

  async restoreSession() {
    try {
      this.secretBackend = await api.secretBackend();
      const s = await api.sessionLoad();
      if (s) {
        this.project = s.project;
        this.path = s.path;
        this.dirty = s.dirty;
        const ids = new Set(s.project.requests.map((r) => r.id));
        this.openTabs = s.openTabs.filter((id) => ids.has(id));
        this.activeId = s.activeId && ids.has(s.activeId) ? s.activeId : (this.openTabs[0] ?? null);
      }
    } catch (e) {
      await message(String(e), { title: "Session not restored", kind: "warning" });
    } finally {
      this.restored = true;
    }
  }

  scheduleSave() {
    if (!this.restored) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.saveSessionNow(), AUTOSAVE_MS);
  }

  async saveSessionNow() {
    if (!this.restored) return;
    clearTimeout(this.saveTimer);
    try {
      await api.sessionSave({
        project: $state.snapshot(this.project),
        path: this.path,
        dirty: this.dirty,
        openTabs: $state.snapshot(this.openTabs),
        activeId: this.activeId,
      });
    } catch (e) {
      console.error("autosave failed", e);
    }
  }
}

export const app = new AppState();
