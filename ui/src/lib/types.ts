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

export interface Project {
  version: number;
  name: string;
  wsdls: WsdlEntry[];
  requests: SavedRequest[];
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
