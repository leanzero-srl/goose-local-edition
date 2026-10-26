export const CONTROLLER_SCOPES = [
  "catalog.read", "history.read", "work.send", "work.steer", "work.stop", "input.answer",
  "resources.read", "models.read", "models.download", "models.manage", "providers.read",
  "providers.manage", "devices.read", "devices.manage",
] as const;
export type ControllerScope = typeof CONTROLLER_SCOPES[number];
export type Platform = "ios" | "ipados" | "desktop";
export interface DeviceRecord {
  device_id: string; name: string; platform: Platform; role: "controller"; revision: number;
  scopes: ControllerScope[]; created_at: string; revoked_at: string | null;
}
export interface DeviceCredentials {
  access_token: string; token_type: "Bearer"; expires_at: string; refresh_token: string; device: DeviceRecord;
}
export interface AccessClaims {
  ver: 2; token_use: "controller"; iss: "leanzero-link-auth"; aud: "leanzero-link/controller";
  sub: string; account_key: string; device_id: string; device_revision: number;
  jti: string; iat: number; exp: number;
}
export interface DeviceState {
  record: DeviceRecord; refresh_hash: string; refresh_expires_at: number;
  used_refreshes: Record<string, { request_id: string; receipt: string; revision: number }>;
}
export interface Challenge {
  challenge_id: string; enrollment_id: string; hash: string; attempts: number; expires_at: number;
  status: "sending" | "ready" | "consumed" | "failed";
  request_hash?: string; receipt?: string; device_id?: string;
}
export interface HostState {
  owner_node_id: string; host_name: string; protocol_version: 1; revision: number;
  credential_hash: string; registration_id: string; registration_receipt: string;
  tls_spki_sha256: string | null;
  controller_port?: number | null;
  lease: { runtime_id: string; lease_id: string; expires_at: number; renewed_at?: number; store_generation: string;
    catalog_revision: number; mesh_node_key: string } | null;
  generation: string | null; catalog_high_water: number;
}
export interface AccountState {
  legacy_mesh_mints?: { in_flight: number; expires_at: number };
  mesh_enrollments?: Record<string, MeshEnrollment>;
  generation_recoveries?: Record<string, { digest: string; host_revision: number; store_generation: string }>;
  schema: 1; account_id: string; email: string;
  challenges: Record<string, Challenge>; devices: Record<string, DeviceState>; hosts: Record<string, HostState>;
  rate_windows: Record<string, { start: number; count: number }>;
  revocations: Record<string, { request_id: string; expected_revision: number; revision: number; revoked_at: string }>;
}
export interface AuthorizationLease {
  active: true; account_id: string; device_id: string; device_revision: number; role: "controller";
  owner_node_id: string; scopes: ControllerScope[]; token_expires_at: string; lease_seconds: number;
  host_revision: number; store_generation: string; host_lease_id: string;
}

export interface MeshBinding {
  node_id: string; node_key: string; user_id: string; key_id: string;
}
export interface MeshEnrollment {
  enrollment_id: string; role: "host" | "controller"; subject_id: string;
  status: "pending" | "issued" | "confirmed" | "revoked";
  created_at: number; expires_at: number; user_id: string;
  key_id: string | null; receipt: string | null; binding: MeshBinding | null;
  cleanup_pending: boolean;
}
export interface MeshJoinCredential {
  enrollment_id: string; auth_key: string; login_server: string; expiry_seconds: number;
}
export interface RegisteredPeers {
  protocol_version: 1; revision: number; lease_seconds: number;
  peers: { mesh_node_key: string; role: "host" | "controller"; owner_node_id: string | null }[];
}
