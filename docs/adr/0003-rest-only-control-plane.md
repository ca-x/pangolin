# ADR 0003: Keep the control plane REST-only

Status: accepted

## Context

AxonHub exposes a GraphQL service endpoint and playground. Pangolin currently has one
control-plane protocol: project-scoped REST handlers resolve a session or service-key
principal, authorize the exact project permission, and audit every mutation in the same
SQLite transaction as the state change.

A useful GraphQL surface would need a stable schema, field-level cost and size bounds,
object-level project scoping, mutation authorization, transactional audit semantics, and
an authenticated playground. Pangolin has no GraphQL consumer or independently specified
schema yet. Adding a token endpoint that merely mirrors one REST read would satisfy the
route name while creating a second authorization and error surface with no product value.

## Decision

Pangolin records divergence D6 and keeps the control plane REST-only. There is no GraphQL
endpoint or playground, and therefore no GraphQL mutation can bypass a REST refusal or its
audit requirement. Project reads continue through the existing `project:read` permission;
mutations continue through their named REST permission and transactional audit path.

This decision does not affect the public AI protocol routes.

## Consequences

- Operators have one documented control-plane protocol, error envelope, authorization
  path, and mutation audit contract.
- Pangolin does not claim AxonHub GraphQL parity; the capability matrix and parity ledger
  identify D6 explicitly.
- GraphQL may be reconsidered only with a concrete consumer and schema. A future ADR must
  require project-scoped authorization tests for every root field, bounded query depth and
  complexity, an authenticated playground, and mutation tests proving the same permission
  and transactional audit behavior as REST.
