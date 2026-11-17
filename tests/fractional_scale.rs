// Exercise the vendored policy used by both Wayland Enter and Leave handlers.
#[path = "../vendor/gpui-linux/src/linux/wayland/scale_policy.rs"]
mod scale_policy;

#[test]
fn fractional_scale_wins_even_on_old_surfaces() {
    assert!(!scale_policy::use_output_scale(true, false));
    assert!(!scale_policy::use_output_scale(true, true));
    assert!(!scale_policy::use_output_scale(false, true));
    assert!(scale_policy::use_output_scale(false, false));
}
