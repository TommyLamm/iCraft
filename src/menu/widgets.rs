//! Shared menu widget table: one layout drives draw, hit-test, and Tab focus.

use super::MenuRect;

/// Widget chrome kind. Labels and click effects are resolved by the owning screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WidgetKind {
    Button,
    Field,
    Toggle,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Widget {
    pub kind: WidgetKind,
    pub rect: MenuRect,
}

/// Static screen description. Scrolling lists are built at runtime instead.
#[derive(Debug, Clone, Copy)]
pub(super) struct Screen {
    pub widgets: &'static [Widget],
}

impl Screen {
    pub const fn new(widgets: &'static [Widget]) -> Self {
        Self { widgets }
    }

    pub fn focus_count(&self) -> usize {
        self.widgets.len()
    }

    pub fn focus_rect(&self, index: usize) -> Option<[f32; 4]> {
        self.widgets.get(index).map(|w| w.rect.as_array())
    }

    pub fn hit_index(&self, x: f32, y: f32) -> Option<usize> {
        self.widgets
            .iter()
            .position(|widget| widget.rect.contains(x, y))
    }

    pub fn widget(&self, index: usize) -> Option<&Widget> {
        self.widgets.get(index)
    }
}

pub(super) const MAIN_SCREEN: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.34, 0.34, 0.21, 0.34),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.34, 0.34, 0.03, 0.16),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.34, 0.34, -0.15, -0.02),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.34, 0.34, -0.33, -0.20),
    },
]);

pub(super) const CONFIRM_DELETE_SCREEN: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.48, -0.02, -0.16, -0.02),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(0.02, 0.48, -0.16, -0.02),
    },
]);

const fn create_world_rect(index: usize) -> MenuRect {
    match index {
        0 => MenuRect::new(-0.52, 0.52, 0.34, 0.47),
        1 => MenuRect::new(-0.52, 0.52, 0.13, 0.26),
        2 => MenuRect::new(-0.52, 0.52, -0.08, 0.05),
        3 => MenuRect::new(-0.52, 0.52, -0.29, -0.16),
        4 => MenuRect::new(-0.52, 0.52, -0.42, -0.30),
        5 => MenuRect::new(-0.52, -0.02, -0.54, -0.43),
        6 => MenuRect::new(0.02, 0.52, -0.54, -0.43),
        7 => MenuRect::new(-0.52, -0.02, -0.69, -0.58),
        8 => MenuRect::new(0.02, 0.52, -0.69, -0.58),
        9 => MenuRect::new(-0.52, -0.02, -0.84, -0.71),
        _ => MenuRect::new(0.02, 0.52, -0.84, -0.71),
    }
}

pub(super) const CREATE_WORLD_SCREEN: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Field,
        rect: create_world_rect(0),
    },
    Widget {
        kind: WidgetKind::Field,
        rect: create_world_rect(1),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: create_world_rect(2),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: create_world_rect(3),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: create_world_rect(4),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: create_world_rect(5),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: create_world_rect(6),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: create_world_rect(7),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: create_world_rect(8),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: create_world_rect(9),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: create_world_rect(10),
    },
]);

const OPTIONS_ROW_TOPS: [f32; 6] = [0.58, 0.38, 0.18, -0.02, -0.22, -0.42];

const fn options_rect(index: usize) -> MenuRect {
    if index < 12 {
        let row = index % 6;
        let left = index < 6;
        let top = OPTIONS_ROW_TOPS[row];
        if left {
            MenuRect::new(-0.82, -0.05, top - 0.13, top)
        } else {
            MenuRect::new(0.05, 0.82, top - 0.13, top)
        }
    } else {
        match index {
            12 => MenuRect::new(-0.82, -0.30, -0.78, -0.64),
            13 => MenuRect::new(-0.25, 0.25, -0.78, -0.64),
            _ => MenuRect::new(0.30, 0.82, -0.78, -0.64),
        }
    }
}

pub(super) const OPTIONS_SCREEN: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(0),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(1),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(2),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(3),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(4),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(5),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(6),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(7),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(8),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(9),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(10),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: options_rect(11),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: options_rect(12),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: options_rect(13),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: options_rect(14),
    },
]);

const fn accessibility_rect(index: usize) -> MenuRect {
    if index < 10 {
        let column = index / 5;
        let row = index % 5;
        let (x0, x1) = if column == 0 {
            (-0.82, -0.05)
        } else {
            (0.05, 0.82)
        };
        let top = 0.56 - row as f32 * 0.18;
        MenuRect::new(x0, x1, top - 0.13, top)
    } else {
        MenuRect::new(-0.25, 0.25, -0.78, -0.64)
    }
}

pub(super) const ACCESSIBILITY_SCREEN: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(0),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(1),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(2),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(3),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(4),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(5),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(6),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(7),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(8),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: accessibility_rect(9),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: accessibility_rect(10),
    },
]);

pub(super) const RESOURCE_PACKS_BOTTOM: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.78, -0.28, -0.78, -0.64),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.22, 0.22, -0.78, -0.64),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(0.28, 0.78, -0.78, -0.64),
    },
]);

pub(super) const WORLDS_BOTTOM: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.72, -0.27, -0.64, -0.51),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.23, 0.23, -0.64, -0.51),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(0.27, 0.72, -0.64, -0.51),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.72, -0.27, -0.84, -0.72),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.23, 0.23, -0.84, -0.72),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(0.27, 0.72, -0.84, -0.72),
    },
]);

pub(super) const CONTROLS_SENSITIVITY: Widget = Widget {
    kind: WidgetKind::Toggle,
    rect: MenuRect::new(-0.48, 0.48, 0.49, 0.62),
};

pub(super) const CONTROLS_DONE: Widget = Widget {
    kind: WidgetKind::Button,
    rect: MenuRect::new(-0.25, 0.25, -0.78, -0.64),
};

/// Visible control-binding rows on the controls screen (scrolling list).
pub(super) const CONTROLS_VISIBLE_ROWS: usize = 8;

pub(super) fn control_binding_rect(visible_index: usize) -> MenuRect {
    let column = visible_index / 4;
    let row = visible_index % 4;
    let (x0, x1) = if column == 0 {
        (-0.78, -0.04)
    } else {
        (0.04, 0.78)
    };
    let top = 0.38 - row as f32 * 0.19;
    MenuRect::new(x0, x1, top - 0.14, top)
}

pub(super) fn resource_pack_item_rect(visible_index: isize) -> MenuRect {
    let top = 0.56 - visible_index as f32 * 0.14;
    MenuRect::new(-0.78, 0.78, top - 0.11, top)
}

pub(super) fn world_item_rect(visible_index: isize) -> MenuRect {
    let top = 0.58 - visible_index as f32 * 0.19;
    MenuRect::new(-0.72, 0.72, top - 0.15, top)
}

pub(super) fn recent_server_item_rect(index: usize) -> MenuRect {
    let top = 0.34 - index as f32 * 0.10;
    MenuRect::new(0.04, 0.56, top - 0.08, top)
}

pub(super) const MULTIPLAYER_MODE: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Toggle,
        rect: MenuRect::new(-0.52, -0.02, 0.45, 0.58),
    },
    Widget {
        kind: WidgetKind::Toggle,
        rect: MenuRect::new(0.02, 0.52, 0.45, 0.58),
    },
]);

pub(super) const MULTIPLAYER_HOST_PORT: Widget = Widget {
    kind: WidgetKind::Field,
    rect: MenuRect::new(-0.52, 0.52, 0.17, 0.30),
};

pub(super) const MULTIPLAYER_JOIN_FIELDS: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Field,
        rect: MenuRect::new(-0.56, -0.02, 0.17, 0.30),
    },
    Widget {
        kind: WidgetKind::Field,
        rect: MenuRect::new(-0.56, -0.02, -0.04, 0.09),
    },
    Widget {
        kind: WidgetKind::Field,
        rect: MenuRect::new(-0.56, -0.02, -0.25, -0.12),
    },
]);

pub(super) const MULTIPLAYER_PING: Widget = Widget {
    kind: WidgetKind::Button,
    rect: MenuRect::new(0.04, 0.56, -0.24, -0.11),
};

pub(super) const MULTIPLAYER_BOTTOM: Screen = Screen::new(&[
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(-0.52, -0.02, -0.58, -0.45),
    },
    Widget {
        kind: WidgetKind::Button,
        rect: MenuRect::new(0.02, 0.52, -0.58, -0.45),
    },
]);

pub(super) const MAIN_SCREEN_RECTS: [MenuRect; 4] = [
    MAIN_SCREEN.widgets[0].rect,
    MAIN_SCREEN.widgets[1].rect,
    MAIN_SCREEN.widgets[2].rect,
    MAIN_SCREEN.widgets[3].rect,
];
pub(super) const CONFIRM_DELETE_RECTS: [MenuRect; 2] = [
    CONFIRM_DELETE_SCREEN.widgets[0].rect,
    CONFIRM_DELETE_SCREEN.widgets[1].rect,
];
pub(super) const CREATE_WORLD_SCREEN_RECTS: [MenuRect; 11] = [
    CREATE_WORLD_SCREEN.widgets[0].rect,
    CREATE_WORLD_SCREEN.widgets[1].rect,
    CREATE_WORLD_SCREEN.widgets[2].rect,
    CREATE_WORLD_SCREEN.widgets[3].rect,
    CREATE_WORLD_SCREEN.widgets[4].rect,
    CREATE_WORLD_SCREEN.widgets[5].rect,
    CREATE_WORLD_SCREEN.widgets[6].rect,
    CREATE_WORLD_SCREEN.widgets[7].rect,
    CREATE_WORLD_SCREEN.widgets[8].rect,
    CREATE_WORLD_SCREEN.widgets[9].rect,
    CREATE_WORLD_SCREEN.widgets[10].rect,
];
pub(super) const OPTIONS_BOTTOM_SCREEN_RECTS: [MenuRect; 3] = [
    OPTIONS_SCREEN.widgets[12].rect,
    OPTIONS_SCREEN.widgets[13].rect,
    OPTIONS_SCREEN.widgets[14].rect,
];
pub(super) const RESOURCE_PACKS_BOTTOM_RECTS: [MenuRect; 3] = [
    RESOURCE_PACKS_BOTTOM.widgets[0].rect,
    RESOURCE_PACKS_BOTTOM.widgets[1].rect,
    RESOURCE_PACKS_BOTTOM.widgets[2].rect,
];
pub(super) const WORLDS_BOTTOM_RECTS: [MenuRect; 6] = [
    WORLDS_BOTTOM.widgets[0].rect,
    WORLDS_BOTTOM.widgets[1].rect,
    WORLDS_BOTTOM.widgets[2].rect,
    WORLDS_BOTTOM.widgets[3].rect,
    WORLDS_BOTTOM.widgets[4].rect,
    WORLDS_BOTTOM.widgets[5].rect,
];
pub(super) const MULTIPLAYER_MODE_RECTS: [MenuRect; 2] = [
    MULTIPLAYER_MODE.widgets[0].rect,
    MULTIPLAYER_MODE.widgets[1].rect,
];
pub(super) const MULTIPLAYER_HOST_PORT_RECT: MenuRect = MULTIPLAYER_HOST_PORT.rect;
pub(super) const MULTIPLAYER_JOIN_FIELD_RECTS: [MenuRect; 3] = [
    MULTIPLAYER_JOIN_FIELDS.widgets[0].rect,
    MULTIPLAYER_JOIN_FIELDS.widgets[1].rect,
    MULTIPLAYER_JOIN_FIELDS.widgets[2].rect,
];
pub(super) const MULTIPLAYER_PING_RECT: MenuRect = MULTIPLAYER_PING.rect;
pub(super) const MULTIPLAYER_BOTTOM_RECTS: [MenuRect; 2] = [
    MULTIPLAYER_BOTTOM.widgets[0].rect,
    MULTIPLAYER_BOTTOM.widgets[1].rect,
];

