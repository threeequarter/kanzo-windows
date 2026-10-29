# AGENTS.md

## Purpose and Boundaries

Kanzo is a kanban board in the browser plus an MCP server, in one Rust binary. This repo is a personal fork, `origin` = threeequarter/kanzo-windows, of `upstream` = emanuele-em/kanzo. The fork adds Windows support for two machines: an ARM64 laptop and an AMD64 server. It publishes no releases. Updating means merging `upstream/main` and building again.

Upstream's conventions apply to all code here: `.github/CONTRIBUTING.md` (keep it light, no new dependencies without a strong reason, short plain-text MCP replies, a board with no build step, plain-English comments). This file covers only what differs in the fork and how to keep the next upstream merge easy.

Agents don't commit or push; the user does.

## Fork Delta

Everything the fork changes. Before a merge or a change near these spots, check this list. `git diff upstream/main` gives the exact patch.

| Area | Where | What the fork does |
|---|---|---|
| Logon service | `src/service.rs`: `TASK`, `task_xml`, `windows_quote`, `install_task`, `uninstall_task`, `log_path` | `kanzo service install`/`uninstall` on Windows register a Task Scheduler task through `schtasks /Create /XML` (UTF-16 with a BOM). It starts at the user's logon, as that user, without admin rights. `restart()` has no Windows branch: it returns `false`. |
| Background mode | `src/main.rs` (hidden `--background` flag), `service::detach_console` | The task passes `--background`. Kanzo then calls `FreeConsole` and points stdout/stderr at `%LOCALAPPDATA%\kanzo\kanzo.log` with `SetStdHandle`. |
| No console flashes | `service::no_window`, used in `sync::command`, `sync::hostname`, `web::Guard::new`, `mcp::git_repos` | Once detached, child programs start with `CREATE_NO_WINDOW`. Otherwise every git call opens a window. It does nothing in a terminal, so `kanzo sync` can still prompt for a passphrase. |
| Self-update off | `update::SELF_UPDATE`, `update::NO_SELF_UPDATE`; `main::update_kanzo`; `web::update_status` (`off` field); `app.js` `checkForUpdates` | On Windows, `latest()` returns `None`, `install()` fails, and the board says it doesn't update itself. `update::RELEASES` still points to upstream and is never used on Windows. |
| Paths | `home::without_verbatim`, applied in `Home::open` | Removes the `\\?\` prefix that `canonicalize` adds on Windows, so users and agents see `C:\...`. |
| DB identity | `Home::db_identity`, `#[cfg(windows)]` branch | Uses the NTFS file index (`GetFileInformationByHandle`), so the board notices when `kanzo sync` replaces `kanzo.db`. Don't switch to the creation time: NTFS file tunneling gives a file recreated under the same name the old creation time. |
| Lock errors | `home::unsupported` | Treats `ERROR_INVALID_FUNCTION` and `ERROR_NOT_SUPPORTED` as "no file locks here". |
| Browser | `web::open_browser` | Opens the board with `explorer.exe <url>` on Windows. |
| Workspace marker | `src/mcp.rs`: `Workspace::find`, `Ctx::project`, `INSTRUCTIONS` | A `.kanzo` file (`project = "slug"`) in the agent's cwd or a folder above it names the project. Lookup order: an explicit `project`, then `.kanzo`, then git remotes. A broken `.kanzo`, or one naming an unknown project, fails with the file's path rather than falling back. Not platform-specific; only stdio MCP uses it. |
| UI labels | `src/index.html` titles | Ctrl+K and Ctrl+Enter instead of ⌘. `app.js` already accepted both keys. |
| Build | `.cargo/config.toml` | `+crt-static` for `aarch64-`/`x86_64-pc-windows-gnullvm`, so `kanzo.exe` needs no `libunwind.dll`. |
| Dependency | `Cargo.toml` `[target.'cfg(windows)'.dependencies]` | `windows-sys` 0.61 (already in the lock file through other crates), features Foundation, Storage_FileSystem, System_Console, System_Threading. |
| CI | `.github/workflows/windows.yml` | fmt, clippy and tests on `windows-11-arm` and `windows-latest`. |
| Docs | `README.md` "Windows (this fork)" | Build, install and `.kanzo`. |

## Change Guidance and Invariants

Keeping merges cheap:
- Put Windows code in `#[cfg(windows)]` items, or in `cfg!(windows)` branches next to the existing macOS/Linux ones. Don't change Unix behavior.
- Add new code as new functions rather than rewriting upstream bodies. `service.rs` keeps the Windows code in its own section after the systemd code.
- Don't edit or delete upstream files under `.github/`. Fork CI lives only in `windows.yml`.
- Don't add database migrations or raise `COMPATIBLE_FROM` (`src/store.rs`). Upstream owns the schema numbering, and a fork migration would clash with its next one. Board-level fork features belong outside the database, the way `.kanzo` does.
- Code that is unused on Windows must be gated so clippy stays clean: `-D warnings` runs on Windows too. Examples: `update::restart_args` and its `OsString` import are `#[cfg(unix)]`.
- A CLI flag used only on Windows must still be read on every platform. `main` reads `cli.background` outside the `cfg`, so Linux and macOS don't warn about an unused field.

Windows invariants:
- Every child process the board or `kanzo mcp` starts goes through `service::no_window`. CLI-only commands such as `schtasks` in `install_task` don't need it.
- `install_task` replaces the task first, then stops and restarts it. A failed `/Create` must leave the running board alone. Replacing a running task doesn't stop it.
- Don't change the task XML's `InteractiveToken` principal or its logon trigger for the user without the user's agreement. They keep the board in the user's session, with access to `~/kanzo` and git credentials, and no admin rights.

After each `git merge upstream/main`, check:
- New `Command::new` calls in board or MCP code need `no_window`.
- New `#[cfg(unix)]` code needs a Windows branch or gating, so the Windows build and clippy pass.
- New `canonicalize` calls on user-visible paths need `without_verbatim`.
- Changes in `update.rs` must keep `SELF_UPDATE` short-circuits at the top of `latest` and `install`.
- Changes to `mcp::INSTRUCTIONS` must keep the `.kanzo` sentence. Changes to `Ctx::project` must keep the lookup order.
- Then update the delta table above.

## Validation

Local builds use llvm-mingw in `~/.llvm-mingw` and the gnullvm toolchain. The default MSVC toolchain can't link on the laptop. From Git Bash, in the repo root:

```sh
export PATH="$HOME/.llvm-mingw/bin:$PATH"
cargo +stable-aarch64-pc-windows-gnullvm fmt --check
cargo +stable-aarch64-pc-windows-gnullvm clippy --all-targets --locked -- -D warnings
cargo +stable-aarch64-pc-windows-gnullvm test --locked
cargo +stable-aarch64-pc-windows-gnullvm build --release --locked --target-dir target/install
cargo +stable-aarch64-pc-windows-gnullvm build --release --locked --target x86_64-pc-windows-gnullvm   # server
```

- Build into `--target-dir target/install` while the board runs. A running `kanzo.exe` can't be overwritten, so `target/release` is locked if the board was ever started from there.
- The Windows-specific tests are `home::tests::paths_without_verbatim_prefix`, `home::tests::identity_changes_when_the_database_is_replaced`, `service::tests::task_quotes_paths`, and the `mcp::tests` for `.kanzo`.
- macOS and Linux can't be checked locally. Upstream's `ci.yml` runs on pushes to the fork's `main`.

## Known Issues and Guardrails

- Never push version tags. `release.yml` runs on tags like `0.2.1` and would try to build and publish upstream's release, including to crates.io.
- If the user registered the `Kanzo` task from an administrator terminal, their account can only read it. Then `kanzo service install` fails with "Access is denied", while `schtasks /End` and `/Run` still work. The fix is `kanzo service uninstall` once from an administrator terminal, then `install` from a normal one.
- To swap the installed `~/.cargo/bin/kanzo.exe` without re-registering, rename the running file to `kanzo.exe.old`, copy the new build in, then `schtasks /End /TN Kanzo` and `schtasks /Run /TN Kanzo`. Running `kanzo mcp` processes keep the old file open until their Pi session ends.
- Unverified: whether Task Scheduler's `RestartOnFailure` fires when kanzo exits with an error rather than failing to start.
