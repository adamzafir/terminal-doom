# TERMINAL DOOM

A complete, original Doom-inspired first-person shooter rendered entirely with
terminal characters. It uses a custom Rust raycaster and gameplay engine—no
existing Doom engine, source port, or game assets.

## Install

The installed command is always `doom`.

### Homebrew

```sh
brew install --cask --no-quarantine adamzafir/terminal-doom/terminal-doom
doom
```

### npm

Rust must be installed because the npm package builds the small native
executable for your platform.

```sh
npm install -g https://github.com/adamzafir/terminal-doom/archive/refs/tags/v0.1.0.tar.gz
doom
```

### Build from source

```sh
cargo install --path .
doom
```

For the intended view, use a terminal window of at least 80×28 characters.

## Controls

| Key | Action |
|---|---|
| `W` / `S` | Move forward / backward |
| `A` / `D` | Turn left / right |
| `Z` / `C` | Strafe left / right |
| `←` / `→` | Turn (alternate) |
| `Space` | Fire |
| `E` | Open a door / use the exit |
| `1` / `2` / `3` | Pistol / shotgun / chaingun |
| `M` | Toggle automap |
| `H` | Toggle help |
| `P` or `Esc` | Pause |
| `Q` | Quit |

Reach each marked exit. Keys open locked blast doors; weapons, ammunition,
health, and armor can be collected along the way.

## Notes

- The view, sprites, HUD, maps, and weapon art are generated from text.
- Terminal size is detected dynamically.
- The terminal is restored if the game exits normally or encounters an error.
- This is an original homage and does not include copyrighted Doom data.
