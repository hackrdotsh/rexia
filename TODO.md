# Deferred work

These are future tasks and considerations; none are being implemented yet.

- [ ] Implement QR-code device pairing (`client.pair_device`). This is a required
  parity feature, not a Linux exclusion. Manually adding a host remains a
  workaround, not a replacement for pairing.

- [ ] Revisit distribution and updates for other Linux distributions. Nix is the
  current packaging approach, not a permanent requirement. Evaluate native
  packages or portable bundles, then choose an update strategy appropriate to
  each distribution format. Keep in-app update work deferred for now.
- [ ] Consider optional custom themes as an override of Noctalia's live theme,
  while retaining Noctalia-following behavior as the default. Decide the theme
  format, import support, and behavior without Noctalia before implementation.
