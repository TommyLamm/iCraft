//! Single control-binding table covering every `ControlBindings` field.

use super::ControlBindings;
use winit::keyboard::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlAction {
    Forward,
    Backward,
    Left,
    Right,
    Jump,
    Sprint,
    Sneak,
    Inventory,
    Chat,
    TimeSpeed,
    Advancements,
    Debug,
    Perspective,
    Gamemode,
    Pause,
    Hotbar1,
    Hotbar2,
    Hotbar3,
    Hotbar4,
    Hotbar5,
    Hotbar6,
    Hotbar7,
    Hotbar8,
    Hotbar9,
}

pub(super) struct ControlBindingMeta {
    pub action: ControlAction,
    pub label_key: &'static str,
    pub getter: fn(&ControlBindings) -> KeyCode,
    pub setter: fn(&mut ControlBindings, KeyCode),
}

pub(super) const CONTROL_BINDINGS: &[ControlBindingMeta] = &[
    ControlBindingMeta {
        action: ControlAction::Forward,
        label_key: "menu.control_forward",
        getter: |c| c.forward,
        setter: |c, k| c.forward = k,
    },
    ControlBindingMeta {
        action: ControlAction::Backward,
        label_key: "menu.control_backward",
        getter: |c| c.backward,
        setter: |c, k| c.backward = k,
    },
    ControlBindingMeta {
        action: ControlAction::Left,
        label_key: "menu.control_left",
        getter: |c| c.left,
        setter: |c, k| c.left = k,
    },
    ControlBindingMeta {
        action: ControlAction::Right,
        label_key: "menu.control_right",
        getter: |c| c.right,
        setter: |c, k| c.right = k,
    },
    ControlBindingMeta {
        action: ControlAction::Jump,
        label_key: "menu.control_jump",
        getter: |c| c.jump,
        setter: |c, k| c.jump = k,
    },
    ControlBindingMeta {
        action: ControlAction::Sprint,
        label_key: "menu.control_sprint",
        getter: |c| c.sprint,
        setter: |c, k| c.sprint = k,
    },
    ControlBindingMeta {
        action: ControlAction::Sneak,
        label_key: "menu.control_sneak",
        getter: |c| c.sneak,
        setter: |c, k| c.sneak = k,
    },
    ControlBindingMeta {
        action: ControlAction::Inventory,
        label_key: "menu.control_inventory",
        getter: |c| c.inventory,
        setter: |c, k| c.inventory = k,
    },
    ControlBindingMeta {
        action: ControlAction::Chat,
        label_key: "menu.control_chat",
        getter: |c| c.chat,
        setter: |c, k| c.chat = k,
    },
    ControlBindingMeta {
        action: ControlAction::TimeSpeed,
        label_key: "menu.control_time_speed",
        getter: |c| c.time_speed,
        setter: |c, k| c.time_speed = k,
    },
    ControlBindingMeta {
        action: ControlAction::Advancements,
        label_key: "menu.control_advancements",
        getter: |c| c.advancements,
        setter: |c, k| c.advancements = k,
    },
    ControlBindingMeta {
        action: ControlAction::Debug,
        label_key: "menu.control_debug",
        getter: |c| c.debug,
        setter: |c, k| c.debug = k,
    },
    ControlBindingMeta {
        action: ControlAction::Perspective,
        label_key: "menu.control_perspective",
        getter: |c| c.perspective,
        setter: |c, k| c.perspective = k,
    },
    ControlBindingMeta {
        action: ControlAction::Gamemode,
        label_key: "menu.control_gamemode",
        getter: |c| c.gamemode,
        setter: |c, k| c.gamemode = k,
    },
    ControlBindingMeta {
        action: ControlAction::Pause,
        label_key: "menu.control_pause",
        getter: |c| c.pause,
        setter: |c, k| c.pause = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar1,
        label_key: "menu.control_hotbar_1",
        getter: |c| c.hotbar_1,
        setter: |c, k| c.hotbar_1 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar2,
        label_key: "menu.control_hotbar_2",
        getter: |c| c.hotbar_2,
        setter: |c, k| c.hotbar_2 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar3,
        label_key: "menu.control_hotbar_3",
        getter: |c| c.hotbar_3,
        setter: |c, k| c.hotbar_3 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar4,
        label_key: "menu.control_hotbar_4",
        getter: |c| c.hotbar_4,
        setter: |c, k| c.hotbar_4 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar5,
        label_key: "menu.control_hotbar_5",
        getter: |c| c.hotbar_5,
        setter: |c, k| c.hotbar_5 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar6,
        label_key: "menu.control_hotbar_6",
        getter: |c| c.hotbar_6,
        setter: |c, k| c.hotbar_6 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar7,
        label_key: "menu.control_hotbar_7",
        getter: |c| c.hotbar_7,
        setter: |c, k| c.hotbar_7 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar8,
        label_key: "menu.control_hotbar_8",
        getter: |c| c.hotbar_8,
        setter: |c, k| c.hotbar_8 = k,
    },
    ControlBindingMeta {
        action: ControlAction::Hotbar9,
        label_key: "menu.control_hotbar_9",
        getter: |c| c.hotbar_9,
        setter: |c, k| c.hotbar_9 = k,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_bindings_table_covers_every_field() {
        let defaults = ControlBindings::default();
        assert_eq!(CONTROL_BINDINGS.len(), 24);
        let mut seen = ControlBindings {
            forward: KeyCode::F24,
            backward: KeyCode::F24,
            left: KeyCode::F24,
            right: KeyCode::F24,
            jump: KeyCode::F24,
            sprint: KeyCode::F24,
            sneak: KeyCode::F24,
            inventory: KeyCode::F24,
            chat: KeyCode::F24,
            time_speed: KeyCode::F24,
            advancements: KeyCode::F24,
            debug: KeyCode::F24,
            perspective: KeyCode::F24,
            gamemode: KeyCode::F24,
            pause: KeyCode::F24,
            hotbar_1: KeyCode::F24,
            hotbar_2: KeyCode::F24,
            hotbar_3: KeyCode::F24,
            hotbar_4: KeyCode::F24,
            hotbar_5: KeyCode::F24,
            hotbar_6: KeyCode::F24,
            hotbar_7: KeyCode::F24,
            hotbar_8: KeyCode::F24,
            hotbar_9: KeyCode::F24,
        };
        for meta in CONTROL_BINDINGS {
            let key = (meta.getter)(&defaults);
            (meta.setter)(&mut seen, key);
            assert_eq!((meta.getter)(&seen), key);
        }
        assert_eq!(seen.forward, defaults.forward);
        assert_eq!(seen.hotbar_9, defaults.hotbar_9);
        assert_eq!(seen.pause, defaults.pause);
        assert_eq!(seen.chat, defaults.chat);
    }
}
