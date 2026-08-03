# Persistence

Both halves of Hydra keep state in files rather than a database. The rules below
are the same on each side; the file inventories differ.

## Loading

Persistent readers return a `Result` and distinguish three cases:

- the file is absent — a default is used, because this is a normal first run;
- the file is unreadable — an error, naming the path and the I/O cause;
- the file is present but does not parse — an **error, never a default**.

All three used to collapse into an empty struct. One incompatible record silently
destroyed a whole file, including records unrelated to it: inbounds, hosts and
profiles disappeared with no message. Losing a file quietly is worse than
refusing to start, because nothing points at the cause.

`AppState::new` therefore returns a `Result`, and the panel refuses to start on a
parse failure, naming the file and the parse position. On the node the split runs
along a different line: startup readers refuse to start, while runtime readers
return the error upward, because a process already serving traffic must not die
on a bad file mid-operation.

Returning a `Result` puts the decision at the call site. Every current caller
comes from startup and chooses to refuse; a future reload-without-restart caller
can handle the same error differently without changing the readers.

## Writing

Files are replaced through `replace_file_durably`: write to a temp file,
`fsync(temp)`, `rename`, `fsync(directory)`. All four steps matter. Without the
temp fsync the rename can reach the journal before the data, leaving a valid name
pointing at garbage. Without the directory fsync the rename itself is not
durable. ext4 with `data=ordered` usually hides both, but that is a mount option,
not a POSIX guarantee.

fsync is applied by recoverability, not by size:

| fsync | Panel files |
| --- | --- |
| yes | admin store, master keys, route materials, reality materials, subscription catalog, nodes, network resources, clusters, core config, security settings, Telegram settings, users, user templates, provisioning tasks, monitoring thresholds |
| no | operational log, audit log, alert history, Telegram events, usage points, installer jobs |

On the node the same split gives fsync to node state, generated config, node
runtime config, sidecar runtime config, the Xray config, the route credential
manifest, generated sidecar configs and the WireGuard session map; apply history
and runtime events use `Durability::BestEffort`.

The second group in each case is the bounded, compacted buffers. Losing their
last few records to a power failure is acceptable; an fsync per event at 1 vCPU
is not.

## Permissions

Temp files are created with mode `0600` through `OpenOptions::mode`, not
chmod'ed afterwards. `fs::write` creates by umask, usually `0644`, and in the
window before the chmod the material is world-readable. This was not theoretical:
the AES-256-GCM master keys, the Argon2 admin hash, the encrypted 2FA secret,
WireGuard private keys, Hysteria2 configs and the credential manifest all passed
through that window, and two panel writers never chmod'ed the final file at all.

Directories the panel creates are created at `0700` through `create_secret_dir_all`
(`DirBuilder::mode`), for the same reason: setting the mode at creation leaves no
window, while a chmod afterwards does.

## Startup permission audit

The panel refuses to start when a secret-class file is wider than `0600` or a
data directory is wider than `0700`, naming the path, the actual mode and the
`chmod` that fixes it. Directory permissions are included because permission to
list a directory defeats the mode of the files inside it.

Master keys are why this check exists rather than being a nicety. They are
written **once**: the loader reads an existing key and returns, so the corrected
write path never touches the file again and there is no self-healing through
temp-and-rename. Without this check, a key with wide permissions stays wide
forever.

The check refuses rather than repairs. A silent `chmod` would hide that the
secret was readable for some period, and that is exactly what the operator needs
to know. In practice the triggers are not the panel's own history: a foreign
umask when restoring from a backup, or an operator's manual `chmod`. Hydra is
strict about what it did not create and correct about what it does.

Covered paths: the admin store and its key, the node store and its key, Telegram
settings and their key, the subscription catalog and the devices key, route
materials and their key, and Reality materials and their key.

## Panel file inventory

Each path is configurable through its environment variable:

`HYDRA_SECURITY_SETTINGS_PATH`, `HYDRA_ADMIN_PATH`,
`HYDRA_ADMIN_SECRETS_KEY_PATH`, `HYDRA_AUDIT_LOG_PATH`,
`HYDRA_MONITORING_THRESHOLDS_PATH`, `HYDRA_ALERT_HISTORY_PATH`,
`HYDRA_CORE_CONFIG_PATH`, `HYDRA_CORE_APPLY_HISTORY_PATH`,
`HYDRA_NODE_APPLY_RESULTS_PATH`, `HYDRA_OPERATIONAL_LOG_PATH`,
`HYDRA_USERS_PATH`, `HYDRA_USER_ACTIVITY_LOG_PATH`,
`HYDRA_USER_TEMPLATES_PATH`, `HYDRA_NETWORK_RESOURCES_PATH`,
`HYDRA_CLUSTERS_PATH`, `HYDRA_NODES_PATH`, `HYDRA_NODE_SECRETS_KEY_PATH`,
`HYDRA_NODE_SYNC_HISTORY_PATH`, `HYDRA_TELEGRAM_SETTINGS_PATH`,
`HYDRA_TELEGRAM_SECRETS_KEY_PATH`, `HYDRA_TELEGRAM_EVENTS_PATH`,
`HYDRA_ROUTE_MATERIALS_KEY_PATH`.

A persisted format is a schema like any other: changes to one are versioned
through the [schema registry](schema-versioning.md), and migration impact is
documented rather than discovered.

## Xray integration paths

Xray validation and updates are configured separately, since they touch an
external binary:

| Variable | Purpose | Default |
| --- | --- | --- |
| `HYDRA_XRAY_BINARY_PATH` | binary used for external config validation | unset |
| `HYDRA_XRAY_VALIDATION_TEMP_DIR` | temporary validation files, deleted after each check | `data/xray-validation` |
| `HYDRA_XRAY_RELEASE_API_URL` | release feed for update planning | official `XTLS/Xray-core` latest release API |
| `HYDRA_XRAY_UPDATE_WORK_DIR` | update working directory | `data/xray-updates` |
| `HYDRA_XRAY_UPDATE_MAX_DOWNLOAD_BYTES` | download size bound | `134217728` |

Update archives are extracted with enclosed archive paths only, and only the
candidate binary is extracted. A binary swap keeps a backup and attempts a
rollback when post-swap validation fails.
