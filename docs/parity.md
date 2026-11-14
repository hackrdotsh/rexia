# Parity with Rex for Mac

How rexia compares with Rex for Mac v0.1.0-preview.30 (embedded server preview.19), from
Superlogical's published action and settings references. rexia is checked against a
preview.19 server.

## Actions

Rex for Mac lists 116 actions. rexia implements 91 of them under the same names, so
init.lua bindings, `rex do` and `client.perform` reach rexia the same way. It also adds
three of its own: `client.sidebar.activity.toggle`, `.by_session` and `.by_priority`.
Keys follow Linux conventions: where Mac uses `cmd`, rexia uses `ctrl+shift` (Ghostty's
convention), and `ctrl` alone for font size and Settings (`ctrl+,`).

Implemented by area:

- **Panes (39):** split in every direction, focus and move by direction, resize, balance,
  zoom, float, minimize floating panes, move to a tab or a new tab, rename, copy, paste,
  select all, send key, go to anything, go to directory, insert path, view activity.
- **Sessions (11):** new, close, rename, select, next and previous, switch, expand and
  collapse, refresh.
- **Client (41):** command palette, find, the sidebar's tab filter, tabs (new, close, close others, go to, move,
  rename, next and previous), modes, prompt jumps, page scrolling, font size, sidebar and
  activity view, hosts (add, remove, switch), settings, diagnostics, config path and
  reload, version and debug copy, notices, full screen.

Not implemented yet:

| Action | Notes |
| --- | --- |
| `client.window.new`, `client.window.close`, `client.tab.move_to_new_window` | rexia has one window. |
| `client.key.forward` as a palette entry | Works as a binding (keys reach Lua scripts), but isn't listed. |
| `client.toolbar.reveal`, `client.pane_header.reveal`, `client.tab_style.toggle` | Toolbar layouts and vertical tabs aren't built. |
| `client.shortcuts.edit` | Rebind keys in init.lua instead. |
| `client.server.restart` | rexia never starts or stops a server. |
| `client.visibility.toggle` | Needs a global shortcut; compositors bind this themselves. |
| `client.theme.change`, `client.theme.import`, `client.appearance.switch_mode` | Not planned: colors follow Noctalia. |
| `client.update.*` (7) | Not applicable: rexia ships through Nix. |
| `client.agents.setup`, `client.onboarding`, `client.pair_device`, `client.permissions`, `client.quick_look` | macOS-specific or tied to Rex.app's installer. |

## Settings

| Mac section | rexia |
| --- | --- |
| Notifications › Program Activity | All of it: activity in tabs, idle processes, each process, desktop notifications, launcher badge (Unity LauncherEntry), sounds with volume, and the per-state tabs/notify/sound switches. |
| Notifications › System Notifications | Terminal notifications (OSC 9 and 777). Tailscale notices aren't applicable. |
| Notifications › In-App Notifications | Not built; errors show as a dismissable notice. |
| Interface › Panes, Active Pane, Splits & Focus | Pane style, pane headers (always, hover, hidden), active pane highlight, fade inactive panes, focus follows mouse. Highlight color, opacity and animation aren't built. |
| Interface › Appearance, Themes | Replaced by Noctalia: palette, corner radius, borders, shadows and the interface font follow its live config. |
| Interface › Toolbar, Tab behavior, Tab icons, Window | Sidebar toggle only. Toolbar layouts, vertical tabs and tab icon options aren't built. |
| Interface › Padding | Padding X and Y. Filling padding with program colors isn't built. |
| Terminal › Typography | Font family, size and line height. Defaults come from Ghostty's config, then fontconfig. |
| Terminal › Cursor | Cursor style (block, bar, underline) for programs that don't choose one, force style, and blink speed (off, slow, normal, fast, or a custom interval). As in preview.30, a cursor blinks only when its program asks. When a pane attaches, a program's explicit steady block looks like the default, so it shows your style until the program sets the cursor again. Cursor shaders aren't built. |
| Terminal › Advanced Font, Colors | Not built. Ligatures are off. |
| Terminal › Selection | Copy on select. |
| Server › Client Info | Client label (`client.set_label`). Remote control is always on. |
| Server › Rex Server, Remote Address, Tailscale | Not applicable: rexia doesn't manage servers. |
| Remote Hosts | HTTPS, WebSocket and `ssh://[USER@]HOST[:PORT][/SOCKET]` hosts, kept on the home server (`hosts.put`, `hosts.remove`) and shared with other clients. SSH hosts go through the system `ssh` (your keys, agent and `~/.ssh/config`) and `rex __internal dial-stdio` on the host, as the Rex CLI does; picking an SSH agent in rexia isn't built, so set `IdentityAgent` in `~/.ssh/config`. |
| Integrations, Developer, App Icon, Launch | Not applicable or not built. |

## Behavior

Working:

- Panes laid out exactly as the server arranges them, with dividers, floating layers,
  zoom and minimized layers.
- Terminal rendering through libghostty-vt: colors, styles, wide characters, scrollback,
  selection, search, mouse reporting, bracketed paste, OSC 52 clipboard writes, and
  OSC 8 links opened with ctrl+click (web, mail and file links only).
- The server's keymap, including modes, sequences and init.lua bindings. rexia's
  defaults apply until the server answers.
- Remote control: other clients' `client.perform` requests run in rexia and are answered
  with `client.reply`.
- Program status (OSC 7501): badges on tabs, sessions and pane headers; the activity list;
  desktop notifications for panes out of view that open the pane when clicked; sounds
  from the desktop's sound theme.
- Several hosts in one sidebar, streaming only the host on screen, with reconnects.
- Servers that only speak API revisions below 8 are refused, with a message to update the
  server.

Known gaps:

- No input method support (IME composition) in terminal panes.
- One window per process.
