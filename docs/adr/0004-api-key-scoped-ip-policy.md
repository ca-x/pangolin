# ADR 0004: Keep IP policy scoped to API keys

Status: accepted

## Context

AxonHub exposes an instance-wide client-IP blocklist and a request-log action that
adds an address to it. Pangolin resolves the trusted connection address at gateway
admission and evaluates `allowed_ips` and `denied_ips` on the authenticated API key.
The policy therefore travels with the credential, remains project-scoped, and is
checked before routing or provider I/O.

An instance-wide list would create a second authority outside the key policy. In a
self-hosted deployment the apparent peer may also be a reverse proxy unless trusted
proxy handling is configured explicitly; promoting a request-row address into a
global ban could lock out unrelated projects or trusted infrastructure.

## Decision

Pangolin records divergence D3 and keeps gateway IP authorization on each API key.
There is no instance-wide IP blocklist and no request-log “ban IP” mutation. Operators
who need a site-wide network boundary should enforce it at the reverse proxy or host
firewall, where trusted-proxy and network topology are known.

Key policies continue to support bounded allow/deny CIDR lists, are project-scoped,
and fail closed at admission. The console exposes those fields on the key itself.

## Consequences

- A single address must be denied on each affected key, or at the deployment edge.
- Creating another key does not inherit a global deny list because none exists.
- A request-log row cannot silently create instance-wide authorization state.
- Pangolin does not claim AxonHub parity for the global blocklist or ban affordance;
  the capability matrix, parity ledger, and README disclose the boundary.
- Reconsidering this decision requires a new ADR covering trusted proxy resolution,
  audited mutations, project/instance authority, lockout recovery, bounded storage,
  and admission tests.
