/// Enter/Leave's integer output scale is only a fallback. Neither event may replace
/// a scale provided by one of the surface-specific protocols.
pub(crate) fn use_output_scale(fractional_available: bool, preferred_available: bool) -> bool {
    !fractional_available && !preferred_available
}
