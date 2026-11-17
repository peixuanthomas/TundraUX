# identity

`identity` attaches each Shell run to the operating-system user that launched it. It records the system username and an in-memory session identifier for worker coordination. Workspace operations use the current process's OS permissions. Linux user management delegates to `platform::management::users` and system authorization.

`platform` reads the username from Linux NSS, the macOS account database, or the Windows logon API. Account-specific appearance, dashboard, and clock preferences are stored by username in [storage](../storage/README.md). The process session carries neither a password nor a saved login token.

## Validation

```sh
cargo test --locked -p identity
```
