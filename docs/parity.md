# Parity with Rex for Mac

How rexia compares with Rex for Mac v0.1.0-preview.30 (embedded server preview.19), from
Superlogical's published action and settings references. rexia is checked against a
preview.19 server.

## Actions

Rex for Mac lists 116 actions. rexia implements 104 of them under the same names, so
init.lua bindings, `rex do` and `client.perform` reach rexia the same way. It also adds
four of its own: `client.sidebar.activity.toggle`, `.by_session` and `.by_priority`, and
`client.tab.mute.toggle` (Rex for Mac has Mute Tab only in the tab's menu).
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
| `client.pair_device`, `client.permissions` | macOS-specific or tied to Rex.app's installer. |

## Line-by-line audit

Every setting in the Mac settings reference, every Mac action, and every Mac changelog post
from preview.4 to preview.30 was checked against rexia's code. Actions: all 116 accounted
for (104 built, 12 excluded above). These settings and changelog features were missing
when checked, and are built in this order; each moves to its section once it's done:

| Gap | Source |
| --- | --- |
| Quick Look every link on screen, stepping with the arrow keys; files on remote hosts downloaded first; Ctrl-click a remote file link saves a copy under ~/Downloads/Rex/HOST and opens it | preview.28, 29 |
| Settings search, `rex://settings/...` links to a group or setting, reset buttons on changed settings, every simple setting as a palette row | preview.20, 25, 28 |

## Settings

| Mac section | rexia |
| --- | --- |
| Notifications › Program Activity | All of it: activity in tabs, idle processes, each process, desktop notifications, launcher badge (Unity LauncherEntry), sounds with volume, and the per-state tabs/notify/sound switches. |
| Notifications › System Notifications | Terminal notifications (OSC 9 and 777). Tailscale sign-in, device approval and failures, each with its switch: a desktop notification when built-in Tailscale comes to need a sign-in (clicking opens the sign-in link) or an admin's approval, taken back once it moves on, and a critical one when it stops or reports a new error. |
| Notifications › In-App Notifications | Notification position: click a spot in a picture of the window to float notices at the top or bottom, left, center or right. |
| Interface › Panes, Active Pane, Splits & Focus | Pane style, pane headers (always, hover, hidden), active pane highlight with its color (accent, terminal text, or one of the 16 terminal colors), opacity and animation (fade in, move from the previous pane, or instant), fade inactive panes, fade panes for inactive windows, focus follows mouse. Autobalance splits (on by default): when a pane opens or closes in the tab on screen, its splits even out; a divider you drag or move with the keyboard keeps its place on screen until Balance Splits, and one dropped near the middle of its split snaps there. |
| Interface › Appearance, Themes | Replaced by Noctalia: palette, corner radius, borders, shadows and the interface font follow its live config. |
| Interface › Toolbar, Tab behavior | Toolbar layout (above, or down the sidebar on the left or right side), toolbar behavior (always, on hover, or hidden until Reveal Toolbar; a toolbar that isn't always shown floats over the panes so they don't resize), and tabs filling the bar. With vertical tabs, the selected session's tabs are listed in the sidebar. |
| Interface › Tab icons, Window | Tab icons: show zero to five pane icons per tab (vertical tabs stop at three), focused pane first; use color always, for active panes, for active tabs, or never; shell icon theme (default artwork, terminal theme colors, or accent). Colored icons sit on a tile in their program's color from the terminal palette. Window: sidebar text size (small, medium or large; Linux has no system sidebar size to follow), and a gesture to toggle the sidebar (a two-finger touchpad swipe sideways, a swipe with Alt held, or off). Showing or hiding the sidebar always keeps the window's width, as tiling compositors expect. |
| Interface › Gestures | Use trackpad gestures: spread two fingers over a pane to zoom it, pinch to restore it, and swipe for the sidebar. Pinches need a compositor with pointer gestures (Wayland `zwp_pointer_gestures_v1`). |
| Interface › Padding | Padding X and Y, fill with program colors (off; automatic, which fills while a full-screen program runs along edges of one color; always, which extends every edge cell), and include pane header. |
| Terminal › Typography | Font family, size and line height (font style and weight aren't built yet, see the audit). Defaults come from Ghostty's config, then fontconfig. |
| Terminal › Cursor | Cursor style (block, bar, underline) for programs that don't choose one, force style, and blink speed (off, slow, normal, fast, or a custom interval). As in preview.30, a cursor blinks only when its program asks. When a pane attaches, a program's explicit steady block looks like the default, so it shows your style until the program sets the cursor again. Cursor shaders: a trail behind the focused cursor when it jumps two cells or more, in eight effects (Warp, Sweep, Comet, Blaze, Tapered, Rainbow, Gradient, Frozen), with a preview to click in. GPUI has no custom GPU shaders, so each effect is drawn as a shape with its own taper, gradient, glow and fade rather than as Rex's own shader code. |
| Terminal › Advanced Font | Character spacing, ligatures, which of bold and italic draw in their style, and fallback fonts. Font smoothing and forced faux bold and italic are left to the system's font rasterizer. |
| Terminal › Colors | Improve text contrast (text below a 3:1 contrast ratio against its background is lifted just enough) and force theme palette for text (program colors become the perceptually nearest theme color, then contrast is corrected). Display P3 doesn't apply on Linux. |
| Terminal › Selection | Copy on select. |
| Server › Client Info | Client label (`client.set_label`). Remote control is always on. |
| Server › Rex Server | Settings › Server shows whether the server is running, where, and for how long, with its version, build, platform, API revisions, process and session count. Compatibility compares the server's API revision with the revisions rexia speaks, and, for this computer's server, the installed `rex` with the running one: after an update it says the server is still the old version and offers Restart Server (also in the palette). Pending settings changes: web client origins changed since rexia last started the server, or a server rexia didn't start, show as pending until Restart Server applies them (rexia remembers which server process got which origins), and key bindings fetched from an older configuration or server process show as stale, with Reload Key Bindings. |
| Server › Remote Address, Tailscale | Remote Address lists the addresses other devices can use (network listeners and built-in Tailscale's HTTPS address), who each lets in, whether its certificate is ready, and copies them. Pairing a device by QR code is the Mac and iOS apps' and isn't built; add the address as a host instead. Tailscale turns built-in Tailscale on and off, shows its state as it changes (starting, needs signing in, needs approval, running as its name and account), opens or asks for the sign-in link, limits access to your account or lets tailnet members in, sets the preferred hostname and renames the device, takes tags and a control server before signing in and shows them after, and signs out. The server only takes these over its local socket, so they're shown read-only for remote hosts. Adding listeners such as `--extra-listen` stays a server start option. |
| Remote Hosts | HTTPS, WebSocket and `ssh://[USER@]HOST[:PORT][/SOCKET]` hosts, kept on the home server (`hosts.put`, `hosts.remove`) and shared with other clients. SSH hosts go through the system `ssh` (your keys, agent and `~/.ssh/config`) and `rex __internal dial-stdio` on the host, as the Rex CLI does; Settings › Client & Hosts › SSH picks the agent ssh asks for keys: ssh's default (`SSH_AUTH_SOCK`, or `IdentityAgent` in `~/.ssh/config`), an agent found on this computer (the session's, systemd's ssh-agent, GnuPG, GNOME Keyring, 1Password or Bitwarden), or a custom socket. ssh gets it as `IdentityAgent`, with `SSH_AUTH_SOCK` set to match, and the agent's status says whether it answers and how many keys it holds. ssh's questions come to rexia (it's ssh's `SSH_ASKPASS`) and show above whatever's open, Settings included: Connect or Don't Connect for a host key ssh hasn't seen, a hidden field for a password or key passphrase, and a notice for touching a security key. A declined question isn't asked again while rexia reconnects; the host shows Retry, which asks again and reconnects at once (any disconnected host has Retry). The panes of an ssh:// host share one ssh connection (`ControlMaster=auto`, open 60 s after its last use), so one answer covers them all. A host whose rex closes without answering (one too old to relay, first on its PATH) says so and what to check. tests/live_ssh_remote.rs reaches a real second machine through the system ssh when REXIA_TEST_SSH_HOST names one. Up and down arrows reorder the hosts in Settings and the sidebar; the order is rexia's own (`host_order`), since the server's host list has none. |
| Integrations › Coding Agents, Agent Behavior | Integrate with available agents installs or removes Rex's skill and session hooks with `rex skill setup` and `rex skill remove` on this computer, for Claude Code, Codex, OpenCode and Pi; each agent's row says whether it was found and whether its skill and hook are installed, from an older Rex, or held by another skill named rex, and an Update button appears when they're out of date. Let agents name sessions passes `--session-naming`. Install Rex for Agents (`client.agents.setup`) runs setup from the palette and says what changed. |
| Interface › Animation | Animation style: Snappy (120 ms, settling fast), Natural (260 ms, easing in and out) or Off. Panes glide to their new places when the layout changes, new panes fade in, the active border fades or moves at the same pace, and the sidebar slides in and out. Terminals keep their size while things move and resize once at the end. Closing panes don't animate. Linux has no system Reduce Motion setting rexia reads. |
| Terminal › Keyboard | Show key sequence hints: after a sequence's first key (such as a tmux-style prefix), the keys that can come next show, or with it off only the mode does. The Option key setting is macOS-only (Alt always sends Alt). |
| Terminal › Scrolling, Resizing | Smooth scrolling: touchpad scrolling moves history by pixels, drawing the text part of a line lower or higher between whole lines; mouse wheels still move three lines a notch. Smooth terminal resizing: when a pane changes size, its text reflows here on that frame without waiting for the server, including while panes glide during animations (the server is asked once they settle). |
| Terminal › Selection | Copy on select; selection foreground (the theme's, the accent, the program's own colors, or one of the 16 terminal colors) and background (the theme's, the accent, or a terminal color); rounded corners, which round only the selection's outer corners so touching rows stay joined; and a shine that sweeps across the selection to confirm each copy. |
| Terminal › Links | Ctrl+click opens web, mail and file links at once. Links that open other apps, such as ssh://, ask first (Cancel, Always Open, Open), always open, or never open; Always Open remembers the link's app and host (`ssh://build`), and Trusted links lists them with Remove. Links that carry code, such as `javascript:` and `data:`, never open. |
| Terminal › Integrated Path Browser | Show recently used paths: the last paths chosen in Insert Path and Go to Directory come first (up to 8, files only in Insert Path), kept per host so every session on it shares them. |
| Terminal › Titles, Shell | Prefer generated titles names panes and tabs after the running program and its directory (`nvim · rexia`) instead of titles programs set. Startup command: a program alone starts as the shell (so a shell such as fish starts as a login shell); with arguments it runs directly. It applies to new terminals on local hosts only, so remote hosts keep their own shells. Empty runs the login shell. |
| Developer › Local API, Web Client Origins | Settings › Developer shows the local server's API address with Copy. Web client origins: add a browser origin or wildcard subdomain (`https://*.example.com`), see the list, and remove entries. Rex servers take origins only as a start option, so rexia passes them as `--websocket-origin-pattern` when it starts the server with Restart Server; a server started some other way (such as by systemd) needs them in its own command line. |
| Developer › Rex Installation | Location: the rex on PATH, or your own build, given as an executable or a source checkout with one in bin, build, dist or out. Installation status checks it runs and is rex, and shows its version, commit, build time and SHA-256, each with Copy. Restart Server, agent setup (`rex skill`) and the installed-version check all run the chosen rex. rexia doesn't build a checkout itself. Developer Mode and App Version are macOS app options. |
| Developer › Experimental | Terminal anchoring: Top, or Bottom, which moves the text down by the part of a line left over so it sits on the pane's bottom edge. New quick switcher: Go to Anything lists every program reporting a status on its own row (several in one pane each get one, most urgent first), then every session and pane. Update experiments are macOS app options. |
| App Icon, Launch | Not applicable: macOS app options. |

## Behavior

Working:

- Fractional Wayland scaling stays at the compositor's requested scale, including on
  older `wl_surface` versions. At 150%, terminal pixels are rendered at 150%, not at
  200% and then shrunk; braille retains sharp, evenly spaced edges under umbriel.

- Typing a host's address in the command palette (`alice@studio`, `ssh://…`, `https://…`,
  `studio.tail1234.ts.net`, an IP, with a port or not) offers Add Host as the last row; it
  opens Add Remote Host with the address and a name from it filled in. Single words and
  hosts already added get no offer.

- An ssh host whose key ssh doesn't trust, or that refuses the login, three times in a row
  stops retrying; the host's status says what to check (`ssh-keygen -R HOST` for a changed
  key; the account, authorized_keys, ssh-agent and password for a login) until Retry.

- Braille draws with Ghostty's dot size and spacing: spare pixels widen the margins and the
  gaps between dots before the dots grow, so btop's graphs look as they do in Ghostty.
- Terminal cells round to the nearest device pixel, as Ghostty's do, so a font's cell is
  the same size in both (DepartureMono 11 pt at 1.5×: 14 × 28) and braille stays evenly
  spaced across rows.

- The pointer hides while typing in a terminal, for every key the terminal takes (Return,
  Backspace, arrows and Ctrl combinations too, not only characters), until the mouse moves.

- Fade inactive panes dims a pane's content as one picture, under a sheet of the terminal's
  background, so half-block pictures (fastfetch logos) keep their colors apart when faded.

- With focus follows mouse, the pane under the pointer takes keyboard focus as well as the
  highlight: what you type goes to the pane the pointer moved onto.

- Settings › Terminal › Font style lists the terminal font's upright faces from fontconfig
  (DM Mono: Light, Regular, Medium); choosing one sets Font weight, which steps 100 to 900
  for fonts with more weights. Bold text draws 300 heavier than the chosen weight, up to 900.

- Hold the Go to Tab modifier (Alt by default, where Rex for Mac uses Command) to see
  each tab's key on it, in the tab bar and the sidebar. The keys come from the keymap, so a
  configuration that rebinds them shows its own keys, or none. They wait 400 ms so Alt
  combinations typed in a terminal don't flash them, and go away at any other key or when
  the modifier is let go.

- Double-click to rename: the selected tab, in the tab bar or the sidebar, and a pane's
  title in its header. A double-click on a tab that wasn't selected only selects it, as
  in Rex for Mac.

- What floats over the panes keeps its clicks: Settings, dialogs and lists, ssh's
  questions, the right-click menu, notices, upload progress, hover cards and a floating
  toolbar block the mouse for what's under them, so a click inside one never focuses the
  terminal behind it. A click on the dimmed backdrop still closes Settings.

- Settings › Shortcuts (Rex for Mac's Shortcuts page, from preview.8): every action,
  rexia's and the server's own, with its keys and where they come from (Default, Yours,
  or the init.lua line, both when init.lua binds it too), filtered as you type. Change…
  records a new key and comes back to the page; Remove leaves an action without a key;
  Reset puts the default back, asking first when another action has taken that key and
  taking it back from it. Keys two actions share are called out, the server's keymap
  problems are listed, and Copy as Lua gives your changes as `rex.unbind`/`rex.bind`
  lines for init.lua. Tab now steps through all eight Settings sections.

- Session rows in the sidebar: right-click for Rename Session…, Move Up, Move Down and
  Close Session…; double-click to rename; a close button under the pointer (closing a
  session always asks, naming busy programs); and drag a row up or down to reorder its
  host's sessions, with a line where it will land. The order is the server's
  (`session.move`), so every client shows it.

- Sidebar tab rows show a close button under the pointer (closing asks first when the tab
  is busy), and a tab's hover card steps aside while its right-click menu is open.

- The sidebar's width: drag its edge beside the panes (8 pixels across the border, with a
  line in the accent color under the pointer and while dragging) to resize it (180 to 480 pixels; on
  the right side, dragging left widens it). The width is kept in settings when the drag
  ends, so it's remembered across launches, and a double-click on the edge goes back to
  260.

- Dragging: a tab dragged in the tab bar or the sidebar moves to where the bar between
  tabs shows. A pane picked up by its header, or with Ctrl held anywhere over it (Rex for
  Mac's ⌘-drag), drops beside another pane on the side that lights up, onto another tab to
  move into it, or between tabs (or after the last) to become a tab of its own. A drag
  starts once the pointer moves a few pixels, carries the tab's or pane's title, and
  Escape cancels it. Ctrl-click still opens a link, on release over the same cell, so a
  Ctrl-drag that starts on one doesn't open it.

- Mute Tab: right-clicking a tab, in the tab bar or the sidebar, opens its menu (Rename
  Tab, Mute Tab, Close Other Tabs, Close Tab). A muted tab shows a crossed-out speaker and
  keeps its status mark, but its programs send no desktop notifications (status reports or
  their own OSC 9 and 777), play no sounds and add nothing to the launcher badge. Mutes are
  saved with rexia's settings, so they last across launches, and they hold for tabs in
  sessions that aren't on screen: rexia asks the server where those panes sit. Mute or
  Unmute Tab is also in the palette (`client.tab.mute.toggle`).

- Hover cards: resting the pointer on a tab, in the tab bar or the sidebar, shows a card
  with each of its panes: its program and title, what each program reporting in it says
  (sub-agents on their own lines, with their state, message and progress), a terminal
  progress bar, and its directory. Clicking a pane in the card goes to it, switching tabs.
  Resting on a pane header's title shows that pane's card. Cards follow the panes while
  they're open, and stay open while the pointer moves into them.

- Tooltips on buttons: resting the pointer on a button for half a second names what it
  does, with the keys bound to it in the keymap in use (so a shortcut changed with Change
  Shortcut or in init.lua shows), in Noctalia's popup style. The toolbar, tab, sidebar,
  pane header, Activity, Find and notice buttons all have one.

- The terminal grid sits on whole device pixels, as Ghostty's does: a cell is a whole
  number of device pixels wide (rounded) and tall (rounded up), and the grid starts on a
  device pixel, so every glyph does too and pixel fonts such as DepartureMono stay crisp.

- Closing busy work asks first: closing a pane or tab, or the other panes or tabs beside
  one (from the actions, the tab's close button or the pane header's), asks when a program
  in it is working, needs attention, failed, or finished without being seen. The question
  names each program and what it's doing; clicking one goes to its pane, Enter closes and
  Escape keeps them. Closing a session always asks, and names its busy programs too.

- Braille (U+2800–28FF) and block elements (U+2580–259F) are drawn by rexia rather than
  taken from a font, as Ghostty does: braille as square dots on a 2 × 4 grid sized to the
  cell, blocks and shades as exact fills, on whole device pixels. Fonts such as
  DepartureMono have no braille, and the fallback font's dots (some draw the unset ones
  hollow) turned btop's graphs into hatching.

- Pasting and dropping. A paste that would run as it's pasted (a line break when the
  program hasn't turned on bracketed paste, or anything that would end a bracketed paste)
  asks first, showing the text, with Cancel as the default. Files copied in a file manager
  paste as their paths, quoted for the shell; an image on the clipboard pastes as a file
  only you can read (under `$XDG_RUNTIME_DIR/rexia-UID/pastes`). Files dragged onto a pane
  type their paths, even straight after typing (GPUI's own drop handling misses that case).
  For a terminal on another machine, dropped and pasted files are uploaded there
  (`upload_begin` and a `rex.data.bytes.v1` stream per file, as one batch) and the terminal
  types the paths of its copies; an upload that takes a moment shows its progress with
  Cancel, and one the terminal refuses, such as a file over its 100 MiB limit, says why.

- The terminal's context menu: right-click a pane, or Shift+right-click when its program
  uses the mouse. Copy (only with a selection), Paste and Select All; then what's under the
  pointer: a web link (Open Link, Copy Link, Quick Look) or a file (Open File, Copy Path,
  Show in Files through the file manager's `org.freedesktop.FileManager1`, or its folder
  without one, and Quick Look); then Split Right and Down, Zoom, Float, Clear Terminal and
  Reset Terminal (the server's block actions), Copy Pane ID and Close Pane. Items show their
  shortcuts; the arrow keys and Enter choose, and Escape or a click elsewhere closes it.

- Quick Look (`client.quick_look`, ctrl+shift+y) previews what's under the pointer in the
  focused pane: a program's link first, else a path or URL in the text (a `:line:col`
  suffix is dropped, and relative paths start from the pane's directory). Files go to
  GNOME's previewer (Sushi, `org.gnome.NautilusPreviewer` over D-Bus); without it, and for
  web links, the desktop's opener (`xdg-open`) shows them. Files on a remote host aren't
  previewed.

- Open Onboarding Session… (`client.onboarding`) opens a session with the server's
  onboarding block (`com.superlogical.onboarding`), or shows the one already open. The
  server keeps its step, so every client showing it stays together; rexia draws its own
  Linux steps: welcome, how rexia looks (tabs and pane spacing), keys (where init.lua is,
  Change Shortcut), coding agents (Install Rex for Agents), and done (close the session).
  It doesn't migrate other terminals' settings the way Rex for Mac's walkthrough does.

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
