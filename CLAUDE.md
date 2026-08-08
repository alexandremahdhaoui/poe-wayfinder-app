
## Architecture

Read `docs/architecture-audit/scope.md`. This crate drifted and was refactored.
The `architecture` forge stage enforces the rules now. When it fails, move the
code. Never raise the floor.

main only builds adapters, injects them into controllers, injects those into
drivers, and starts the drivers. Under 150 lines.

Copy `~/workspaces/playground/golden-rust` for shape. Traits live with their
implementation. `#[cfg_attr(test, mockall::automock)]` sits above the trait.
There is no mocks directory.
