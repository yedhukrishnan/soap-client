// Mirrors the serde types in crates/soap-core.

export type SoapVersion = "1.1" | "1.2";

export interface Operation {
  name: string;
  soapAction: string | null;
  style: "document" | "rpc";
  documentation: string | null;
  sampleRequest: string;
}

export interface Interface {
  name: string;
  soapVersion: SoapVersion;
  endpoints: string[];
  operations: Operation[];
}

export interface WsdlDocument {
  targetNamespace: string;
  interfaces: Interface[];
}

export interface WsdlEntry {
  id: string;
  source: string;
  text: string;
  /** Documents pulled in via wsdl:import / xsd:import / xsd:include. */
  imports?: { location: string; text: string }[];
  /** Display names for interfaces (binding name -> label); requests keep the real name. */
  interfaceLabels?: Record<string, string>;
  document?: WsdlDocument;
}

export interface HeaderEntry {
  name: string;
  value: string;
  enabled: boolean;
}

export type PasswordType = "PasswordText" | "PasswordDigest";

export type Auth =
  | { kind: "none" }
  | { kind: "basic"; username: string; password: string }
  | {
      kind: "wsSecurity";
      username: string;
      password: string;
      passwordType: PasswordType;
      timestampTtlSecs: number;
    };

export interface RequestSpec {
  endpoint: string;
  soapVersion: SoapVersion;
  soapAction: string | null;
  body: string;
  headers: HeaderEntry[];
  auth: Auth;
  timeoutSecs: number;
  acceptInvalidCerts: boolean;
}

export type Check =
  | { type: "statusCode"; code: number }
  | { type: "contains"; text: string; ignoreCase: boolean }
  | { type: "notContains"; text: string; ignoreCase: boolean }
  | { type: "xpath"; expression: string; expected: string }
  | { type: "noSoapFault" }
  | { type: "maxResponseTime"; ms: number };

export interface Assertion {
  id: string;
  enabled: boolean;
  check: Check;
}

export interface AssertionResult {
  id: string;
  passed: boolean;
  message: string;
}

export interface SavedRequest {
  id: string;
  name: string;
  wsdlId: string;
  interfaceName: string;
  operationName: string;
  spec: RequestSpec;
  assertions: Assertion[];
}

export interface Variable {
  name: string;
  /** Always "" for secrets: their value lives in the OS keychain. */
  value: string;
  secret: boolean;
}

export interface Environment {
  id: string;
  name: string;
  variables: Variable[];
}

export interface Project {
  version: number;
  /** Stable id; namespaces this project's secrets in the keychain. */
  id: string;
  name: string;
  variables: Variable[];
  environments: Environment[];
  activeEnvironment: string | null;
  wsdls: WsdlEntry[];
  requests: SavedRequest[];
}

/** A resolved variable sent with a request: a plain value or a keychain key. */
export interface VarBinding {
  name: string;
  value?: string;
  secretKey?: string;
}

export interface Session {
  project: Project;
  path: string | null;
  dirty: boolean;
  openTabs: string[];
  activeId: string | null;
}

export interface SecretBackend {
  persistent: boolean;
  /** macOS asks for Touch ID (or the Mac password) to read secrets. */
  biometric: boolean;
  name: string;
}

export interface PreparedRequest {
  url: string;
  headers: [string, string][];
  body: string;
}

export interface ResponseData {
  status: number;
  statusText: string;
  headers: [string, string][];
  body: string;
  bodyPretty: string | null;
  elapsedMs: number;
  sizeBytes: number;
  request: PreparedRequest;
}
