// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";
import type {
  Assertion, AssertionResult, Project, RequestSpec, ResponseData, SecretBackend, Session, VarBinding, WsdlEntry,
} from "./types";

export const api = {
  importWsdl: (source: string) => invoke<WsdlEntry>("import_wsdl", { source }),
  sendRequest: (callId: string, spec: RequestSpec, vars: VarBinding[]) =>
    invoke<ResponseData>("send_request", { callId, spec, vars }),
  cancelRequest: (callId: string) => invoke<void>("cancel_request", { callId }),
  runAssertions: (assertions: Assertion[], response: ResponseData) =>
    invoke<AssertionResult[]>("run_assertions", { assertions, response }),
  formatXml: (xml: string) => invoke<string | null>("format_xml", { xml }),
  saveProject: (path: string, project: Project) => invoke<void>("save_project", { path, project }),
  openProject: (path: string) => invoke<Project>("open_project", { path }),

  sessionLoad: () => invoke<Session | null>("session_load"),
  sessionSave: (session: Session) => invoke<void>("session_save", { session }),

  secretBackend: () => invoke<SecretBackend>("secret_backend"),
  secretSet: (key: string, value: string) => invoke<void>("secret_set", { key, value }),
  secretDelete: (keys: string[]) => invoke<void>("secret_delete", { keys }),
  secretRename: (from: string, to: string) => invoke<void>("secret_rename", { from, to }),
  secretStatus: (keys: string[]) => invoke<boolean[]>("secret_status", { keys }),
};
