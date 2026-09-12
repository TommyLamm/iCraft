// Tests extracted from state.rs::camera_perspective_tests (Plan 27).

use super::*;

use super::*;

#[test]
fn f5_cycles_like_minecraft() {
    let first = CameraPerspective::FirstPerson;
    let back = first.next();
    let front = back.next();
    assert_eq!(back, CameraPerspective::ThirdPersonBack);
    assert_eq!(front, CameraPerspective::ThirdPersonFront);
    assert_eq!(front.next(), CameraPerspective::FirstPerson);
}

#[test]
fn front_camera_sits_ahead_and_looks_back_at_player() {
    let (offset, view_yaw, view_pitch) =
        perspective_camera_transform(CameraPerspective::ThirdPersonFront, 0.35, -0.2);
    let view_forward = Vec3::new(
        view_yaw.cos() * view_pitch.cos(),
        view_pitch.sin(),
        view_yaw.sin() * view_pitch.cos(),
    )
    .normalize();
    assert!((offset.length() - 4.0).abs() < 1e-5);
    assert!(offset.normalize().dot(view_forward) < -0.999);
}
