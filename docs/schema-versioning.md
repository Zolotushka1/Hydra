# Schema versioning

Every document that crosses an API boundary carries a `schema_version`, and every
version in the product lives in one registry: `panel-core/src/schemas.rs`.

## The registry

Document constructors read their version from `SchemaId::X.version()`, and
`GET /api/ui/contracts` publishes the same registry. A document body and the
published contract cannot advertise different numbers, because there is only one
number. This closed a real drift, in which the contract advertised
`protocol_capabilities` version 1 while the view emitted 3.

Three kinds of schema exist:

| Kind | Meaning | Published |
| --- | --- | --- |
| `document` | exactly one constructor, which takes its version from the registry | yes |
| `model` | spans several documents, no single constructor | yes |
| `persistence` | an on-disk format | no |

`subscription_catalog` is the only model: nothing ever emits that number, it
tracks the evolution of the catalog model itself. The document actually rendered
behind it is `subscription_bundle`.

A schema version is never written as a literal. Adding one means adding a
`SchemaId` variant; the `match` in `spec()` is exhaustive, so a variant without a
spec does not compile. `schema_version` is `u16` everywhere.

The authoritative table of names, versions and kinds is in
[`panel/docs/api.md`](../panel/docs/api.md), and a test requires it to match the
code row for row.

## The policy

| Change | Version bump |
| --- | --- |
| optional field added | no |
| field renamed, removed or retyped | yes, plus a changelog line |
| field given a different meaning under the same name | yes, plus a changelog line |
| enum variant added | no |
| enum variant removed, or its serialized value renamed | yes, plus a changelog line |

The meaning-change case is the one to watch. The JSON keeps its shape, every
mechanical check stays green, and a client keeps parsing it while being wrong. No
test catches that; it is bumped by hand or not at all.

Adding an optional field is only safe while consumers ignore unknown fields, so
`#[serde(deny_unknown_fields)]` never goes on a type that crosses the API.
Adding an enum variant is only safe while every consumer already has a
`_ => Unknown` arm — that obligation is on consumers, because the panel is where
new variants originate. Inside the panel, matches on contract enums stay
exhaustive, which is what the enum registry enforces.

## What keeps the policy honest

Three tests. The schema registry table in `panel/docs/api.md` must match the code
row for row; every schema name must appear in that document; and the policy
section itself must still describe every case, including the `_ => Unknown`
obligation. If the section is renamed or cut, the comments and error messages
that reference it become a dead link, and the tests fail rather than the
reference rotting quietly.

Version history predating the registry was never recorded and is not
reconstructed. Changelog entries start from the first bump made under this
policy.

## Registries this depends on

**Route table.** `panel-core/src/routes.rs` holds `ROUTE_TABLE`: one `RouteSpec`
per method-and-path pair. Two things derive from it and nothing else — the axum
router, built by iterating the table, and `GET /api/ui/contracts`, which
publishes the table filtered to `admin_ui` exposure. There is no second list of
routes. Adding a route means a `RouteId` variant, a table row and a handler arm;
a missing arm is a compile error and a missing row fails a test. The method is
read from the row, so a route cannot be declared `GET` and registered as `POST`.

**Enum registry.** `panel-domain/src/registry.rs` registers every enum that
crosses the API through the `enum_registry!` macro, which gives each type a
`pub const ALL` plus an exhaustive `match`. Adding a variant without registering
it fails to compile with `non-exhaustive patterns`, so a variant cannot silently
disappear from the published contract. The contract publishes the serde
representation of each entry of `ALL`, so the declared string and the wire string
are the same string by construction. Before this existed, the contract carried a
second hand-written list that had already lost a variant.

## Contract tests

Golden snapshots of the admin-surface documents live in `panel-core/golden/`.
They catch renamed and removed fields, which compile fine and break a frontend
silently. Regenerate them with
`HYDRA_UPDATE_GOLDEN=1 cargo test -p panel-core golden`; a changed golden belongs
in the same commit as the change that caused it, because reading that diff is the
mechanism.

Volatile values are normalized: timestamps, identifiers, tokens, hashes,
revisions, paths and live host measurements. Field names are never normalized,
since renames are what the snapshots exist to catch.

A secret guard walks each snapshot: keys named like credentials must not carry
non-empty strings, and no string may look like credential material — a PEM block,
or 32 or more characters of base64-shaped text with mixed case and digits. The
guard matches on **shape, not substrings**. `node_auth_token_rotated` is an audit
event type and `/api/system/secret-readiness` is a route; a substring guard flags
both. The guard is itself tested against known-bad inputs, so it cannot rot into
a no-op.
