export type LoginResponse = {
  token: string;
  admin: {
    username: string;
    session: {
      issued_at_unix: number;
      expires_at_unix: number;
      client_ip: string;
    };
  };
};

export type NodeSummary = {
  id: string;
  name: string;
  address: string;
  port: number;
  api_port: number;
  usage_coefficient: number;
  enabled: boolean;
  status: string;
  sync_status: string;
  provisioning_status: string;
  xray_version: string | null;
  node_version: string | null;
  last_heartbeat_at_unix: number | null;
  last_applied_revision: string | null;
};

export type NodeDiagnostics = {
  node: NodeSummary;
  local_health?: {
    status: string;
    node_id?: string | null;
    applied_revision?: string | null;
    consecutive_tick_failures: number;
  } | null;
  local_state?: {
    applied_revision?: string | null;
    last_error?: string | null;
    xray_detected_version?: string | null;
    rollback_marker_path?: string | null;
    xray_runtime_status: string;
    xray_last_detail?: string | null;
    xray_restart_attempts: number;
    xray_next_restart_not_before_unix?: number | null;
    runtime_events: Array<{ kind: string; detail: string; created_at_unix: number }>;
    apply_history: Array<{ revision?: string | null; status: string; detail: string; created_at_unix: number }>;
  } | null;
  recommendations: string[];
};

export type ProvisioningPreflight = {
  node_id: string;
  passed: boolean;
  checked_at_unix: number;
  checks: Array<{ check: string; status: string; detail: string }>;
  recommendations: string[];
};

export type BootstrapReadiness = {
  node: NodeSummary;
  ready: boolean;
  checked_at_unix: number;
  failed_steps: string[];
  recommendations: string[];
};

export type ProvisioningTask = {
  task_id: string;
  node_id: string;
  status: string;
  created_at_unix: number;
  started_at_unix?: number | null;
  finished_at_unix?: number | null;
  updated_at_unix: number;
  verify_after_finish: boolean;
  verified_ready?: boolean | null;
  verify_probe_id?: string | null;
  recommendations: string[];
  failures: Array<{ step: string; category: string; detail: string }>;
  remediation: Array<{ action: string; detail: string }>;
  request_context: {
    transport: string;
    target_host?: string | null;
    ssh_port?: number | null;
    ssh_username?: string | null;
    uses_password_auth: boolean;
    uses_private_key_auth: boolean;
  };
  planned_steps: string[];
  steps: Array<{
    step: string;
    status: string;
    detail: string;
    failure_category?: string | null;
    created_at_unix: number;
  }>;
};

export type NodeActionResponse = { detail: string };

export type SecuritySettings = {
  login_protection_enabled: boolean;
  smart_ban_enabled: boolean;
  trust_x_forwarded_for: boolean;
  trusted_proxy_ips: string[];
  trusted_proxy_cidrs: string[];
  max_failed_attempts: number;
  attempt_window_seconds: number;
  block_for_seconds: number;
  session_ttl_seconds: number;
};

export type TwoFactorState = {
  enabled: boolean;
  two_step_enabled: boolean;
  configured: boolean;
  confirmed_at_unix?: number | null;
};

export type TwoFactorSetupResponse = {
  secret_base32: string;
  otpauth_url: string;
  state: TwoFactorState;
};

export type ActiveBan = {
  client_ip: string;
  ban_kind: string;
  blocked_until_unix: number;
};

export type SecurityAuditEvent = {
  event_type: string;
  username?: string | null;
  client_ip?: string | null;
  created_at_unix: number;
  detail: string;
};

export type SystemOverview = {
  memory_budget_mb: number;
  memory_used_bytes: number;
  memory_total_bytes: number;
  disk: {
    total_bytes: number;
    used_bytes: number;
    free_bytes: number;
  };
  operational_log_lines_buffered: number;
  core_status: string;
  active_alerts: Array<{
    kind: string;
    severity: string;
    message: string;
    observed_percent: number;
    threshold_percent: number;
    first_seen_at_unix: number;
  }>;
};

export type SystemThresholds = {
  disk_warning_percent: number;
  disk_critical_percent: number;
  memory_warning_percent: number;
  memory_critical_percent: number;
};

export type AlertEvent = {
  kind: string;
  severity: string;
  status: string;
  observed_percent: number;
  threshold_percent: number;
  created_at_unix: number;
  message: string;
};

export type CoreConfigState = {
  config: string;
  saved_at_unix?: number | null;
  valid_json: boolean;
};

export type CoreRuntimeState = {
  status: string;
  last_action?: {
    action: string;
    created_at_unix: number;
  } | null;
  applied_revision?: string | null;
};

export type CoreApplyRecord = {
  revision: string;
  created_at_unix: number;
  result: string;
  detail: string;
};

export type UserStatus = "active" | "disabled" | "expired" | "on_hold";

export type User = {
  username: string;
  status: UserStatus;
  data_limit_bytes?: number | null;
  used_traffic_bytes: number;
  expire_at_unix?: number | null;
  note?: string | null;
  template_id?: string | null;
  next_template_id?: string | null;
  proxy_profile_ids: string[];
  excluded_inbound_tags: string[];
  subscription_token: string;
  sub_revoked_at_unix?: number | null;
  created_at_unix: number;
  updated_at_unix: number;
};

export type UserTemplate = {
  id: string;
  name: string;
  status: UserStatus;
  data_limit_bytes?: number | null;
  expire_duration_seconds?: number | null;
  note?: string | null;
  proxy_profile_ids: string[];
  excluded_inbound_tags: string[];
  created_at_unix: number;
  updated_at_unix: number;
};

export type Inbound = {
  tag: string;
  port: number;
  protocol: string;
  network: "tcp" | "ws" | "grpc" | "http_upgrade" | "quic";
  tls_enabled: boolean;
  created_at_unix: number;
  updated_at_unix: number;
};

export type Host = {
  id: string;
  remark: string;
  address: string;
  port: number;
  path?: string | null;
  sni?: string | null;
  security: "none" | "tls" | "reality";
  created_at_unix: number;
  updated_at_unix: number;
};

export type ProxyProfile = {
  id: string;
  name: string;
  proxy_type: "vmess" | "vless" | "trojan" | "shadowsocks";
  settings_json: string;
  excluded_inbound_tags: string[];
  created_at_unix: number;
  updated_at_unix: number;
};

export type UserSubscriptionView = {
  username: string;
  subscription_token: string;
  subscription_path: string;
  revoked_at_unix?: number | null;
};

export type UserActivityEntry = {
  username: string;
  kind: string;
  actor_username?: string | null;
  detail: string;
  traffic_delta_bytes?: number | null;
  total_used_traffic_bytes?: number | null;
  created_at_unix: number;
};

export class ApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

type RequestOptions = {
  method?: string;
  token?: string | null;
  body?: unknown;
};

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers = new Headers();
  headers.set("Content-Type", "application/json");
  if (options.token) headers.set("Authorization", `Bearer ${options.token}`);

  const response = await fetch(path, {
    method: options.method ?? "GET",
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });

  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const payload = await response.json();
      if (typeof payload?.error === "string") message = payload.error;
      else if (typeof payload?.reason === "string") message = payload.reason;
    } catch {
      // ignore
    }
    throw new ApiError(response.status, message);
  }

  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export const api = {
  login(username: string, password: string, twoFactorCode?: string) {
    return request<LoginResponse>("/api/admin/login", {
      method: "POST",
      body: {
        username,
        password,
        two_factor_code: twoFactorCode || undefined,
      },
    });
  },
  systemOverview(token: string) {
    return request<SystemOverview>("/api/system/overview", { token });
  },
  systemThresholds(token: string) {
    return request<SystemThresholds>("/api/system/thresholds", { token });
  },
  updateSystemThresholds(token: string, body: SystemThresholds) {
    return request<SystemThresholds>("/api/system/thresholds", {
      method: "PUT",
      token,
      body,
    });
  },
  systemAlerts(token: string) {
    return request<AlertEvent[]>("/api/system/alerts", { token });
  },
  systemAlertHistory(token: string) {
    return request<AlertEvent[]>("/api/system/alerts/history", { token });
  },
  coreConfig(token: string) {
    return request<CoreConfigState>("/api/core/config", { token });
  },
  saveCoreConfig(token: string, config: string) {
    return request<CoreConfigState>("/api/core/config", {
      method: "PUT",
      token,
      body: { config },
    });
  },
  coreState(token: string) {
    return request<CoreRuntimeState>("/api/core/state", { token });
  },
  coreAction(token: string, action: "start" | "stop" | "restart") {
    return request<CoreRuntimeState>("/api/core/actions", {
      method: "POST",
      token,
      body: { action },
    });
  },
  applyGeneratedCoreConfig(token: string) {
    return request<unknown>("/api/core/apply-generated", {
      method: "POST",
      token,
      body: {},
    });
  },
  coreApplyHistory(token: string) {
    return request<CoreApplyRecord[]>("/api/core/apply-history", { token });
  },
  securitySettings(token: string) {
    return request<SecuritySettings>("/api/admin/security/settings", { token });
  },
  updateSecuritySettings(token: string, body: SecuritySettings) {
    return request<SecuritySettings>("/api/admin/security/settings", {
      method: "PUT",
      token,
      body,
    });
  },
  twoFactorState(token: string) {
    return request<TwoFactorState>("/api/admin/2fa/state", { token });
  },
  setupTwoFactor(token: string) {
    return request<TwoFactorSetupResponse>("/api/admin/2fa/setup", {
      method: "POST",
      token,
    });
  },
  enableTwoFactor(token: string, code: string, twoStepEnabled: boolean) {
    return request<TwoFactorSetupResponse>("/api/admin/2fa/enable", {
      method: "POST",
      token,
      body: { code, two_step_enabled: twoStepEnabled },
    });
  },
  disableTwoFactor(token: string, code: string) {
    return request<TwoFactorSetupResponse>("/api/admin/2fa/disable", {
      method: "POST",
      token,
      body: { code },
    });
  },
  updateTwoFactorTwoStep(token: string, enabled: boolean) {
    return request<TwoFactorSetupResponse>("/api/admin/2fa/two-step", {
      method: "POST",
      token,
      body: { enabled },
    });
  },
  activeBans(token: string) {
    return request<ActiveBan[]>("/api/admin/security/bans", { token });
  },
  createBan(token: string, clientIp: string, banKind: "temporary" | "permanent", durationSeconds?: number) {
    return request<ActiveBan>("/api/admin/security/bans", {
      method: "POST",
      token,
      body: {
        client_ip: clientIp,
        ban_kind: banKind,
        duration_seconds: durationSeconds,
      },
    });
  },
  removeBan(token: string, clientIp: string) {
    return request<void>(`/api/admin/security/bans/${encodeURIComponent(clientIp)}`, {
      method: "POST",
      token,
    });
  },
  securityAudit(token: string) {
    return request<SecurityAuditEvent[]>("/api/admin/security/audit", { token });
  },
  nodes(token: string) {
    return request<NodeSummary[]>("/api/nodes", { token });
  },
  nodeDiagnostics(token: string, nodeId: string) {
    return request<NodeDiagnostics>(`/api/nodes/${nodeId}/diagnostics`, { token });
  },
  nodeBootstrapReadiness(token: string, nodeId: string) {
    return request<BootstrapReadiness>(`/api/nodes/${nodeId}/bootstrap-readiness`, { token });
  },
  nodeBootstrapProbe(token: string, nodeId: string) {
    return request(`/api/nodes/${nodeId}/bootstrap-probe`, { method: "POST", token });
  },
  nodeProvisioning(token: string, nodeId: string) {
    return request<ProvisioningTask[]>(`/api/nodes/${nodeId}/provisioning`, { token });
  },
  nodeProvisioningPreflight(token: string, nodeId: string) {
    return request<ProvisioningPreflight>(`/api/nodes/${nodeId}/provisioning/preflight`, { token });
  },
  startNodeProvisioning(token: string, nodeId: string) {
    return request<ProvisioningTask>(`/api/nodes/${nodeId}/provisioning/start`, {
      method: "POST",
      token,
      body: { verify_after_finish: true },
    });
  },
  reprovisionNode(token: string, nodeId: string) {
    return request<ProvisioningTask>(`/api/nodes/${nodeId}/provisioning/reprovision`, {
      method: "POST",
      token,
      body: { verify_after_finish: true },
    });
  },
  retryNodeProvisioning(token: string, nodeId: string, taskId: string) {
    return request<ProvisioningTask>(`/api/nodes/${nodeId}/provisioning/${taskId}/retry`, {
      method: "POST",
      token,
      body: { verify_after_finish: true },
    });
  },
  nodeRuntimeAction(token: string, nodeId: string, action: string) {
    return request<NodeActionResponse>(`/api/nodes/${nodeId}/local/runtime/${action}`, {
      method: "POST",
      token,
    });
  },
  nodeXrayUpdate(token: string, nodeId: string) {
    return request<NodeActionResponse>(`/api/nodes/${nodeId}/local/xray/update`, {
      method: "POST",
      token,
    });
  },
  operationalLogs(token: string, limit = 50) {
    return request<Array<Record<string, unknown>>>(`/api/system/logs?limit=${limit}`, { token });
  },
  users(token: string) {
    return request<User[]>("/api/users", { token });
  },
  createUser(token: string, body: {
    username: string;
    status?: UserStatus;
    data_limit_bytes?: number | null;
    expire_at_unix?: number | null;
    note?: string | null;
    template_id?: string | null;
    next_template_id?: string | null;
    proxy_profile_ids?: string[];
    excluded_inbound_tags?: string[];
  }) {
    return request<User>("/api/users", { method: "POST", token, body });
  },
  updateUser(token: string, username: string, body: Partial<{
    status: UserStatus;
    data_limit_bytes: number | null;
    expire_at_unix: number | null;
    note: string | null;
    template_id: string | null;
    next_template_id: string | null;
    proxy_profile_ids: string[];
    excluded_inbound_tags: string[];
  }>) {
    return request<User>(`/api/users/${encodeURIComponent(username)}`, { method: "PUT", token, body });
  },
  deleteUser(token: string, username: string) {
    return request<void>(`/api/users/${encodeURIComponent(username)}`, { method: "DELETE", token });
  },
  resetUserUsage(token: string, username: string) {
    return request<User>(`/api/users/${encodeURIComponent(username)}/usage/reset`, { method: "POST", token });
  },
  revokeUserSubscription(token: string, username: string) {
    return request<UserSubscriptionView>(`/api/users/${encodeURIComponent(username)}/subscription/revoke`, { method: "POST", token });
  },
  userSubscription(token: string, username: string) {
    return request<UserSubscriptionView>(`/api/users/${encodeURIComponent(username)}/subscription`, { token });
  },
  userTemplates(token: string) {
    return request<UserTemplate[]>("/api/user-templates", { token });
  },
  inbounds(token: string) {
    return request<Inbound[]>("/api/inbounds", { token });
  },
  createInbound(token: string, body: Pick<Inbound, "tag" | "port" | "protocol" | "network" | "tls_enabled">) {
    return request<Inbound>("/api/inbounds", { method: "POST", token, body });
  },
  deleteInbound(token: string, tag: string) {
    return request<void>(`/api/inbounds/${encodeURIComponent(tag)}`, { method: "DELETE", token });
  },
  hosts(token: string) {
    return request<Host[]>("/api/hosts", { token });
  },
  createHost(token: string, body: Pick<Host, "remark" | "address" | "port" | "path" | "sni" | "security">) {
    return request<Host>("/api/hosts", { method: "POST", token, body });
  },
  deleteHost(token: string, hostId: string) {
    return request<void>(`/api/hosts/${encodeURIComponent(hostId)}`, { method: "DELETE", token });
  },
  proxyProfiles(token: string) {
    return request<ProxyProfile[]>("/api/proxy-profiles", { token });
  },
  createProxyProfile(token: string, body: Pick<ProxyProfile, "name" | "proxy_type" | "settings_json" | "excluded_inbound_tags">) {
    return request<ProxyProfile>("/api/proxy-profiles", { method: "POST", token, body });
  },
  deleteProxyProfile(token: string, profileId: string) {
    return request<void>(`/api/proxy-profiles/${encodeURIComponent(profileId)}`, { method: "DELETE", token });
  },
  usersActivity(token: string) {
    return request<UserActivityEntry[]>("/api/users/activity", { token });
  },
};
