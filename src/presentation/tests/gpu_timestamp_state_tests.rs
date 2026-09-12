// Tests extracted from state.rs::gpu_timestamp_state_tests (Plan 27).

use super::*;

use super::GpuTimestampReadbackState as S;
#[test]
fn transitions_are_ordered_and_failure_is_recoverable() {
    assert_eq!(S::Unmapped.map_requested(), S::Unmapped);
    assert_eq!(S::CopyEncoded.map_requested(), S::Mapping);
    assert_eq!(S::Mapping.map_completed(true), S::Mapped);
    assert_eq!(S::Mapped.consume(), S::Consumed);
    assert_eq!(S::Mapping.map_completed(false), S::Unmapped);
    assert_eq!(S::Consumed.consume(), S::Consumed);
    assert_eq!(S::Mapped.map_requested(), S::Mapped);
}

#[test]
fn two_submission_tagged_slots_cannot_be_reused_while_mapping_or_mapped() {
    let mut slots = [
        super::GpuTimestampReadbackStatus::unmapped(),
        super::GpuTimestampReadbackStatus::unmapped(),
    ];
    assert!(slots[0].reserve_copy(10));
    assert!(slots[0].begin_mapping(10));
    assert!(!slots[0].reserve_copy(11));
    assert!(slots[1].reserve_copy(11));
    assert!(slots[1].begin_mapping(11));

    slots[0].map_completed(10, true);
    assert!(!slots[0].reserve_copy(12));
    assert!(slots[0].consume(10));
    assert!(slots[0].reserve_copy(12));
    assert_eq!(slots[0].submission_tag, Some(12));
}

#[test]
fn capability_requires_timestamp_query_and_inside_passes() {
    assert!(!super::gpu_timestamp_capability(false, false));
    assert!(!super::gpu_timestamp_capability(true, false));
    assert!(super::gpu_timestamp_capability(true, true));
}
