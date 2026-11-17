# GPUI Linux fractional-scale patch

Source: the `gpui-pre-linux` 0.3.12 crates.io package. Its Apache license is
preserved in `LICENSE-APACHE`.

`src/linux/wayland/window.rs` changes the output Enter/Leave fallback to use
integer output scaling only when neither fractional scaling nor
`wl_surface.preferred_buffer_scale` is available. Previously, on a compositor
advertising an older `wl_surface`, Enter could overwrite an already-received
fractional scale: a 150% window rendered at 200% and was downsampled. This made
braille and other pixel-aligned terminal content blurry under umbriel.

Keep the remaining upstream code unchanged. Remove this override when a GPUI
release includes the fix. Run its policy test with:

```sh
cargo test --test fractional_scale
```

Verification: an isolated umbriel compositor at 150%, DepartureMono Nerd Font
11 pt, and identical `⣿` rows in Rexia and Ghostty 1.4.1-dev+6d95777. Before the
patch, Wayland tracing showed `preferred_scale(180)` followed by
`set_buffer_scale(2)` and a 1736 × 2520 buffer for an 868 × 1260 window. After
the patch, the buffer stays 1302 × 1890. White, yellow, blue and red rows match
Ghostty's foreground and edge pixels in each 401 × 27 sample. Sway was not a
sufficient reproduction: its newer surface version avoided the faulty fallback.
