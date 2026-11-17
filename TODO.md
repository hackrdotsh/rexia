# Deferred work

These are future considerations, not current implementation tasks.

- [ ] Revisit distribution and updates for other Linux distributions. Nix is the
  current packaging approach, not a permanent requirement. Evaluate native
  packages or portable bundles, then choose an update strategy appropriate to
  each distribution format. Keep in-app update work deferred for now.
- [ ] Consider optional custom themes as an override of Noctalia's live theme,
  while retaining Noctalia-following behavior as the default. Decide the theme
  format, import support, and behavior without Noctalia before implementation.
