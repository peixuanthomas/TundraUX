# Application self-updates

Open **Settings → Update** to check the current build and available updates. On Linux, in-app updates require a writable portable directory with both executables and `tundra-installation.json`. System package installations use their own update mechanism.

The workflow is implemented in [`app::update`](../src/update/). Shell saves settings and exits for replacement; the companion CLI runs the update helper.

## Update modes

On Linux, **Settings → Update → Update mode** is saved as `linux_update_mode` in `config.toml`, with `release` as the default.

- **Release:** reads the latest stable GitHub Release, downloads the Linux x86_64 portable archive, verifies SHA-256, and extracts Shell, CLI, and the complete Vim. Rust and Python are not required.
- **Beta:** shows the latest `master` build and a Git tree with commits and merge lines from every published branch. Select any visible commit, including an older one or one on an unmerged branch. Confirm to download that exact commit and build it with local Rust.

Switching modes clears old check results and checks again. The current mode is retained during a download or build. Downloads and source builds use the user's cache directory. Linux Release updates extract the verified archive once, directly into a private transaction directory on the installation filesystem. The updater checks the programs and Vim and flushes the payload before saving the recovery record.

Windows uses the same source update page, with the remote default branch as the initial selection. macOS currently has no application self-update entry.

The Git tree uses [Git's commit graph](https://git-scm.com/docs/git-log#Documentation/git-log.txt---graph) in the style of [Tig's log view](https://jonas.github.io/tig/doc/tig.1.html). Git reads all branch histories without checking out any files. If Git cannot load the history, the GitHub API can still offer the latest build on the default branch.

Source updates compare the target `packaging/vim.json` with `UPSTREAM.json` in the Vim directory beside the running program, then the application data directory. Matching compares the complete parsed configuration, including the upstream commit, checksum, required features, and Windows iconv dependency. A valid match copies the executable, runtime, licenses, and metadata into the prepared update without launching Python or compiling Vim. Missing or incomplete engines are rebuilt. Copying preserves executable permissions and responds to cancellation; installation and rollback use the same complete Vim payload as before.

An up-to-date Release shows the update mode and current build. When a release is available, the page adds the target build, an update button, download progress, and update notes. The source update page keeps the Git tree available even when the installed build is current. It shows compilation progress and live output beside or below the tree. Opening the page checks for updates. Press F5 to check again.

## Build logs

Each source update writes Cargo and Vim stdout and stderr to a separate `update-*.log` in the application log directory, before the UI applies its 200-line tail and line-length limits. Build progress output and long diagnostic lines are retained. The file includes the target commit, build stages, and exit results, and remains after failure or cancellation even when temporary build files are removed.

Build failures include the log path. Open **Logs → Open folder** to find the file and view it in the editor. On Linux the directory is `$XDG_STATE_HOME/TundraUX/logs`, or `~/.local/state/TundraUX/logs` when `XDG_STATE_HOME` is unset. These separate text logs can be removed with the existing `tundra-cli debug clear-logs legacy --yes` command; runtime JSONL rotation does not manage them.

## Replacement and rollback

The update helper retains the old programs and old `vim/`, then removes the backups after the new Shell reports successful startup. Replacement failure, startup failure, or timeout restores both the programs and Vim. If the old installation had no Vim, rollback removes the directory added by this update.

After an interrupted update, the next startup continues recovery using the transaction record in the installation directory. The Vim directory must contain the executable, complete runtime, license, and `UPSTREAM.json`.

The updater uses transaction protocol 3. Each transaction record includes the Vim transaction state.

Updates do not replace user data, default themes, custom themes, or local assets. Linux preserves executable permissions; the helper replaces the original process through exec and waits for the new Shell to exit. Windows uses backup copies to handle files still held by running processes and cleans up old backups on subsequent startups.

## Cancellation and cleanup

Checks, downloads, Git queries, builds, extraction, validation, and copying all respond to cancellation. Waiting for the network or a child process can also be interrupted; only process trees and directories created by the current task are cleaned up.

A prepared replacement transaction writes a `Cancelled` record before deleting the staging directory. If writing the marker fails, the directory is retained for retry. At startup, cancellation records trigger cleanup only, not replacement.

If successful preparation and cancellation occur at the same time, Shell rechecks cancellation before accepting the result. Once the update helper takes over, it continues with installation, startup confirmation, and rollback. Only one update runs at a time.

See [Update tests](update-tests.md) for test entry points.
