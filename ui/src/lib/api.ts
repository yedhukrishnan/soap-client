// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";
import type { Assertion, AssertionResult, Project, RequestSpec, ResponseData, WsdlEntry } from "./types";

export const api = {
  importWsdl: (source: string) => invoke<WsdlEntry>("import_wsdl", { source }),
  sendRequest: (callId: string, spec: RequestSpec) => invoke<ResponseData>("send_request", { callId, spec }),
  cancelRequest: (callId: string) => invoke<void>("cancel_request", { callId }),
  runAssertions: (assertions: Assertion[], response: ResponseData) =>
    invoke<AssertionResult[]>("run_assertions", { assertions, response }),
  formatXml: (xml: string) => invoke<string | null>("format_xml", { xml }),
  saveProject: (path: string, project: Project) => invoke<void>("save_project", { path, project }),
  openProject: (path: string) => invoke<Project>("open_project", { path }),
};
