# Fractional scaling regression

Rexia must keep the scale supplied by `wp_fractional_scale_v1`, even when the
compositor advertises an older `wl_surface` without `preferred_buffer_scale`.

The original failure happened under umbriel at 150%. An output Enter event
replaced the fractional scale with the output's rounded-up integer scale.
Rexia drew at 200%; the compositor downsampled it to 150%. This made braille
dots periodically wide or soft. It was not a difference in the font's
anti-aliasing settings.

## Automated check

```sh
nix develop --command cargo test --test fractional_scale
```

The test covers both scale protocols and the fallback when neither exists.
It imports the same policy called by the vendored Wayland Enter and Leave
handlers, rather than duplicating their condition.

To exercise the real event ordering with an installed umbriel executable:

```sh
REXIA_TEST_UMBRIEL="$(command -v umbriel)" nix develop --command \
  cargo test --test live_wayland_scale -- --nocapture
```

This launches an isolated headless compositor with a private runtime directory
and a Rexia window pointed at an absent test socket. It checks the Wayland trace
and buffer dimensions without contacting the user's server or changing their
desktop. Without `REXIA_TEST_UMBRIEL`, this opt-in test is skipped.

## Desktop check

1. Use an isolated umbriel instance at 150%, with a private Rex test server.
   Do not use the user's server or change their desktop configuration.
2. Open Rexia and Ghostty with DepartureMono Nerd Font at 11 pt.
3. Print several rows of `⣿` in white, yellow, blue and red in both terminals.
4. Capture both windows at native resolution after the layout settles. Compare
   dot interiors, edges, gaps and row boundaries, without resizing the image.
5. Repeat at 100%. Text and dots should remain sharp there too.

For a protocol-level check, launch the isolated Rexia with `WAYLAND_DEBUG=1`.
An 868 × 1260 logical window at 150% should attach a 1302 × 1890 buffer after
`preferred_scale(180)`. It must not subsequently call `set_buffer_scale(2)`
and replace that buffer with one sized 1736 × 2520.

The verified 150% reproduction used umbriel revision `380f6c1` and Ghostty
`1.4.1-dev+6d95777`. Before the fix, each colored sample had thousands of
different edge pixels. After the fix, each 401 × 27 sample had zero foreground
or edge-pixel differences; only the intentional background colors differed.
The user also confirmed the fix on their desktop.

Sway alone did not reproduce the bug: its newer surface protocol bypassed the
faulty fallback. Keep the old-surface compositor case in future checks.
