# Working on Far Ray Tracer

Instructions for anyone changing this project, people and coding agents alike. Read it
before the first change; it says how to build and check the work, write the changelog,
commit, release, and share the checkout with other agents.

## The project

A ray tracer that began as a C++ final project in 2008, ported to Rust, with a desktop app.

| Path | What it holds |
| --- | --- |
| `crates/far-core` | The engine: scene loading, the 2008, Fixed and Enhanced renderers, materials, lights, volumes |
| `crates/far-cli` | The `far` command line |
| `src-tauri` | The Tauri 2 desktop app: commands, menu, updater |
| `src` | The app's React and TypeScript interface |
| `assets/scenes` | Example scenes, in `.far` (JSON); `assets/scene-data` has their images, meshes and IES files |
| `reference` | The original C++ program and the scripts that compare it with the port |
| `tools` | Scripts that generate example scenes, textures, screenshots and fetch Open Image Denoise |
| `docs/images` | The README's screenshots |

The README explains every feature and the scene format; `TODO.md` lists what is planned;
`CHANGELOG.md` lists what changed in each version.

## Ground rules

- **Language.** Everything in the repository is in English: code, comments, docs, commit
  messages, release notes. Talk to the user in the user's language.
- **Writing.** Plain, complete sentences. Comments say why, not what the line already says.
  Name things for what they are; no abbreviations a reader must decode.
- **The 2008 renderer is history.** Classic mode must stay pixel-identical to the original C++
  program: `npm run validate` must report `same=18 different=0`. New features go into the
  Enhanced mode only. The code in `reference/cpp` keeps its original Portuguese comments and
  is changed only to make it build.
- **The original project folder outside this repository is read-only.**
- **Scene files are strict.** New `.far` fields go in `crates/far-core/src/scene_file.rs`, with
  `deny_unknown_fields`, validation and an error message that says what is wrong and what is
  allowed. Old scenes must keep opening unchanged.
- **No secrets in the repository.** The update signing key lives outside it (see Releases).
- **License.** MIT, FARSystems. Third-party code and data need an entry in
  `THIRD_PARTY_NOTICES.md`; example data (IES files, textures, models) is made by the scripts
  in `tools`, not copied.

## Setting up and checking

```bash
npm install
tools/fetch_oidn.sh            # Open Image Denoise into src-tauri/oidn (not committed)
npm run tauri dev              # the app, with live reload
```

Before calling a change done, run what it touches:

| Check | Command |
| --- | --- |
| Engine and command line tests | `cargo test --release -p far-core -p far-cli` |
| App crate tests | `cargo test -p far-app` |
| 2008 renderer against the C++ original (macOS) | `npm run validate` |
| Whole workspace compiles | `cargo check --workspace` |
| Interface types | `npx tsc --noEmit` |

Render a scene to look at the result, which tests do not judge:

```bash
cargo build --release -p far-cli
target/release/far render assets/scenes/showcase.far -o out/test.png --enhanced --samples 64 --denoise --tone-mapping agx-punchy
```

`out/` is ignored by git; put scratch images there or in a temporary folder.

### Checking the interface without the app window

Agents usually cannot drive the app's native window. The interface also runs in a browser
against the Vite server of `npm run tauri dev` (http://localhost:5173), with the Tauri calls
mocked. Make an HTML page under `out/` that sets `window.__TAURI_INTERNALS__` before loading
`/src/main.tsx`, with an `invoke` that answers `startup` and `load_scene` and fails the rest, and
open it at `http://localhost:5173/out/<page>.html`. Delete the page afterwards. Say plainly when
something was only checked this way and not in the real app.

## What a finished change includes

1. Tests for new engine code, next to it (`#[cfg(test)]`) or in `crates/far-core/tests`.
2. The checks above, passing.
3. The README section for the feature, updated or new. A new scene field is documented with a
   JSON example.
4. An entry under **Unreleased** in `CHANGELOG.md` (see below).
5. `TODO.md` updated when the change completes or adds a planned item.
6. For a visible feature, an example scene built by a script in `tools`, and its screenshot in
   `tools/screenshots.sh` when it belongs in the README gallery.
7. In the settings panel, new settings as rows of `src/components/props.tsx` inside a section:
   `ScrubRow`, `ChoiceRow`, `CheckRow` or `ColorRow`. Rare ones go in the section's
   `advanced`. Give each number its usual range (`min`, `max`) and its real limits (`hard`, or
   `strict` when it has no meaning outside the range), and a `hint` for anything not obvious.
   Keep the app's dark glass style.

## Changelog

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Its sections
become the release notes on GitHub, so write for the people who use the app.

- Add each change under `## [Unreleased]` when it lands, in one of `### Added`, `### Changed`,
  `### Fixed`, `### Removed`, in that order.
- One bullet per change, saying what the user gets: "Save Scene writes the camera…", not
  "Refactor scene_save.rs". Group related items with a bold lead and nested bullets.
- Leave out changes nobody using the app would notice (internal refactors, tests, tooling),
  unless they change how the project is built or released.
- A fix to something already released says which versions had the problem.
- Never rewrite the section of a version already released, except to correct a mistake.

## Commits

- Commit and push only when the user asks. Releases and tags likewise.
- One topic per commit. The first line is a short summary in the imperative or as a title
  (under 72 characters); after a blank line, bullets with what changed and why.
- No attribution or co-author lines.
- Commit only the files of your own change. Check `git status` and `git diff` first: other
  agents may have work in progress in the same checkout (see below). Never commit `out/`,
  `target/`, `src-tauri/oidn/lib` or anything secret.
- The project works on `main` directly.

## Releases

Versions follow [Semantic Versioning](https://semver.org/). Pre-releases carry a label:
`3.0.0-beta.5`, then `3.0.0`.

1. **Changelog.** Rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD`, add a new empty
   `## [Unreleased]` above it, and update the compare links at the bottom of the file. The
   release workflow stops if the tag's version has no section.
2. **Version.** Set it in `Cargo.toml` (workspace), `package.json`, `package-lock.json` (both
   places near the top) and `src-tauri/tauri.conf.json`. Run `cargo check --workspace` so
   `Cargo.lock` follows.
3. **Checks.** Run all the checks above.
4. **Commit and tag.** Commit as "Release X.Y.Z", then:
   ```bash
   git push origin main
   git tag vX.Y.Z && git push origin vX.Y.Z
   ```
   The workflow stops if the tag and the app version differ. Never move or reuse a tag.
5. **Build.** `.github/workflows/release.yml` tests the engine, builds macOS (Apple Silicon and
   Intel) and Windows, signs the update bundles and makes a draft release whose notes come from
   the changelog. Follow it with `gh run watch`.
6. **Publish.** Check the draft (`gh release view vX.Y.Z`: three installers, the update
   bundles and `latest.json`), then publish it: `gh release edit vX.Y.Z --draft=false`. A
   version with a label is published as a pre-release.
7. **Update channel.** Publishing starts `.github/workflows/update-channel.yml`, which copies
   the release's `latest.json` to `beta.json` on the release tagged `updater`. Stable versions
   update from the latest release; pre-releases from `beta.json`. Check that it ran.

The updater only installs bundles signed with the project's key. The private key and its
password are the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; the public key is in `src-tauri/tauri.conf.json`. A new
key would cut off every installed copy from updates.

The builds are not signed with a developer certificate, so macOS and Windows ask before the
first launch; the release notes say how to open them.

## Several agents at once

The user often runs several agent sessions on this checkout at the same time.

- **Own your files.** Before editing, know which files your task owns. Do not edit files another
  session is working on; if you must, ask the user or that session first.
- **Small, exact edits.** In files others may also touch (`README.md`, `CHANGELOG.md`,
  `src/styles.css`, `src-tauri/src/main.rs`), change only the lines you need; never rewrite the
  whole file from an old copy.
- **Commit only your work**, and only when asked. If another session's changes are in the
  tree, leave them out of your commit or ask.
- **Do not stop or restart** the user's running `npm run tauri dev`, and do not start a second
  one: its port is fixed.
- **Releases and pushes** are the user's call; one session at a time does them.
- When a session reports back, check the diff before saying the work is done.
