# Lupa — Net Bank

## Application Kind
`web_app`

## Dependencies
- chrono
- uuid
- serde
- serde_json
- reqwest
- tracing
- anyhow

## Purpose

A net bank for Nordea retail banking customers. The customer can list accounts, view transactions, and initiate transfers (own-account, domestic, and international). Every feature is accessible via the web UI and via an in-process conversational chat agent. The server handles many concurrent user sessions with no cross-session leakage.

Depends on:

- ListCustomerAccounts
- ShowAccountTransactions
- TransferBetweenOwnAccounts
- TransferDomestic
- TransferInternational
- AuthenticateWithNordea
- AuthenticateSession
- RefreshNordeaSession
- RevokeSession
- RecordAuditEntry
- EnforceIdempotency
- ConfirmTransfer
- ChatSession

The application delegates all banking logic to contexts. The server is a thin adapter that authenticates requests, constructs contexts per-request, and returns responses.

---

## Runtime Topology

The application runs as a single HTTP server process. It serves the web application (SSR) and runs the chat agent in-process. The chat agent is not a remote client — it is a Rust component inside the server that receives user utterances, asks an LLM for a completion with tool schemas derived from the context catalogue, and dispatches tool calls directly to contexts as regular in-process function calls.

---

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| Nordea API client | Connection-pooled HTTP client for Nordea OpenBanking calls. Shared across requests. |
| Session store | Pluggable: in-memory for dev and fixture-mode tests, durable (Postgres or Redis) for staging and production. |
| LLM client | Shared HTTP client for the chat agent's LLM backend. Anthropic Messages API by default. |
| Agent tool registry | Built once at startup from the context catalogue. In-process Rust data structure, not a network surface. The tool set equals the business-context functionalities and nothing more. |
| Audit sink | Pluggable; structured-log sink for the reference implementation. Bounded buffer; overflow is itself an audit event. |
| Idempotency ledger | Pluggable; in-memory for tests, durable backing for production. Scoped per customer. |
| Telemetry | Structured logs with PII redaction, request tracing via spans, metrics. |

---

## Startup Sequence

On startup:

- Read Nordea client credentials from the environment (client id, client secret)
- Read session-store connection config from the environment
- Read LLM credentials from the environment
- Read deployment config: bind address, base URL for OAuth callback, environment name
- Connect to the session store and verify connectivity
- Perform a graceful-degradation check against Nordea's sandbox
- Build the agent tool registry from the context catalogue
- Mount the HTTP surfaces: SSR routes from the routing table, JSON endpoints for the chat pane, static assets for brand stylesheet and client WASM

The application does not assume a current user at startup. A user identity only exists once a request arrives bound to an authenticated session cookie.

---

## Session Lifecycle

Sessions begin at the successful completion of the Nordea OAuth consent flow with PSD2 scopes and SCA challenge. The session stores the Nordea bearer token, the refresh token, the customer identifier, and conversation state for the chat agent. Sessions expire on the shorter of bearer-token expiry, configured idle timeout, or explicit logout. Expired sessions trigger refresh-token exchange where possible, or redirect to re-authenticate. Session persistence crosses server restarts — the session store is external, not in-process memory.

---

## Main Loop Behavior

For every incoming HTTP request:

1. Authenticate the session (resolve session cookie, verify liveness, check scopes)
2. Bind the authenticated customer as a role player
3. Construct the relevant context per-request with role players bound from the authenticated session
4. Invoke the context functionality
5. Record an audit entry
6. Return the response

Unauthenticated requests to protected routes redirect to the login flow. The chat pane works the same way — each chat turn is a request; the agent's tool calls are ordinary in-process context invocations with per-request construction, scoped to the chat turn's session.

---

## Concurrency Model

The server handles many concurrent sessions. No global mutable state holds per-user data. Shared resources (Nordea HTTP client pool, LLM client, session-store handle, audit sink, metrics registry) are constructed at startup and shared across requests. Everything user-scoped is bound per-request. The async runtime is the executor; blocking work runs on the blocking pool.

---

## Security Posture

- CSRF tokens on every state-changing form, bound to the session
- Session cookies with secure flags: same-site strict, HTTP-only, secure (except loopback dev)
- Per-response Content Security Policy with a session-scoped nonce
- HTTPS required in staging and production; HTTP permitted only on loopback for dev
- Rate limits on auth and transfer endpoints keyed by session and source address
- Automatic lockout after repeated SCA failures
- No PII in logs — account numbers, IBANs, and names are redacted at the logging layer, not at call sites

---

## Error Handling

Runtime errors return structured error responses. Authentication failures redirect to login. Transfer failures return the failure classification from the idempotency ledger or Nordea's error response. Every error is audit-logged before returning.

Normal exit code is 0. Graceful shutdown on SIGTERM drains in-flight requests before exit.
