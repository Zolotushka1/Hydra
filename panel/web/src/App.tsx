import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import {
  ApiError,
  api,
  type ActiveBan,
  type AlertEvent,
  type BootstrapReadiness,
  type CoreApplyRecord,
  type CoreConfigState,
  type CoreRuntimeState,
  type NodeDiagnostics,
  type NodeSummary,
  type ProvisioningPreflight,
  type ProvisioningTask,
  type SecurityAuditEvent,
  type SecuritySettings,
  type SystemOverview,
  type SystemThresholds,
  type TwoFactorSetupResponse,
  type TwoFactorState,
  type Host,
  type Inbound,
  type ProxyProfile,
  type User,
  type UserActivityEntry,
  type UserStatus,
  type UserTemplate,
} from "./lib/api";

type Section = "overview" | "security" | "nodes" | "users" | "logs";

const formatUnix = (value?: number | null) => (value ? new Date(value * 1000).toLocaleString() : "n/a");

function formatBytes(value?: number | null) {
  if (!value || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(size >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function badgeClass(value?: string | null) {
  const normalized = (value ?? "").toLowerCase();
  if (["active", "applied", "completed", "enabled", "healthy", "passed", "ready", "running", "success"].includes(normalized)) return "good";
  if (["degraded", "pending", "stopped", "warning"].includes(normalized)) return "warn";
  if (["blocked", "critical", "drifted", "error", "failed", "offline", "permanent"].includes(normalized)) return "danger";
  return "muted";
}

function App() {
  const [token, setToken] = createSignal<string | null>(null);
  const [section, setSection] = createSignal<Section>("overview");
  const [username, setUsername] = createSignal("admin");
  const [password, setPassword] = createSignal("admin123");
  const [twoFactorCode, setTwoFactorCode] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [message, setMessage] = createSignal<{ kind: "success" | "error"; text: string } | null>(null);

  const [overview, setOverview] = createSignal<SystemOverview | null>(null);
  const [thresholds, setThresholds] = createSignal<SystemThresholds | null>(null);
  const [alerts, setAlerts] = createSignal<AlertEvent[]>([]);
  const [alertHistory, setAlertHistory] = createSignal<AlertEvent[]>([]);
  const [coreConfig, setCoreConfig] = createSignal<CoreConfigState | null>(null);
  const [coreDraft, setCoreDraft] = createSignal("");
  const [coreState, setCoreState] = createSignal<CoreRuntimeState | null>(null);
  const [coreApplyHistory, setCoreApplyHistory] = createSignal<CoreApplyRecord[]>([]);

  const [securitySettings, setSecuritySettings] = createSignal<SecuritySettings | null>(null);
  const [trustedIpsDraft, setTrustedIpsDraft] = createSignal("");
  const [trustedCidrsDraft, setTrustedCidrsDraft] = createSignal("");
  const [twoFactorState, setTwoFactorState] = createSignal<TwoFactorState | null>(null);
  const [twoFactorSetup, setTwoFactorSetup] = createSignal<TwoFactorSetupResponse | null>(null);
  const [verificationCode, setVerificationCode] = createSignal("");
  const [disableCode, setDisableCode] = createSignal("");
  const [activeBans, setActiveBans] = createSignal<ActiveBan[]>([]);
  const [banIp, setBanIp] = createSignal("");
  const [banKind, setBanKind] = createSignal<"temporary" | "permanent">("temporary");
  const [banDuration, setBanDuration] = createSignal("300");
  const [auditEvents, setAuditEvents] = createSignal<SecurityAuditEvent[]>([]);

  const [nodes, setNodes] = createSignal<NodeSummary[]>([]);
  const [selectedNodeId, setSelectedNodeId] = createSignal<string | null>(null);
  const [nodeDiagnostics, setNodeDiagnostics] = createSignal<NodeDiagnostics | null>(null);
  const [bootstrapReadiness, setBootstrapReadiness] = createSignal<BootstrapReadiness | null>(null);
  const [preflight, setPreflight] = createSignal<ProvisioningPreflight | null>(null);
  const [provisioningTasks, setProvisioningTasks] = createSignal<ProvisioningTask[]>([]);

  const [users, setUsers] = createSignal<User[]>([]);
  const [templates, setTemplates] = createSignal<UserTemplate[]>([]);
  const [inbounds, setInbounds] = createSignal<Inbound[]>([]);
  const [hosts, setHosts] = createSignal<Host[]>([]);
  const [proxyProfiles, setProxyProfiles] = createSignal<ProxyProfile[]>([]);
  const [userActivity, setUserActivity] = createSignal<UserActivityEntry[]>([]);
  const [newUsername, setNewUsername] = createSignal("");
  const [newUserStatus, setNewUserStatus] = createSignal<UserStatus>("active");
  const [newUserDataLimit, setNewUserDataLimit] = createSignal("");
  const [newUserNote, setNewUserNote] = createSignal("");
  const [newInboundTag, setNewInboundTag] = createSignal("");
  const [newInboundPort, setNewInboundPort] = createSignal("443");
  const [newHostRemark, setNewHostRemark] = createSignal("");
  const [newHostAddress, setNewHostAddress] = createSignal("");
  const [newHostPort, setNewHostPort] = createSignal("443");
  const [newProxyName, setNewProxyName] = createSignal("");
  const [newProxySettings, setNewProxySettings] = createSignal("{}");

  const [logs, setLogs] = createSignal<Array<Record<string, unknown>>>([]);
  const selectedNode = createMemo(() => nodes().find((node) => node.id === selectedNodeId()) ?? null);

  function reportError(error: unknown) {
    if (error instanceof ApiError) setMessage({ kind: "error", text: error.message });
    else setMessage({ kind: "error", text: error instanceof Error ? error.message : String(error) });
  }

  async function run<T>(action: () => Promise<T>, success?: string) {
    setBusy(true);
    setMessage(null);
    try {
      const result = await action();
      if (success) setMessage({ kind: "success", text: success });
      return result;
    } catch (error) {
      reportError(error);
      return undefined;
    } finally {
      setBusy(false);
    }
  }

  async function loadOverview(currentToken = token()) {
    if (!currentToken) return;
    const [nextOverview, nextThresholds, nextAlerts, nextAlertHistory, nextCoreConfig, nextCoreState, nextApplyHistory] =
      await Promise.all([
        api.systemOverview(currentToken),
        api.systemThresholds(currentToken),
        api.systemAlerts(currentToken),
        api.systemAlertHistory(currentToken),
        api.coreConfig(currentToken),
        api.coreState(currentToken),
        api.coreApplyHistory(currentToken),
      ]);
    setOverview(nextOverview);
    setThresholds(nextThresholds);
    setAlerts(nextAlerts);
    setAlertHistory(nextAlertHistory);
    setCoreConfig(nextCoreConfig);
    setCoreDraft(nextCoreConfig.config);
    setCoreState(nextCoreState);
    setCoreApplyHistory(nextApplyHistory);
  }

  async function loadSecurity(currentToken = token()) {
    if (!currentToken) return;
    const [settings, twoFactor, bans, audit] = await Promise.all([
      api.securitySettings(currentToken),
      api.twoFactorState(currentToken),
      api.activeBans(currentToken),
      api.securityAudit(currentToken),
    ]);
    setSecuritySettings(settings);
    setTrustedIpsDraft(settings.trusted_proxy_ips.join(", "));
    setTrustedCidrsDraft(settings.trusted_proxy_cidrs.join(", "));
    setTwoFactorState(twoFactor);
    setActiveBans(bans);
    setAuditEvents(audit);
  }

  async function loadNodes(currentToken = token()) {
    if (!currentToken) return;
    const nextNodes = await api.nodes(currentToken);
    setNodes(nextNodes);
    if (!selectedNodeId() && nextNodes.length > 0) setSelectedNodeId(nextNodes[0].id);
  }

  async function loadSelectedNode(currentToken = token(), nodeId = selectedNodeId()) {
    if (!currentToken || !nodeId) return;
    const [diagnostics, readiness, nextPreflight, tasks] = await Promise.all([
      api.nodeDiagnostics(currentToken, nodeId),
      api.nodeBootstrapReadiness(currentToken, nodeId),
      api.nodeProvisioningPreflight(currentToken, nodeId),
      api.nodeProvisioning(currentToken, nodeId),
    ]);
    setNodeDiagnostics(diagnostics);
    setBootstrapReadiness(readiness);
    setPreflight(nextPreflight);
    setProvisioningTasks(tasks);
  }

  async function loadLogs(currentToken = token()) {
    if (!currentToken) return;
    setLogs(await api.operationalLogs(currentToken, 100));
  }

  async function loadUsers(currentToken = token()) {
    if (!currentToken) return;
    const [nextUsers, nextTemplates, nextInbounds, nextHosts, nextProfiles, nextActivity] = await Promise.all([
      api.users(currentToken),
      api.userTemplates(currentToken),
      api.inbounds(currentToken),
      api.hosts(currentToken),
      api.proxyProfiles(currentToken),
      api.usersActivity(currentToken),
    ]);
    setUsers(nextUsers);
    setTemplates(nextTemplates);
    setInbounds(nextInbounds);
    setHosts(nextHosts);
    setProxyProfiles(nextProfiles);
    setUserActivity(nextActivity);
  }

  async function refreshAll(currentToken = token()) {
    await Promise.all([loadOverview(currentToken), loadSecurity(currentToken), loadNodes(currentToken), loadUsers(currentToken), loadLogs(currentToken)]);
    await loadSelectedNode(currentToken);
  }

  createEffect(() => {
    const currentToken = token();
    const nodeId = selectedNodeId();
    if (currentToken && nodeId) loadSelectedNode(currentToken, nodeId).catch(reportError);
  });

  async function handleLogin(event: SubmitEvent) {
    event.preventDefault();
    const response = await run(() => api.login(username(), password(), twoFactorCode() || undefined));
    if (!response) return;
    setToken(response.token);
    setTwoFactorCode("");
    await run(() => refreshAll(response.token), `Logged in as ${response.admin.username}`);
  }

  async function saveThresholds(event: SubmitEvent) {
    event.preventDefault();
    const current = thresholds();
    const currentToken = token();
    if (!current || !currentToken) return;
    const saved = await run(() => api.updateSystemThresholds(currentToken, current), "Thresholds saved");
    if (saved) setThresholds(saved);
    await run(() => loadOverview(currentToken));
  }

  async function saveCoreConfig() {
    const currentToken = token();
    if (!currentToken) return;
    const saved = await run(() => api.saveCoreConfig(currentToken, coreDraft()), "Core config saved");
    if (saved) setCoreConfig(saved);
    await run(() => loadOverview(currentToken));
  }

  async function coreAction(action: "start" | "stop" | "restart") {
    const currentToken = token();
    if (!currentToken) return;
    const state = await run(() => api.coreAction(currentToken, action), `Core ${action} requested`);
    if (state) setCoreState(state);
    await run(() => loadOverview(currentToken));
  }

  async function applyGeneratedConfig() {
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.applyGeneratedCoreConfig(currentToken), "Generated config applied");
    await run(() => loadOverview(currentToken));
  }

  async function saveSecurity(event: SubmitEvent) {
    event.preventDefault();
    const current = securitySettings();
    const currentToken = token();
    if (!current || !currentToken) return;
    const payload = {
      ...current,
      trusted_proxy_ips: trustedIpsDraft().split(",").map((item) => item.trim()).filter(Boolean),
      trusted_proxy_cidrs: trustedCidrsDraft().split(",").map((item) => item.trim()).filter(Boolean),
    };
    const saved = await run(() => api.updateSecuritySettings(currentToken, payload), "Security settings saved");
    if (saved) setSecuritySettings(saved);
    await run(() => loadSecurity(currentToken));
  }

  async function generateTwoFactor() {
    const currentToken = token();
    if (!currentToken) return;
    const setup = await run(() => api.setupTwoFactor(currentToken), "2FA secret generated");
    if (setup) {
      setTwoFactorSetup(setup);
      setTwoFactorState(setup.state);
    }
  }

  async function enableTwoFactor() {
    const currentToken = token();
    const state = twoFactorState();
    if (!currentToken || !state) return;
    const setup = await run(() => api.enableTwoFactor(currentToken, verificationCode(), state.two_step_enabled), "2FA enabled");
    if (setup) {
      setTwoFactorSetup(setup);
      setTwoFactorState(setup.state);
      setVerificationCode("");
    }
  }

  async function disableTwoFactor() {
    const currentToken = token();
    if (!currentToken) return;
    const setup = await run(() => api.disableTwoFactor(currentToken, disableCode()), "2FA disabled");
    if (setup) {
      setTwoFactorSetup(setup);
      setTwoFactorState(setup.state);
      setDisableCode("");
    }
  }

  async function toggleTwoStep(enabled: boolean) {
    const currentToken = token();
    if (!currentToken) return;
    const setup = await run(() => api.updateTwoFactorTwoStep(currentToken, enabled), "2FA mode updated");
    if (setup) {
      setTwoFactorSetup(setup);
      setTwoFactorState(setup.state);
    }
  }

  async function createBan(event: SubmitEvent) {
    event.preventDefault();
    const currentToken = token();
    if (!currentToken) return;
    await run(
      () => api.createBan(currentToken, banIp(), banKind(), banKind() === "temporary" ? Number(banDuration()) : undefined),
      "Ban created",
    );
    setBanIp("");
    await run(() => loadSecurity(currentToken));
  }

  async function removeBan(clientIp: string) {
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.removeBan(currentToken, clientIp), "Ban removed");
    await run(() => loadSecurity(currentToken));
  }

  async function nodeAction(action: "start-provisioning" | "reprovision" | "bootstrap-probe" | "restart" | "rollback" | "xray-update") {
    const currentToken = token();
    const nodeId = selectedNodeId();
    if (!currentToken || !nodeId) return;
    const operations = {
      "start-provisioning": () => api.startNodeProvisioning(currentToken, nodeId),
      reprovision: () => api.reprovisionNode(currentToken, nodeId),
      "bootstrap-probe": () => api.nodeBootstrapProbe(currentToken, nodeId),
      restart: () => api.nodeRuntimeAction(currentToken, nodeId, "restart"),
      rollback: () => api.nodeRuntimeAction(currentToken, nodeId, "rollback"),
      "xray-update": () => api.nodeXrayUpdate(currentToken, nodeId),
    };
    await run(operations[action], `Node action completed: ${action}`);
    await run(() => loadNodes(currentToken));
    await run(() => loadSelectedNode(currentToken, nodeId));
  }

  async function retryTask(taskId: string) {
    const currentToken = token();
    const nodeId = selectedNodeId();
    if (!currentToken || !nodeId) return;
    await run(() => api.retryNodeProvisioning(currentToken, nodeId, taskId), "Provisioning retry queued");
    await run(() => loadSelectedNode(currentToken, nodeId));
  }

  async function createUser(event: SubmitEvent) {
    event.preventDefault();
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.createUser(currentToken, {
      username: newUsername(),
      status: newUserStatus(),
      data_limit_bytes: newUserDataLimit() ? Number(newUserDataLimit()) : null,
      note: newUserNote() || null,
    }), "User created");
    setNewUsername("");
    setNewUserDataLimit("");
    setNewUserNote("");
    await run(() => loadUsers(currentToken));
  }

  async function updateUserStatus(username: string, status: UserStatus) {
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.updateUser(currentToken, username, { status }), "User updated");
    await run(() => loadUsers(currentToken));
  }

  async function deleteUser(username: string) {
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.deleteUser(currentToken, username), "User deleted");
    await run(() => loadUsers(currentToken));
  }

  async function resetUsage(username: string) {
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.resetUserUsage(currentToken, username), "Usage reset");
    await run(() => loadUsers(currentToken));
  }

  async function revokeSubscription(username: string) {
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.revokeUserSubscription(currentToken, username), "Subscription revoked");
    await run(() => loadUsers(currentToken));
  }

  async function createInbound(event: SubmitEvent) {
    event.preventDefault();
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.createInbound(currentToken, {
      tag: newInboundTag(),
      port: Number(newInboundPort()),
      protocol: "vless",
      network: "tcp",
      tls_enabled: true,
    }), "Inbound created");
    setNewInboundTag("");
    await run(() => loadUsers(currentToken));
  }

  async function createHost(event: SubmitEvent) {
    event.preventDefault();
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.createHost(currentToken, {
      remark: newHostRemark(),
      address: newHostAddress(),
      port: Number(newHostPort()),
      path: null,
      sni: null,
      security: "tls",
    }), "Host created");
    setNewHostRemark("");
    setNewHostAddress("");
    await run(() => loadUsers(currentToken));
  }

  async function createProxyProfile(event: SubmitEvent) {
    event.preventDefault();
    const currentToken = token();
    if (!currentToken) return;
    await run(() => api.createProxyProfile(currentToken, {
      name: newProxyName(),
      proxy_type: "vless",
      settings_json: newProxySettings(),
      excluded_inbound_tags: [],
    }), "Proxy profile created");
    setNewProxyName("");
    setNewProxySettings("{}");
    await run(() => loadUsers(currentToken));
  }

  if (!token()) {
    return (
      <div class="login-shell">
        <form class="login-card" onSubmit={handleLogin}>
          <div>
            <p class="eyebrow">Hydra Panel</p>
            <h1>Operator Login</h1>
          </div>
          <label>Username<input value={username()} onInput={(event) => setUsername(event.currentTarget.value)} /></label>
          <label>Password<input type="password" value={password()} onInput={(event) => setPassword(event.currentTarget.value)} /></label>
          <label>2FA Code<input value={twoFactorCode()} onInput={(event) => setTwoFactorCode(event.currentTarget.value)} /></label>
          <Show when={message()}>{(item) => <div class={`message ${item().kind}`}>{item().text}</div>}</Show>
          <button type="submit" disabled={busy()}>{busy() ? "Signing in..." : "Sign in"}</button>
        </form>
      </div>
    );
  }

  return (
    <div class="app-shell">
      <div class="app-shell__layout">
        <aside class="sidebar">
          <div>
            <p class="eyebrow">Hydra</p>
            <h2>Control Plane</h2>
            <p class="muted small">Rust backend with SolidJS operator UI.</p>
          </div>
          <nav class="sidebar-nav">
            <button classList={{ active: section() === "overview" }} onClick={() => setSection("overview")}>Overview</button>
            <button classList={{ active: section() === "security" }} onClick={() => setSection("security")}>Security</button>
            <button classList={{ active: section() === "nodes" }} onClick={() => setSection("nodes")}>Nodes</button>
            <button classList={{ active: section() === "users" }} onClick={() => setSection("users")}>Users</button>
            <button classList={{ active: section() === "logs" }} onClick={() => setSection("logs")}>Logs</button>
          </nav>
          <button class="secondary" onClick={() => setToken(null)}>Logout</button>
        </aside>

        <main class="content">
          <div class="topbar">
            <div><p class="eyebrow">Runtime</p><h1>{section().charAt(0).toUpperCase() + section().slice(1)}</h1></div>
            <button class="secondary" onClick={() => run(() => refreshAll())}>Refresh</button>
          </div>

          <Show when={message()}>{(item) => <div class={`message ${item().kind}`}>{item().text}</div>}</Show>
          <Show when={section() === "overview"}><OverviewView /></Show>
          <Show when={section() === "security"}><SecurityView /></Show>
          <Show when={section() === "nodes"}><NodesView /></Show>
          <Show when={section() === "users"}><UsersView /></Show>
          <Show when={section() === "logs"}><LogsView /></Show>
        </main>
      </div>
    </div>
  );

  function OverviewView() {
    return (
      <div class="stack">
        <section class="panel">
          <div class="section-head"><div><h2>System Overview</h2><p class="muted small">Resource state, core runtime, and active alerts.</p></div></div>
          <div class="key-grid">
            <div class="kv"><span class="small muted">Memory used</span><strong>{formatBytes(overview()?.memory_used_bytes)}</strong></div>
            <div class="kv"><span class="small muted">Memory total</span><strong>{formatBytes(overview()?.memory_total_bytes)}</strong></div>
            <div class="kv"><span class="small muted">Disk used</span><strong>{formatBytes(overview()?.disk.used_bytes)}</strong></div>
            <div class="kv"><span class="small muted">Disk free</span><strong>{formatBytes(overview()?.disk.free_bytes)}</strong></div>
            <div class="kv"><span class="small muted">Core status</span><span class={`badge ${badgeClass(coreState()?.status ?? overview()?.core_status)}`}>{coreState()?.status ?? overview()?.core_status ?? "unknown"}</span></div>
            <div class="kv"><span class="small muted">Buffered logs</span><strong>{overview()?.operational_log_lines_buffered ?? 0}</strong></div>
          </div>
          <div class="stack tight" style={{ "margin-top": "1rem" }}>
            <For each={alerts()}>{(alert) => <div class="recommendation"><span class={`badge ${badgeClass(alert.severity)}`}>{alert.severity}</span> {alert.message}</div>}</For>
          </div>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Thresholds</h2></div></div>
          <form class="form-grid three-col" onSubmit={saveThresholds}>
            <label>Disk warning %<input type="number" value={thresholds()?.disk_warning_percent ?? 0} onInput={(event) => setThresholds((current) => current ? { ...current, disk_warning_percent: Number(event.currentTarget.value) } : current)} /></label>
            <label>Disk critical %<input type="number" value={thresholds()?.disk_critical_percent ?? 0} onInput={(event) => setThresholds((current) => current ? { ...current, disk_critical_percent: Number(event.currentTarget.value) } : current)} /></label>
            <label>Memory warning %<input type="number" value={thresholds()?.memory_warning_percent ?? 0} onInput={(event) => setThresholds((current) => current ? { ...current, memory_warning_percent: Number(event.currentTarget.value) } : current)} /></label>
            <label>Memory critical %<input type="number" value={thresholds()?.memory_critical_percent ?? 0} onInput={(event) => setThresholds((current) => current ? { ...current, memory_critical_percent: Number(event.currentTarget.value) } : current)} /></label>
            <div class="toolbar align-end"><button type="submit">Save thresholds</button></div>
          </form>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Core Control</h2><p class="muted small">Config editor, runtime actions, and apply history.</p></div></div>
          <div class="toolbar">
            <button class="secondary" onClick={() => coreAction("start")}>Start</button>
            <button class="secondary" onClick={() => coreAction("stop")}>Stop</button>
            <button class="secondary" onClick={() => coreAction("restart")}>Restart</button>
            <button onClick={applyGeneratedConfig}>Apply generated config</button>
          </div>
          <div class="stack" style={{ "margin-top": "1rem" }}>
            <label>Core config JSON<textarea value={coreDraft()} onInput={(event) => setCoreDraft(event.currentTarget.value)} /></label>
            <div class="toolbar">
              <button onClick={saveCoreConfig}>Save config</button>
              <span class={`badge ${coreConfig()?.valid_json ? "good" : "danger"}`}>{coreConfig()?.valid_json ? "valid json" : "invalid json"}</span>
              <span class="chip">saved: {formatUnix(coreConfig()?.saved_at_unix)}</span>
            </div>
            <div class="stack tight">
              <For each={coreApplyHistory()}>{(item) => <div class="task-card"><div class="status-row"><span class={`badge ${badgeClass(item.result)}`}>{item.result}</span><span class="chip">{item.revision}</span><span class="chip">{formatUnix(item.created_at_unix)}</span></div><div>{item.detail}</div></div>}</For>
            </div>
          </div>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Alert History</h2></div></div>
          <div class="stack tight"><For each={alertHistory()}>{(alert) => <div class="task-card"><div class="status-row"><span class={`badge ${badgeClass(alert.status)}`}>{alert.status}</span><span class={`badge ${badgeClass(alert.severity)}`}>{alert.severity}</span><span class="chip">{alert.kind}</span><span class="chip">{formatUnix(alert.created_at_unix)}</span></div><div>{alert.message}</div></div>}</For></div>
        </section>
      </div>
    );
  }

  function SecurityView() {
    return (
      <div class="stack">
        <section class="panel">
          <div class="section-head"><div><h2>Security Settings</h2></div></div>
          <form class="stack" onSubmit={saveSecurity}>
            <Show when={securitySettings()}>{(settings) => <>
              <div class="toggle-row"><span>Login protection</span><input type="checkbox" checked={settings().login_protection_enabled} onChange={(event) => setSecuritySettings({ ...settings(), login_protection_enabled: event.currentTarget.checked })} /></div>
              <div class="toggle-row"><span>Smart ban escalation</span><input type="checkbox" checked={settings().smart_ban_enabled} onChange={(event) => setSecuritySettings({ ...settings(), smart_ban_enabled: event.currentTarget.checked })} /></div>
              <div class="toggle-row"><span>Trust X-Forwarded-For</span><input type="checkbox" checked={settings().trust_x_forwarded_for} onChange={(event) => setSecuritySettings({ ...settings(), trust_x_forwarded_for: event.currentTarget.checked })} /></div>
              <div class="form-grid three-col">
                <label>Max failed attempts<input type="number" value={settings().max_failed_attempts} onInput={(event) => setSecuritySettings({ ...settings(), max_failed_attempts: Number(event.currentTarget.value) })} /></label>
                <label>Attempt window<input type="number" value={settings().attempt_window_seconds} onInput={(event) => setSecuritySettings({ ...settings(), attempt_window_seconds: Number(event.currentTarget.value) })} /></label>
                <label>Block seconds<input type="number" value={settings().block_for_seconds} onInput={(event) => setSecuritySettings({ ...settings(), block_for_seconds: Number(event.currentTarget.value) })} /></label>
              </div>
              <label>Session TTL<input type="number" value={settings().session_ttl_seconds} onInput={(event) => setSecuritySettings({ ...settings(), session_ttl_seconds: Number(event.currentTarget.value) })} /></label>
              <label>Trusted proxy IPs<textarea value={trustedIpsDraft()} onInput={(event) => setTrustedIpsDraft(event.currentTarget.value)} /></label>
              <label>Trusted proxy CIDRs<textarea value={trustedCidrsDraft()} onInput={(event) => setTrustedCidrsDraft(event.currentTarget.value)} /></label>
            </>}</Show>
            <div class="toolbar"><button type="submit">Save security settings</button></div>
          </form>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Two-Factor Authentication</h2></div></div>
          <Show when={twoFactorState()}>{(state) => <div class="stack">
            <div class="status-row"><span class={`badge ${state().enabled ? "good" : "muted"}`}>{state().enabled ? "enabled" : "disabled"}</span><span class={`badge ${state().configured ? "good" : "muted"}`}>{state().configured ? "configured" : "not configured"}</span><span class={`badge ${state().two_step_enabled ? "warn" : "muted"}`}>{state().two_step_enabled ? "2-step" : "single-step"}</span></div>
            <div class="toolbar"><button onClick={generateTwoFactor}>Generate setup</button><button class="secondary" onClick={() => toggleTwoStep(!state().two_step_enabled)}>{state().two_step_enabled ? "Disable 2-step" : "Enable 2-step"}</button></div>
            <Show when={twoFactorSetup()}>{(setup) => <div class="setup-box"><div class="kv"><span class="small muted">Secret</span><strong>{setup().secret_base32}</strong></div><div class="kv"><span class="small muted">OTP URL</span><pre>{setup().otpauth_url}</pre></div></div>}</Show>
            <div class="form-grid two-col align-end"><label>Verification code<input value={verificationCode()} onInput={(event) => setVerificationCode(event.currentTarget.value)} /></label><button onClick={enableTwoFactor}>Enable 2FA</button></div>
            <div class="form-grid two-col align-end"><label>Disable code<input value={disableCode()} onInput={(event) => setDisableCode(event.currentTarget.value)} /></label><button class="secondary" onClick={disableTwoFactor}>Disable 2FA</button></div>
          </div>}</Show>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Active Bans</h2></div></div>
          <form class="form-grid three-col" onSubmit={createBan}>
            <label>Client IP<input value={banIp()} onInput={(event) => setBanIp(event.currentTarget.value)} /></label>
            <label>Kind<select value={banKind()} onChange={(event) => setBanKind(event.currentTarget.value as "temporary" | "permanent")}><option value="temporary">temporary</option><option value="permanent">permanent</option></select></label>
            <label>Duration<input value={banDuration()} disabled={banKind() === "permanent"} onInput={(event) => setBanDuration(event.currentTarget.value)} /></label>
            <div class="toolbar"><button type="submit">Create ban</button></div>
          </form>
          <div class="stack tight" style={{ "margin-top": "1rem" }}><For each={activeBans()}>{(ban) => <div class="task-card"><div class="status-row"><span class={`badge ${badgeClass(ban.ban_kind)}`}>{ban.ban_kind}</span><span class="chip">{ban.client_ip}</span><span class="chip">{formatUnix(ban.blocked_until_unix)}</span></div><button class="secondary" onClick={() => removeBan(ban.client_ip)}>Remove</button></div>}</For></div>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Security Audit</h2></div></div>
          <div class="stack tight"><For each={auditEvents()}>{(event) => <div class="task-card"><div class="status-row"><span class="chip">{event.event_type}</span><Show when={event.username}><span class="chip">{event.username}</span></Show><Show when={event.client_ip}><span class="chip">{event.client_ip}</span></Show><span class="chip">{formatUnix(event.created_at_unix)}</span></div><div>{event.detail}</div></div>}</For></div>
        </section>
      </div>
    );
  }

  function NodesView() {
    return (
      <div class="grid two">
        <section class="panel">
          <div class="section-head"><div><h2>Nodes</h2></div></div>
          <div class="node-list"><For each={nodes()}>{(node) => <button classList={{ active: selectedNodeId() === node.id }} onClick={() => setSelectedNodeId(node.id)}><strong>{node.name}</strong><span class="muted small">{node.address}:{node.api_port}</span><div class="status-row"><span class={`badge ${badgeClass(node.status)}`}>{node.status}</span><span class={`badge ${badgeClass(node.sync_status)}`}>{node.sync_status}</span><span class={`badge ${badgeClass(node.provisioning_status)}`}>{node.provisioning_status}</span></div></button>}</For></div>
        </section>
        <section class="panel">
          <Show when={selectedNode()}>{(node) => <div class="stack">
            <div class="section-head"><div><h2>{node().name}</h2><p class="muted small">{node().address}:{node().api_port}</p></div><div class="status-row"><span class={`badge ${badgeClass(node().status)}`}>{node().status}</span><span class={`badge ${badgeClass(node().sync_status)}`}>{node().sync_status}</span></div></div>
            <div class="toolbar"><button onClick={() => nodeAction("start-provisioning")}>Start provisioning</button><button class="secondary" onClick={() => nodeAction("reprovision")}>Reprovision</button><button class="secondary" onClick={() => nodeAction("bootstrap-probe")}>Bootstrap probe</button><button class="secondary" onClick={() => nodeAction("restart")}>Restart runtime</button><button class="secondary" onClick={() => nodeAction("rollback")}>Rollback</button><button class="secondary" onClick={() => nodeAction("xray-update")}>Update Xray</button></div>
            <Show when={nodeDiagnostics()}>{(diagnostics) => <section class="stack tight"><h3>Diagnostics</h3><div class="key-grid"><div class="kv"><span class="small muted">Xray</span><strong>{diagnostics().local_state?.xray_detected_version ?? node().xray_version ?? "n/a"}</strong></div><div class="kv"><span class="small muted">Revision</span><strong>{diagnostics().local_state?.applied_revision ?? node().last_applied_revision ?? "n/a"}</strong></div><div class="kv"><span class="small muted">Runtime</span><span class={`badge ${badgeClass(diagnostics().local_state?.xray_runtime_status)}`}>{diagnostics().local_state?.xray_runtime_status ?? "unknown"}</span></div></div><For each={diagnostics().recommendations}>{(recommendation) => <div class="recommendation">{recommendation}</div>}</For></section>}</Show>
            <Show when={preflight()}>{(item) => <section class="stack tight"><h3>Preflight</h3><span class={`badge ${item().passed ? "good" : "danger"}`}>{item().passed ? "passed" : "failed"}</span><For each={item().checks}>{(check) => <div class="check-row"><span class={`badge ${badgeClass(check.status)}`}>{check.status}</span><div><strong>{check.check}</strong><div class="muted small">{check.detail}</div></div></div>}</For></section>}</Show>
            <Show when={bootstrapReadiness()}>{(item) => <section class="stack tight"><h3>Bootstrap Readiness</h3><span class={`badge ${item().ready ? "good" : "warn"}`}>{item().ready ? "ready" : "not ready"}</span><For each={item().recommendations}>{(recommendation) => <div class="recommendation">{recommendation}</div>}</For></section>}</Show>
            <section class="stack tight"><h3>Provisioning Tasks</h3><For each={provisioningTasks()}>{(task) => <div class="task-card"><div class="status-row"><span class={`badge ${badgeClass(task.status)}`}>{task.status}</span><span class="chip">{task.task_id}</span><span class="chip">{formatUnix(task.updated_at_unix)}</span></div><div class="chip-list"><For each={task.planned_steps}>{(step) => <span class="chip">{step}</span>}</For></div><For each={task.failures}>{(failure) => <div class="failure-row"><span class={`badge ${badgeClass(failure.category)}`}>{failure.category}</span><div><strong>{failure.step}</strong><div class="muted small">{failure.detail}</div></div></div>}</For><For each={task.remediation}>{(item) => <div class="recommendation"><strong>{item.action}</strong><div class="muted small">{item.detail}</div></div>}</For><Show when={task.status !== "completed"}><button class="secondary" onClick={() => retryTask(task.task_id)}>Retry task</button></Show></div>}</For></section>
          </div>}</Show>
        </section>
      </div>
    );
  }

  function UsersView() {
    return (
      <div class="stack">
        <section class="panel">
          <div class="section-head">
            <div>
              <h2>Users</h2>
              <p class="muted small">Create accounts, inspect traffic, and manage subscriptions.</p>
            </div>
          </div>
          <form class="form-grid three-col" onSubmit={createUser}>
            <label>Username<input value={newUsername()} onInput={(event) => setNewUsername(event.currentTarget.value)} /></label>
            <label>Status<select value={newUserStatus()} onChange={(event) => setNewUserStatus(event.currentTarget.value as UserStatus)}><option value="active">active</option><option value="disabled">disabled</option><option value="on_hold">on hold</option></select></label>
            <label>Data limit bytes<input value={newUserDataLimit()} onInput={(event) => setNewUserDataLimit(event.currentTarget.value)} /></label>
            <label>Note<input value={newUserNote()} onInput={(event) => setNewUserNote(event.currentTarget.value)} /></label>
            <div class="toolbar align-end"><button type="submit">Create user</button></div>
          </form>
          <div class="stack tight" style={{ "margin-top": "1rem" }}>
            <For each={users()}>
              {(user) => (
                <div class="task-card">
                  <div class="section-head">
                    <div>
                      <h3>{user.username}</h3>
                      <p class="muted small">{user.note ?? "No note"}</p>
                    </div>
                    <div class="status-row">
                      <span class={`badge ${badgeClass(user.status)}`}>{user.status}</span>
                      <span class="chip">{formatBytes(user.used_traffic_bytes)} used</span>
                      <span class="chip">limit {user.data_limit_bytes ? formatBytes(user.data_limit_bytes) : "none"}</span>
                    </div>
                  </div>
                  <div class="status-row">
                    <span class="chip">sub: {user.subscription_token}</span>
                    <span class="chip">updated: {formatUnix(user.updated_at_unix)}</span>
                    <Show when={user.sub_revoked_at_unix}><span class="chip danger">revoked: {formatUnix(user.sub_revoked_at_unix)}</span></Show>
                  </div>
                  <div class="toolbar compact">
                    <button class="secondary" onClick={() => updateUserStatus(user.username, user.status === "active" ? "disabled" : "active")}>{user.status === "active" ? "Disable" : "Enable"}</button>
                    <button class="secondary" onClick={() => resetUsage(user.username)}>Reset usage</button>
                    <button class="secondary" onClick={() => revokeSubscription(user.username)}>Revoke sub</button>
                    <button class="secondary" onClick={() => deleteUser(user.username)}>Delete</button>
                  </div>
                </div>
              )}
            </For>
          </div>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Templates</h2></div></div>
          <div class="chip-list">
            <For each={templates()}>
              {(template) => <span class="chip">{template.name} / {template.status}</span>}
            </For>
          </div>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Network Resources</h2></div></div>
          <div class="grid two">
            <div class="stack">
              <form class="form-grid" onSubmit={createInbound}>
                <h3>Inbounds</h3>
                <label>Tag<input value={newInboundTag()} onInput={(event) => setNewInboundTag(event.currentTarget.value)} /></label>
                <label>Port<input value={newInboundPort()} onInput={(event) => setNewInboundPort(event.currentTarget.value)} /></label>
                <button type="submit">Create inbound</button>
              </form>
              <div class="chip-list"><For each={inbounds()}>{(item) => <span class="chip">{item.tag}:{item.port} / {item.protocol} / {item.network}</span>}</For></div>
            </div>
            <div class="stack">
              <form class="form-grid" onSubmit={createHost}>
                <h3>Hosts</h3>
                <label>Remark<input value={newHostRemark()} onInput={(event) => setNewHostRemark(event.currentTarget.value)} /></label>
                <label>Address<input value={newHostAddress()} onInput={(event) => setNewHostAddress(event.currentTarget.value)} /></label>
                <label>Port<input value={newHostPort()} onInput={(event) => setNewHostPort(event.currentTarget.value)} /></label>
                <button type="submit">Create host</button>
              </form>
              <div class="chip-list"><For each={hosts()}>{(item) => <span class="chip">{item.remark}: {item.address}:{item.port}</span>}</For></div>
            </div>
          </div>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>Proxy Profiles</h2></div></div>
          <form class="form-grid two-col" onSubmit={createProxyProfile}>
            <label>Name<input value={newProxyName()} onInput={(event) => setNewProxyName(event.currentTarget.value)} /></label>
            <label>Settings JSON<textarea value={newProxySettings()} onInput={(event) => setNewProxySettings(event.currentTarget.value)} /></label>
            <div class="toolbar align-end"><button type="submit">Create profile</button></div>
          </form>
          <div class="stack tight" style={{ "margin-top": "1rem" }}>
            <For each={proxyProfiles()}>
              {(profile) => <div class="task-card"><div class="status-row"><span class="chip">{profile.name}</span><span class="chip">{profile.proxy_type}</span><span class="chip">{profile.id}</span></div><pre>{profile.settings_json}</pre></div>}
            </For>
          </div>
        </section>

        <section class="panel">
          <div class="section-head"><div><h2>User Activity</h2></div></div>
          <div class="stack tight">
            <For each={userActivity()}>
              {(item) => <div class="task-card"><div class="status-row"><span class="chip">{item.username}</span><span class="chip">{item.kind}</span><span class="chip">{formatUnix(item.created_at_unix)}</span></div><div>{item.detail}</div></div>}
            </For>
          </div>
        </section>
      </div>
    );
  }

  function LogsView() {
    return <section class="panel"><div class="section-head"><div><h2>Operational Logs</h2></div></div><div class="stack tight"><For each={logs()}>{(entry) => <div class="task-card"><pre>{JSON.stringify(entry, null, 2)}</pre></div>}</For></div></section>;
  }
}

export default App;
