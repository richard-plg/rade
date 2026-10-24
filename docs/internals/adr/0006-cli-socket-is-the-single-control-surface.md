# One JSON-RPC socket API for agents, users, and tests

Everything an Agent or script can do goes through the `rade` CLI → Unix socket JSON-RPC ([API.md](../API.md)); no second control path (no D-Bus, no MCP in v1). End-to-end tests drive the real app in a headless Wayland compositor through the same CLI, so the API is exercised by every test and the agent-facing contract cannot silently rot.

The hidden `debug.*` methods the tests use are served only by an app started in test mode (`RADE_DEBUG_SOCKET=1`), never to Agents of a normal app: [ADR-0021](0021-debug-socket-methods-only-in-test-mode.md).
