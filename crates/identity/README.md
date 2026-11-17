# identity

`identity` attaches each Shell run to the operating-system user that launched it. It gives app code a common identity and permission vocabulary. System reads and writes still use the acting process or authorized helper's OS permissions.

## Modules

| Location | Responsibility |
| --- | --- |
| [`identity`](src/identity.rs) | `AuthSession::current` validates the current username, assigns the applicable workspace role, and creates a transient session ID and start time. Linux administrator status is read through the native account backend. |
| [`authorization`](src/authorization.rs) | Defines `UserRole`, `PermissionAction`, and `PermissionService` for workspace actions. `DebugPolicy` describes whether the current build allows debug actions. |
| [`time`](src/time.rs) | Provides the Unix epoch timestamps used for session metadata and identifiers. |

`platform` reads the username from Linux NSS, the macOS account database, or the Windows logon API. Account-specific appearance, dashboard, and clock preferences are stored by username in [storage](../storage/README.md). The process session carries neither a password nor a saved login token. Linux account changes go through [platform user management](../platform/docs/user-management.md).

## Validation

```sh
cargo test --locked -p identity
```
