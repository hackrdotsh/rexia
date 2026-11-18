# rexia

A native Linux client for [Rex](https://www.superlogical.com/rex), built on
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and themed by
[Noctalia](https://noctalia.dev). It attaches to Rex servers (API revisions 8–14,
current with Rex v0.1.0-preview.19) and follows Noctalia's palette, corner
radius, borders, and fonts.

rexia is unofficial and isn't affiliated with Superlogical.

See [docs/parity.md](docs/parity.md) for how it compares with Rex for Mac.

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

### Privileged local ports

If Linux denies a forward's local bind, Rexia offers a free port. To also
authorize the requested port, use a system installation of the bind helper and
polkit policy, with a polkit authentication agent running in your desktop session.
The Nix package includes both; install it in the NixOS system configuration
(`environment.systemPackages`, with `security.polkit.enable = true`) so polkit
can see its policy. A per-user package or `nix run` alone doesn't register it.

For other Linux packages, install `rexia-bind-port` to
`/usr/libexec/rexia-bind-port` and `resources/dev.hackr.rexia.bind-port.policy`
to `/usr/share/polkit-1/actions/`. Both must be system-owned and not writable
by ordinary users. The helper must not be setuid; polkit authorizes each bind.
