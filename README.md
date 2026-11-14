# rexia

A native Linux client for [Rex](https://www.superlogical.com/rex), built on
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and themed by
[Noctalia](https://noctalia.dev). It attaches to Rex servers (API revisions 8–14,
current with Rex v0.1.0-preview.19) and follows Noctalia's palette, corner
radius, borders, and fonts.

rexia is unofficial and isn't affiliated with Superlogical.

## Requirements

- Nix with flakes, and direnv with nix-direnv for automatic shell loading.
- A Wayland or X11 session with Vulkan.
- A running Rex server. rexia never starts or stops one.

## Development

```sh
direnv allow          # once per checkout; loads the dev shell from flake.nix
nix run .#dev         # run from source
```

| Command | Purpose |
| --- | --- |
| `nix run .` | Build and run the packaged client |
| `nix build` | Build the package into `./result` |
| `nix run .#dev -- ARGS` | Run from source |
| `nix run .#build` | Release build with Cargo |
| `nix run .#test -- FILTER` | Run the test suite |
| `nix run .#lint` | Clippy with warnings denied |
| `nix run .#fmt` / `.#fmt-check` | Format or check Rust sources |
| `nix run .#check` | Formatting, lint, and tests |
| `nix fmt` | Format Nix files |
| `nix flake check` | Package build with tests, plus format checks |

Development commands run from the checkout root inside the dev shell. The shell
pins Zig 0.17 and the libghostty-vt source, so builds don't download anything.
