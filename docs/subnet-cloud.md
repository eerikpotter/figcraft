# subnet.ee cloud integration

The optional `icp-cloud` feature adds the shared **Sovereign Cloud** section to
FigCraft's File menu: Save, Save As and Open. It uses the same signed-in account
and personal/shared-folder picker as the other subnet.ee tools.

The cloud file is a native `.figcraft` archive (`figcraft.figcraft`, version 1),
including embedded images, saved versions and branches. Opening adds a new tab.
Saving a branch writes its main file with the branch's latest content. A completed
upload only marks its unchanged snapshot clean; later edits stay unsaved. Return
to the current design and finish active text editing or drags before saving.

Shared folders, version conflicts, account changes, transfer verification and
retry behavior are provided by the pinned `vendor/sovereign-cloud` and
`vendor/sovereign-cloud-ui` packages. Format encoding remains in the web adapter.
The optional archive thumbnail is omitted. Local desktop file operations are unchanged.

This feature expects the subnet portal's `/pilveruum/` picker, shared session and
runtime canister configuration on the same origin. It does not provision a cloud
backend or enable FigCraft's separate live-session server.

From the subnet repository:

```sh
python3 scripts/sync-sdk.py figcraft --check-pinned
python3 scripts/build.py figcraft --assemble
python3 scripts/dev.py figcraft
python3 scripts/verify.py figcraft
```

From this fork, run adapter tests with
`cargo test -p figcraft-web --features icp-cloud`. The standalone browser build
remains `trunk build --release` in `apps/figcraft-web`; pass `--features icp-cloud`
when packaging it for a subnet-compatible host.

## Development record

2026-10-15: added the host File-menu extension, shared cloud adapter and all-tab
dirty-state reporting. The adapter includes branch-tab revisions in its change
token and captures their current content without changing the editor engine.
The host uses the existing dark-popover style for the shared menu controls in
both editor themes. Four adapter tests, the menu hook test, release Wasm build,
focused web Clippy, formatting, layers, assets and standalone Wasm/plugin checks
passed. Local browser save/reopen, account isolation and menu appearance were
verified. Full upstream CI stops on existing Clippy errors in the unchanged
`.fig` exporter. Details are in the parent repository's `docs/figcraft-integration.md`.
