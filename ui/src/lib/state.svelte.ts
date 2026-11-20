// Application state (Svelte 5 runes). The project is owned by the frontend;
// the Rust backend is stateless apart from in-flight request cancellation.
import { ask, message, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { disposeModel } from "./monaco";
import type {
  AssertionResult, Interface, Operation, Project, ResponseData, SavedRequest, WsdlEntry,
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

const emptyProject = (): Project => ({ version: 1, name: "Untitled", wsdls: [], requests: [] });

export const uid = () => crypto.randomUUID();

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

  active = $derived(this.project.requests.find((r) => r.id === this.activeId) ?? null);

  markDirty() {
    this.dirty = true;
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
    this.runs[id] = { ...this.runs[id], sending: true, callId, error: undefined };
    try {
      const response = await api.sendRequest(callId, $state.snapshot(req.spec));
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
}

export const app = new AppState();
