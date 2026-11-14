# Parity with Rex for Mac

How rexia compares with Rex for Mac v0.1.0-preview.30 (embedded server preview.19), from
Superlogical's published action and settings references. rexia is checked against a
preview.19 server.

## Actions

Rex for Mac lists 116 actions. rexia implements 102 of them under the same names, so
init.lua bindings, `rex do` and `client.perform` reach rexia the same way. It also adds
three of its own: `client.sidebar.activity.toggle`, `.by_session` and `.by_priority`.
Preview.30's Enable Cursor Shaders and Change Cursor Shader aren't in the published action
list yet, so rexia names them `client.cursor_shader.toggle` and `client.cursor_shader.change`
(with an optional `shader`, such as `{"shader": "blaze"}`).
Keys follow Linux conventions: where Mac uses `cmd`, rexia uses `ctrl+shift` (Ghostty's
convention), and `ctrl` alone for font size and Settings (`ctrl+,`).

Implemented by area:

- **Panes (39):** split in every direction, focus and move by direction, resize, balance,
  zoom, float, minimize floating panes, move to a tab or a new tab, rename, copy, paste,
  select all, send key, go to anything, go to directory, insert path, view activity.
- **Sessions (11):** new, close, rename, select, next and previous, switch, expand and
  collapse, refresh.
- **Client (51):** windows (new, close, move a tab to its own, toggle visibility), send
  key to server, change shortcut, restart server, command palette, find, the sidebar's
  tab filter, toolbar layout and reveals, tabs (new, close, close others, go to, move,
  rename, next and previous), modes, prompt jumps, page scrolling, font size, sidebar and
  activity view, hosts (add, remove, switch), settings, diagnostics, config path and
  reload, version and debug copy, notices, full screen.

Not implemented yet:

| Action | Notes |
| --- | --- |
| `client.theme.change`, `client.theme.import`, `client.appearance.switch_mode` | Not planned: colors follow Noctalia. |
| `client.update.*` (7) | Not applicable: rexia ships through Nix. |
| `client.onboarding`, `client.pair_device`, `client.permissions`, `client.quick_look` | macOS-specific or tied to Rex.app's installer. |

## Settings

| Mac section | rexia |
| --- | --- |
| Notifications › Program Activity | All of it: activity in tabs, idle processes, each process, desktop notifications, launcher badge (Unity LauncherEntry), sounds with volume, and the per-state tabs/notify/sound switches. |
| Notifications › System Notifications | Terminal notifications (OSC 9 and 777). Tailscale notices aren't applicable. |
| Notifications › In-App Notifications | Notification position: click a spot in a picture of the window to float notices at the top or bottom, left, center or right. |
| Interface › Panes, Active Pane, Splits & Focus | Pane style, pane headers (always, hover, hidden), active pane highlight with its color (accent, terminal text, or one of the 16 terminal colors), opacity and animation (fade in, move from the previous pane, or instant), fade inactive panes, fade panes for inactive windows, focus follows mouse. Autobalancing splits is the server's. |
| Interface › Appearance, Themes | Replaced by Noctalia: palette, corner radius, borders, shadows and the interface font follow its live config. |
| Interface › Toolbar, Tab behavior | Toolbar layout (above, or down the sidebar on the left or right side), toolbar behavior (always, on hover, or hidden until Reveal Toolbar; a toolbar that isn't always shown floats over the panes so they don't resize), and tabs filling the bar. With vertical tabs, the selected session's tabs are listed in the sidebar. |
| Interface › Tab icons, Window | Tab icons: show zero to five pane icons per tab (vertical tabs stop at three), focused pane first; use color always, for active panes, for active tabs, or never; shell icon theme (default artwork, terminal theme colors, or accent). Colored icons sit on a tile in their program's color from the terminal palette. Window: sidebar text size (small, medium or large; Linux has no system sidebar size to follow), and a gesture to toggle the sidebar (a two-finger touchpad swipe sideways, a swipe with Alt held, or off). Showing or hiding the sidebar always keeps the window's width, as tiling compositors expect. |
| Interface › Gestures | Use trackpad gestures: spread two fingers over a pane to zoom it, pinch to restore it, and swipe for the sidebar. Pinches need a compositor with pointer gestures (Wayland `zwp_pointer_gestures_v1`). |
| Interface › Padding | Padding X and Y, fill with program colors (off; automatic, which fills while a full-screen program runs along edges of one color; always, which extends every edge cell), and include pane header. |
| Terminal › Typography | Font family, size and line height. Defaults come from Ghostty's config, then fontconfig. |
| Terminal › Cursor | Cursor style (block, bar, underline) for programs that don't choose one, force style, and blink speed (off, slow, normal, fast, or a custom interval). As in preview.30, a cursor blinks only when its program asks. When a pane attaches, a program's explicit steady block looks like the default, so it shows your style until the program sets the cursor again. Cursor shaders: a trail behind the focused cursor when it jumps two cells or more, in eight effects (Warp, Sweep, Comet, Blaze, Tapered, Rainbow, Gradient, Frozen), with a preview to click in. GPUI has no custom GPU shaders, so each effect is drawn as a shape with its own taper, gradient, glow and fade rather than as Rex's own shader code. |
| Terminal › Advanced Font | Character spacing, ligatures, which of bold and italic draw in their style, and fallback fonts. Font smoothing and forced faux bold and italic are left to the system's font rasterizer. |
| Terminal › Colors | Improve text contrast (text below a 3:1 contrast ratio against its background is lifted just enough) and force theme palette for text (program colors become the perceptually nearest theme color, then contrast is corrected). Display P3 doesn't apply on Linux. |
| Terminal › Selection | Copy on select. |
| Server › Client Info | Client label (`client.set_label`). Remote control is always on. |
| Server › Rex Server | Settings › Server shows whether the server is running, where, and for how long, with its version, build, platform, API revisions, process and session count. Compatibility compares the server's API revision with the revisions rexia speaks, and, for this computer's server, the installed `rex` with the running one: after an update it says the server is still the old version and offers Restart Server (also in the palette). |
| Server › Remote Address, Tailscale | Remote Address lists the addresses other devices can use (network listeners and built-in Tailscale's HTTPS address), who each lets in, whether its certificate is ready, and copies them. Pairing a device by QR code is the Mac and iOS apps' and isn't built; add the address as a host instead. Tailscale turns built-in Tailscale on and off, shows its state as it changes (starting, needs signing in, needs approval, running as its name and account), opens or asks for the sign-in link, limits access to your account or lets tailnet members in, sets the preferred hostname and renames the device, takes tags and a control server before signing in and shows them after, and signs out. The server only takes these over its local socket, so they're shown read-only for remote hosts. Adding listeners such as `--extra-listen` stays a server start option. |
| Remote Hosts | HTTPS, WebSocket and `ssh://[USER@]HOST[:PORT][/SOCKET]` hosts, kept on the home server (`hosts.put`, `hosts.remove`) and shared with other clients. SSH hosts go through the system `ssh` (your keys, agent and `~/.ssh/config`) and `rex __internal dial-stdio` on the host, as the Rex CLI does; Settings › Client & Hosts › SSH picks the agent ssh asks for keys: ssh's default (`SSH_AUTH_SOCK`, or `IdentityAgent` in `~/.ssh/config`), an agent found on this computer (the session's, systemd's ssh-agent, GnuPG, GNOME Keyring, 1Password or Bitwarden), or a custom socket. ssh gets it as `IdentityAgent`, with `SSH_AUTH_SOCK` set to match, and the agent's status says whether it answers and how many keys it holds. |
| Integrations › Coding Agents, Agent Behavior | Integrate with available agents installs or removes Rex's skill and session hooks with `rex skill setup` and `rex skill remove` on this computer, for Claude Code, Codex, OpenCode and Pi; each agent's row says whether it was found and whether its skill and hook are installed, from an older Rex, or held by another skill named rex, and an Update button appears when they're out of date. Let agents name sessions passes `--session-naming`. Install Rex for Agents (`client.agents.setup`) runs setup from the palette and says what changed. |
| Interface › Animation | Animation style: Snappy (120 ms, settling fast), Natural (260 ms, easing in and out) or Off. Panes glide to their new places when the layout changes, new panes fade in, the active border fades or moves at the same pace, and the sidebar slides in and out. Terminals keep their size while things move and resize once at the end. Closing panes don't animate. Linux has no system Reduce Motion setting rexia reads. |
| Terminal › Keyboard | Show key sequence hints: after a sequence's first key (such as a tmux-style prefix), the keys that can come next show, or with it off only the mode does. The Option key setting is macOS-only (Alt always sends Alt). |
| Terminal › Scrolling, Resizing | Smooth scrolling: touchpad scrolling moves history by pixels, drawing the text part of a line lower or higher between whole lines; mouse wheels still move three lines a notch. Not built: smooth resizing (reflowing text locally while a pane resizes). |
| Terminal › Selection | Copy on select; selection foreground (the theme's, the accent, the program's own colors, or one of the 16 terminal colors) and background (the theme's, the accent, or a terminal color); rounded corners, which round only the selection's outer corners so touching rows stay joined; and a shine that sweeps across the selection to confirm each copy. |
| Terminal › Links | Ctrl+click opens web, mail and file links at once. Links that open other apps, such as ssh://, ask first (Cancel, Always Open, Open), always open, or never open; Always Open remembers the link's app and host (`ssh://build`), and Trusted links lists them with Remove. Links that carry code, such as `javascript:` and `data:`, never open. |
| Terminal › Integrated Path Browser | Show recently used paths: the last paths chosen in Insert Path and Go to Directory come first (up to 8, files only in Insert Path), kept per host so every session on it shares them. |
| Terminal › Titles, Shell | Prefer generated titles names panes and tabs after the running program and its directory (`nvim · rexia`) instead of titles programs set. Startup command: a program alone starts as the shell (so a shell such as fish starts as a login shell); with arguments it runs directly. It applies to new terminals on local hosts only, so remote hosts keep their own shells. Empty runs the login shell. |
| Developer › Local API, Web Client Origins, Rex Installation | Not built: showing the local API URL, managing web client origins, and choosing the server installation. Developer Mode, App Version and update experiments are macOS app options. |
| Developer › Experimental | Not built: terminal anchoring and the new quick switcher. |
| App Icon, Launch | Not applicable: macOS app options. |

## Behavior

Working:

- Panes laid out exactly as the server arranges them, with dividers, floating layers,
  zoom and minimized layers.
- Terminal rendering through libghostty-vt: colors, styles, wide characters, scrollback,
  selection, search, mouse reporting, bracketed paste, OSC 52 clipboard writes, and
  OSC 8 links opened with ctrl+click (web, mail and file links only).
- Kitty graphics: images sent directly (RGB, RGBA, grayscale or PNG, compressed or not,
  in chunks), and for servers on this machine through files, temporary files in `/tmp`
  and shared memory; placements with cell sizes, offsets, source rectangles and z-index
  in all three layers (below backgrounds, below text, above text); Unicode placeholders
  (virtual placements, as tmux-friendly tools use); placements relative to other
  placements; animations; deletion; and images already on screen when rexia attaches.
  The Rex server answers programs' queries, so programs see support. Images scale from
  the server's cell size to rexia's. For a remote server, files, temporary files and
  shared memory name things on that machine, so only directly sent images show.
- The server's keymap, including modes, sequences and init.lua bindings. rexia's
  defaults apply until the server answers.
- Remote control: other clients' `client.perform` requests run in rexia and are answered
  with `client.reply`.
- Program status (OSC 7501): badges on tabs, sessions and pane headers; the activity list;
  desktop notifications for panes out of view that open the pane when clicked; sounds
  from the desktop's sound theme.
- Input methods (IBus, Fcitx and others through Wayland or XIM): composing text shows
  underlined at the cursor, the candidate window opens there, and committed text goes to
  the program as typed text.
- Several windows, each its own Rex client. New Window opens one with a new session;
  Move Tab to New Window shows a tab in a window of its own. The server keeps one active
  tab per session, so windows sharing a session each keep the tab they show. Preferences
  changed in one window reach the others. Only the first window sends desktop
  notifications, so they aren't doubled.
- Change Shortcut gives one of rexia's actions a new key (or none). It's saved in rexia's
  settings and sent as rexia's own default layer, so bindings in init.lua still win.
- Restart Server asks first, stops this computer's server, and starts it again with
  `rex server start`; sessions come back and rexia reconnects.
- Toggle Visibility brings rexia to the front, or minimizes it when it's in front. Wayland
  gives apps no global shortcuts, so bind it in the compositor, for example in niri:
  `Mod+Grave { spawn "rex" "do" "-C" "rexia" "client.visibility.toggle"; }` (after setting
  rexia's client label to `rexia` in Settings › Client & Hosts).
- Several hosts in one sidebar, streaming only the host on screen, with reconnects.
- Servers that only speak API revisions below 8 are refused, with a message to update the
  server.
