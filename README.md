# ztrx

**ztrx** runs the Xbox Series build of the game natively on Windows PCs with
NVIDIA graphics cards, from game files you supply yourself.

**This repository contains no game files or assets.** You need your own copy
of the game. Not affiliated with or endorsed by its developer or publisher.

## Status (2026-11-25)

Tested on Windows 11 with an **NVIDIA GeForce RTX 3050** (8 GB):

| | |
|---|---|
| Boot, first-boot screens, intro, opening credits | works |
| Prologue | playable from the cell to the boat chase: cell, cafeteria, robbery, police chase (wanted level, minimap), boat, binoculars |
| Stability | 1 h 30 min+ in one session without a crash |
| Frame rate | 1-3 fps in the world (the first run is slower while shaders translate; they are cached after that) |
| Known problems | anything in shadow is black and bright scenes overexpose (the game's ray-traced lighting does not run yet); some blocky artifacts; boats sit low in the water |

## Requirements

- Windows 10/11 x64, a CPU with AVX2
- A Direct3D 12 GPU with shader model 6.6 and mesh shaders (NVIDIA RTX 20 series or newer)
- Rust (the version in `rust-toolchain.toml`)
- Several GB of free Windows commit memory (raise the paging file if `crashes.log` reports error 1455)

## Build

```powershell
cargo build --release -p victory
```

## Play

```powershell
powershell -ExecutionPolicy Bypass -File scripts\play.ps1 -Game "<your game folder>\<the game's exe>"
```

`scripts\play.ps1` keeps the shader cache and saves in folders you choose
(`-Cache`, `-Data`) and presses no buttons for you. Controller: XInput pads
(a DualShock/DualSense through DS4Windows works). With no pad connected the
keyboard acts as one while the game window has focus.

## Faster settings (optional, off by default)

`VICTORY_RELAY=2`, or `VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1` cut frame
time on the test machines. Set them in PowerShell before running the script.

## Debugging switches

| Switch | Effect |
|---|---|
| `VICTORY_CAPTURE=<dir>` + `VICTORY_CAPTURE_FRAMES=<list>` | save those frames as PNG (diagnostics build) |
| `VICTORY_KEYS=30:Enter,35:X` | press keys at those seconds (diagnostics build) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, report the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | shorter per-frame wait for pipelines still being built |

Diagnostics build: `cargo build --profile diag -p victory --features diagnostics`

## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).

## Shader library

`victory-shader-library` is a reusable Rust crate that stores translated host
shaders and their binding metadata. The renderer uses it automatically under
`<cache directory>/shaders`; the play script defaults to
`%LOCALAPPDATA%\ztrx\cache\shaders`. Use `-Cache` to choose another cache root.
Entries are written atomically, reused across runs of the same build/settings,
and regenerated if unreadable. A changed executable or translation setting uses
new entries. No game assets or generated shaders belong in the repository.

Other Rust callers can open `ShaderLibrary`, call `get_or_translate` with a
32-byte translator/settings fingerprint, source bytes and a translation closure,
and read per-instance hit/miss totals with `counts()`.

### Compile shaders before play

The play script now runs `victory precompile <game directory>` and waits for
success before starting the game. It scans files with bounded memory for raw
DXBC containers and packed SGD5 records, including records inside large archives.
It deduplicates containers and fills the same shader library used during play.
Progress and failures are saved to `%TEMP%\ztrx-precompile.log`.
A translation failure or no shaders found stops the launch. Explicitly pass
`-SkipPrecompile` to the play script to bypass this step.

This covers discoverable shaders, not encrypted/compressed archive contents that
the scanner cannot decode, dynamically generated shaders, or driver pipeline
creation. Those can still compile at runtime. A full scan reads the game files
on each launch; successful translations are reused for the same executable and
GPU translation settings. Rebuilding invalidates translations.
