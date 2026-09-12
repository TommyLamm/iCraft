use super::*;

    use super::*;
    use crate::presentation_inventory_policy::{
        schedule_presentation_chunk_load, PresentationChunkLoadPolicy, PresentationTopology,
    };
    use std::collections::HashMap;

    #[test]
    fn legacy_metadata_preserves_saved_hardcore_and_creation_options() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        for (case, level_hardcore, rules_hardcore) in
            [("level", true, false), ("rules", false, true)]
        {
            let world_dir = Path::new(SAVES_DIR).join(format!(
                "icraft_legacy_metadata_{}_{}_{}",
                std::process::id(),
                unique,
                case
            ));
            let backup_dir = world_dir.with_extension("backup");
            fs::create_dir_all(&world_dir).expect("temporary world should be created");

            let mut level = crate::save::LevelData::default();
            level.seed = 0xA11C_E55;
            level.version = CURRENT_WORLD_FORMAT_VERSION;
            level.hardcore = level_hardcore;
            level.rules.hardcore = rules_hardcore;
            level.world_type = WorldType::Superflat;
            level.generate_structures = false;
            level.bonus_chest = true;
            level.cheats_enabled = true;
            let player = crate::save::PlayerData::from_state(
                glam::Vec3::ZERO,
                glam::Vec3::ZERO,
                0.0,
                0.0,
                &crate::player::PlayerState::new(),
                GameMode::Adventure,
                &crate::inventory::Inventory::new(),
                crate::advancements::AdvancementProgressData::default(),
            );
            crate::save::SaveManager::new(&world_dir)
                .save_player_and_level(&level, &player)
                .expect("legacy fixtures should save");

            let assert_metadata = |metadata: WorldMetadata| {
                assert_eq!(metadata.seed, level.seed);
                assert_eq!(metadata.game_mode, GameMode::Adventure);
                assert_eq!(metadata.difficulty, Difficulty::Hard);
                assert!(metadata.hardcore);
                assert_eq!(metadata.world_type, WorldType::Superflat);
                assert!(!metadata.generate_structures);
                assert!(metadata.bonus_chest);
                assert!(metadata.cheats_enabled);
            };
            assert_metadata(legacy_metadata(&world_dir).expect("legacy metadata should load"));

            backup_world(&world_dir, &backup_dir).expect("legacy world backup should succeed");
            assert_metadata(
                legacy_metadata(&backup_dir).expect("backup metadata should retain legacy rules"),
            );

            fs::remove_dir_all(&world_dir).expect("temporary world should be removable");
            fs::remove_dir_all(&backup_dir).expect("temporary backup should be removable");
        }
    }

    #[test]
    fn server_address_book_keeps_recent_ping_results() {
        let mut book = ServerAddressBook::new(2);
        book.remember("127.0.0.1:25565");
        book.record_ping(ServerPingResult {
            address: "example.test:25565".into(),
            version: "0.1.0".into(),
            motd: "Welcome".into(),
            online_players: 2,
            max_players: 20,
            error: None,
        });
        book.record_ping(ServerPingResult {
            address: "127.0.0.1:25565".into(),
            version: "0.1.0".into(),
            motd: "Local".into(),
            online_players: 1,
            max_players: 20,
            error: None,
        });
        assert_eq!(book.addresses().len(), 2);
        assert_eq!(book.addresses()[0], "127.0.0.1:25565");
        assert_eq!(book.recent_results()[0].motd, "Local");
    }

    #[test]
    fn multiplayer_focus_activation_covers_saved_servers_and_actions() {
        assert_eq!(multiplayer_focus_count(MultiplayerMode::Host, 99), 5);
        assert_eq!(multiplayer_focus_count(MultiplayerMode::Join, 0), 8);
        assert_eq!(multiplayer_focus_count(MultiplayerMode::Join, 99), 11);
        let rects = multiplayer_focus_rects(MultiplayerMode::Join, 2);
        assert_eq!(rects.len(), 10);
        assert_eq!(rects[0], [-0.52, -0.02, 0.45, 0.58]);
        assert_eq!(rects[5], [0.04, 0.56, 0.26, 0.34]);
        assert_eq!(rects[7], [0.04, 0.56, -0.24, -0.11]);
        assert_eq!(rects[8], [-0.52, -0.02, -0.58, -0.45]);
        assert_eq!(rects[9], [0.02, 0.52, -0.58, -0.45]);
    }

    #[test]
    fn saved_server_addresses_split_for_join_form() {
        assert_eq!(
            split_host_port("example.test:25565"),
            Some(("example.test".into(), "25565".into()))
        );
        assert_eq!(
            split_host_port("[::1]:25565"),
            Some(("::1".into(), "25565".into()))
        );
        assert!(split_host_port("not-an-address").is_none());
    }

    #[test]
    fn load_world_creation_options_reads_game_mode_and_cheats() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let world_dir = Path::new(SAVES_DIR).join(format!(
            "icraft_creation_options_{}_{}",
            std::process::id(),
            unique
        ));
        let metadata = WorldMetadata {
            name: "CREATIVE WORLD".to_string(),
            seed: 42,
            game_mode: GameMode::Creative,
            difficulty: Difficulty::Normal,
            last_played: 0,
            world_type: WorldType::Default,
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: true,
            hardcore: false,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        };
        metadata.save(&world_dir).expect("world.meta should save");
        let options = load_world_creation_options(&world_dir);
        assert_eq!(options.game_mode, GameMode::Creative);
        assert!(options.cheats_enabled);
        fs::remove_dir_all(&world_dir).expect("temporary world should be removable");
    }

    #[test]
    fn sanitizes_world_names_and_generates_stable_slugs() {
        assert_eq!(sanitize_name("  My <World>!  "), "My World");
        assert_eq!(slugify("My World"), "my_world");
    }

    #[test]
    fn selected_world_directory_survives_list_reordering() {
        let metadata = |name: &str, last_played| WorldMetadata {
            name: name.to_string(),
            seed: 1,
            game_mode: GameMode::Creative,
            difficulty: Difficulty::Normal,
            last_played,
            world_type: WorldType::Default,
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: false,
            hardcore: false,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        };
        let first_dir = PathBuf::from("C:/saves/first");
        let second_dir = PathBuf::from("C:/saves/second");
        let mut worlds = vec![
            WorldEntry {
                directory: first_dir,
                metadata: metadata("SAME NAME", 2),
            },
            WorldEntry {
                directory: second_dir.clone(),
                metadata: metadata("SAME NAME", 1),
            },
        ];

        worlds.reverse();

        let index = world_index_by_directory(&worlds, &second_dir).unwrap();
        assert_eq!(worlds[index].directory, second_dir);
    }

    #[test]
    fn settings_key_names_round_trip() {
        for code in [
            KeyCode::KeyW,
            KeyCode::Space,
            KeyCode::ControlLeft,
            KeyCode::ArrowUp,
        ] {
            assert_eq!(parse_key(key_name(code)), Some(code));
        }
    }

    #[test]
    fn difficulty_steps_both_directions() {
        assert_eq!(Difficulty::Peaceful.step(-1), Difficulty::Hard);
        assert_eq!(Difficulty::Normal.step(1), Difficulty::Hard);
    }

    #[test]
    fn legacy_settings_without_weather_volume_use_reduced_default() {
        let settings = GameSettings::from_file_contents(
            "master_volume:0.8\nsound_volume:0.6\nmusic_volume:0.2\n",
        );

        assert!((settings.weather_volume - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn leftover_dynamic_resolution_settings_keys_are_ignored() {
        let settings = GameSettings::from_file_contents(concat!(
            "fov:90\n",
            "render_scale:0.5\n",
            "dynamic_resolution:true\n",
            "entity_distance_scale:1.5\n",
            "unknown_future_key:1\n",
        ));

        assert_eq!(settings.fov, 90.0);
        assert!((settings.entity_distance_scale - 1.5).abs() < f32::EPSILON);

        let contents = settings.to_file_contents();
        assert!(!contents.contains("render_scale"));
        assert!(!contents.contains("dynamic_resolution"));
        assert!(contents.contains("entity_distance_scale:1.5\n"));
        assert!(contents.contains("fov:90\n"));
    }

    #[test]
    fn weather_volume_load_clamps_out_of_range_values() {
        let too_high = GameSettings::from_file_contents("weather_volume:4.5\n");
        let too_low = GameSettings::from_file_contents("weather_volume:-2\n");
        let not_finite = GameSettings::from_file_contents("weather_volume:NaN\n");

        assert_eq!(too_high.weather_volume, 1.0);
        assert_eq!(too_low.weather_volume, 0.0);
        assert_eq!(not_finite.weather_volume, 0.4);
    }

    #[test]
    fn settings_file_round_trip_includes_weather_volume() {
        let mut original = GameSettings::default();
        original.weather_volume = 0.3;

        let contents = original.to_file_contents();
        let loaded = GameSettings::from_file_contents(&contents);

        assert!(contents.contains("weather_volume:0.3\n"));
        assert!((loaded.weather_volume - original.weather_volume).abs() < f32::EPSILON);
    }

    #[test]
    fn non_finite_view_settings_fall_back_during_load() {
        for value in ["NaN", "inf", "-inf"] {
            let settings =
                GameSettings::from_file_contents(&format!("fov:{value}\nsensitivity:{value}\n"));

            assert_eq!(settings.fov, 70.0, "fov should reject {value}");
            assert_eq!(
                settings.sensitivity, 0.002,
                "sensitivity should reject {value}"
            );
        }
    }

    #[test]
    fn non_finite_view_settings_are_sanitized_before_save() {
        let mut settings = GameSettings::default();
        settings.fov = f32::NAN;
        settings.sensitivity = f32::INFINITY;

        let contents = settings.to_file_contents();
        let loaded = GameSettings::from_file_contents(&contents);

        assert!(contents.contains("fov:70\n"));
        assert!(contents.contains("sensitivity:0.002\n"));
        assert!(!contents.contains("NaN"));
        assert!(!contents.contains("inf"));
        assert_eq!(loaded.fov, 70.0);
        assert_eq!(loaded.sensitivity, 0.002);
    }

    #[test]
    fn view_setting_boundaries_round_trip_without_drift() {
        for (fov, sensitivity) in [(30.0, 0.0002), (120.0, 0.006)] {
            let mut settings = GameSettings::default();
            settings.fov = fov;
            settings.sensitivity = sensitivity;

            let loaded = GameSettings::from_file_contents(&settings.to_file_contents());

            assert_eq!(loaded.fov, fov);
            assert_eq!(loaded.sensitivity, sensitivity);
        }
    }

    #[test]
    fn weather_options_row_is_distinct_from_language_controls_and_back() {
        assert_eq!(options_row_at(-0.08), Some(3));
        assert_eq!(options_row_at(-0.28), Some(4));
        assert_eq!(options_row_at(-0.48), Some(5));
        assert_eq!(options_row_at(-0.70), None);
        assert!(hit(0.4, -0.08, 0.05, 0.82, -0.15, -0.02));
    }

    #[test]
    fn multiplayer_settings_defaults_and_mutation() {
        let mut settings = GameSettings::default();
        assert_eq!(settings.mp_host_port, "25565");
        assert_eq!(settings.mp_server_address, "127.0.0.1");
        assert_eq!(settings.mp_join_port, "25565");
        assert_eq!(settings.mp_username, "PLAYER");

        settings.mp_host_port = "25570".to_string();
        settings.mp_server_address = "192.168.1.100".to_string();
        settings.mp_join_port = "25571".to_string();
        settings.mp_username = "TEST_USER".to_string();

        assert_eq!(settings.mp_host_port, "25570");
        assert_eq!(settings.mp_server_address, "192.168.1.100");
        assert_eq!(settings.mp_join_port, "25571");
        assert_eq!(settings.mp_username, "TEST_USER");
    }

    #[test]
    fn leaving_controls_clears_pending_rebind() {
        let (screen, active_field, rebinding) = back_transition(
            MenuScreen::Controls,
            Some(TextField::WorldName),
            Some(ControlAction::Forward),
        );

        assert_eq!(screen, MenuScreen::Options);
        assert_eq!(active_field, None);
        assert_eq!(rebinding, None);
    }

    #[test]
    fn world_path_guard_rejects_saves_root() {
        assert!(validated_world_path(Path::new(SAVES_DIR)).is_err());
    }

    fn try_create_world_link(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(target, link).is_ok() {
                return true;
            }
            std::process::Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    &link.to_string_lossy(),
                    &target.to_string_lossy(),
                ])
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (target, link);
            false
        }
    }

    #[test]
    fn world_path_guard_rejects_escape_and_symlink_roots() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let pid = std::process::id();
        let outside = std::env::temp_dir().join(format!("icraft_escape_{pid}_{unique}"));
        fs::create_dir_all(&outside).expect("outside dir");
        assert!(
            validated_world_path(&outside).is_err(),
            "canonicalize escaping saves/ must be rejected"
        );

        let _ = fs::create_dir_all(SAVES_DIR);
        let inside = Path::new(SAVES_DIR).join(format!("icraft_real_{pid}_{unique}"));
        let metadata = WorldMetadata {
            name: "REAL".to_string(),
            seed: 1,
            game_mode: GameMode::Survival,
            difficulty: Difficulty::Normal,
            last_played: 0,
            world_type: WorldType::Default,
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: false,
            hardcore: false,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        };
        metadata.save(&inside).expect("real world should save");
        assert!(validated_world_path(&inside).is_ok());

        let link = Path::new(SAVES_DIR).join(format!("icraft_link_{pid}_{unique}"));
        let created =
            try_create_world_link(&outside, &link) || try_create_world_link(&inside, &link);
        if created {
            assert!(
                validated_world_path(&link).is_err(),
                "symlink/junction world roots must not be playable"
            );
            let discovered = discover_worlds();
            assert!(
                !discovered.iter().any(|world| {
                    world.directory.file_name() == link.file_name() || world.directory == link
                }),
                "symlink/junction world roots must not appear in the menu"
            );
            let _ = fs::remove_dir(&link);
            let _ = fs::remove_file(&link);
        }
        let _ = fs::remove_dir_all(&inside);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn client_world_launch_uses_temp_dir_and_placeholder_seed() {
        let launch = WorldLaunch {
            world_dir: std::env::temp_dir().join("icraft_multiplayer_client"),
            seed: 0,
            game_mode: GameMode::Survival,
            difficulty: Difficulty::Normal,
            role: MultiplayerRole::Client {
                server_addr: "127.0.0.1".to_string(),
                port: 25565,
                username: "PLAYER".to_string(),
            },
        };
        assert!(launch.world_dir.starts_with(std::env::temp_dir()));
        assert!(!launch.world_dir.starts_with("saves"));
        assert_eq!(launch.seed, 0);
        assert!(matches!(launch.game_mode, GameMode::Survival));
        assert!(matches!(launch.role, MultiplayerRole::Client { .. }));
    }

    fn join_client_role() -> MultiplayerRole {
        MultiplayerRole::Client {
            server_addr: "127.0.0.1".to_string(),
            port: 25565,
            username: "JOINER".to_string(),
        }
    }

    #[test]
    fn join_client_load_policy_never_generates_or_mutates() {
        let client = join_client_role();
        assert!(client.is_join_client());
        let join_policy = PresentationTopology::from(&client, false).chunk_load_policy();
        assert_eq!(
            join_policy,
            PresentationChunkLoadPolicy::AwaitAuthoritativePayload
        );

        let mut generated = false;
        let loaded = schedule_presentation_chunk_load(join_policy, || {
            generated = true;
            1
        });
        assert!(loaded.is_none());
        assert!(!generated);

        assert_eq!(
            PresentationTopology::from(&MultiplayerRole::Singleplayer, true).chunk_load_policy(),
            PresentationChunkLoadPolicy::GenerateLocally
        );
        assert_eq!(
            PresentationTopology::from(&MultiplayerRole::Host { port: 25565 }, true)
                .chunk_load_policy(),
            PresentationChunkLoadPolicy::GenerateLocally
        );
        assert_eq!(
            schedule_presentation_chunk_load(
                PresentationTopology::Embedded.chunk_load_policy(),
                || 7
            ),
            Some(7)
        );
    }

    #[test]
    fn controls_config_parse_and_round_trip() {
        let mut settings = GameSettings::default();
        let config_text = r#"
# Custom Controls Config Test
key_forward = UP
key_backward = DOWN
key_left = LEFT
key_right = RIGHT
key_jump = SPACE
key_sprint = LCTRL
key_sneak = LSHIFT
key_inventory = E
key_chat = Y
key_advancements = K
key_debug = F1
key_perspective = F2
key_gamemode = M
key_pause = ESC
"#;
        settings.apply_file_contents(config_text);
        assert_eq!(settings.controls.forward, KeyCode::ArrowUp);
        assert_eq!(settings.controls.backward, KeyCode::ArrowDown);
        assert_eq!(settings.controls.left, KeyCode::ArrowLeft);
        assert_eq!(settings.controls.right, KeyCode::ArrowRight);
        assert_eq!(settings.controls.chat, KeyCode::KeyY);
        assert_eq!(settings.controls.advancements, KeyCode::KeyK);
        assert_eq!(settings.controls.debug, KeyCode::F1);
        assert_eq!(settings.controls.perspective, KeyCode::F2);
        assert_eq!(settings.controls.gamemode, KeyCode::KeyM);
        assert_eq!(settings.controls.pause, KeyCode::Escape);

        let exported = settings.to_controls_file_contents();
        assert!(exported.contains("key_forward = UP"));
        assert!(exported.contains("key_backward = DOWN"));
        assert!(exported.contains("key_chat = Y"));
        assert!(exported.contains("key_advancements = K"));
        assert!(exported.contains("key_debug = F1"));
    }

    #[test]
    fn fps_cap_round_trip_and_sanitization() {
        let settings = GameSettings::from_file_contents("fps_cap:144");
        assert_eq!(settings.fps_cap, 144);
        assert!(GameSettings::from_file_contents("fps_cap:0").fps_cap == 0);
        assert_eq!(GameSettings::from_file_contents("fps_cap:999").fps_cap, 240);
        assert!(GameSettings::from_file_contents("fps_cap:-1").fps_cap == 0);
        let mut settings = GameSettings::default();
        settings.fps_cap = 60;
        assert!(settings.to_file_contents().contains("fps_cap:60"));
    }

    #[test]
    fn fps_cap_cycles_through_uncapped_and_standard_rates() {
        assert_eq!(cycle_fps_cap(0, 1), 30);
        assert_eq!(cycle_fps_cap(30, 1), 60);
        assert_eq!(cycle_fps_cap(60, 1), 144);
        assert_eq!(cycle_fps_cap(144, 1), 0);
        assert_eq!(fps_cap_label(0), "UNCAPPED");
    }

    #[test]
    fn accessibility_settings_survive_restart_and_resource_pack_ids_round_trip() {
        let mut settings = GameSettings::default();
        settings.accessibility.ui_scale = 1.75;
        settings.accessibility.chat_scale = 1.5;
        settings.accessibility.chat_opacity = 0.25;
        settings.accessibility.subtitles = true;
        settings.accessibility.high_contrast = true;
        settings.accessibility.reduce_flashing = true;
        settings.accessibility.toggle_sprint = true;
        settings.accessibility.toggle_sneak = true;
        settings.accessibility.camera_bobbing = false;
        settings.accessibility.damage_tilt = false;
        settings.resource_packs = vec!["demo.base".to_string(), "demo.hud".to_string()];

        let loaded = GameSettings::from_file_contents(&settings.to_file_contents());
        assert_eq!(loaded.accessibility, settings.accessibility);
        assert_eq!(loaded.resource_packs, settings.resource_packs);
    }

    #[test]
    fn focus_layout_rects_remain_inside_ndc_at_common_aspects() {
        let rects = [
            [-0.90, 0.90, -0.88, 0.82],   // accessibility
            [-0.86, 0.86, -0.88, 0.82],   // resource packs/worlds
            [-0.64, 0.64, -0.92, 0.78],   // create world
            [-0.48, 0.48, -0.84, 0.72],   // controls/confirm delete
            [-0.99, 0.99, -0.97, -0.875], // chat input
        ];
        for aspect in [4.0f32 / 3.0, 16.0 / 9.0, 21.0 / 9.0] {
            assert!(aspect.is_finite() && aspect > 0.0);
            for [x0, x1, y0, y1] in rects {
                assert!(x0 < x1 && y0 < y1);
                assert!((-1.0..=1.0).contains(&x0));
                assert!((-1.0..=1.0).contains(&x1));
                assert!((-1.0..=1.0).contains(&y0));
                assert!((-1.0..=1.0).contains(&y1));
            }
        }
    }

    #[test]
    fn menu_rect_tables_are_valid_and_consistent() {
        let mut all_rects = Vec::new();
        all_rects.extend_from_slice(&MAIN_SCREEN_RECTS);
        all_rects.extend_from_slice(&CONFIRM_DELETE_RECTS);
        all_rects.extend_from_slice(&CREATE_WORLD_SCREEN_RECTS);
        all_rects.extend_from_slice(&OPTIONS_BOTTOM_SCREEN_RECTS);
        all_rects.extend_from_slice(&options_button_rects());
        all_rects.extend(controls_button_rects());
        all_rects.extend_from_slice(&accessibility_button_rects());
        all_rects.extend_from_slice(&RESOURCE_PACKS_BOTTOM_RECTS);
        all_rects.extend_from_slice(&WORLDS_BOTTOM_RECTS);
        all_rects.extend_from_slice(&MULTIPLAYER_MODE_RECTS);
        all_rects.push(MULTIPLAYER_HOST_PORT_RECT);
        all_rects.extend_from_slice(&MULTIPLAYER_JOIN_FIELD_RECTS);
        all_rects.push(MULTIPLAYER_PING_RECT);
        all_rects.extend_from_slice(&MULTIPLAYER_BOTTOM_RECTS);

        for i in 0..5 {
            all_rects.push(world_item_rect(i));
            all_rects.push(resource_pack_item_rect(i));
        }
        for i in 0..3 {
            all_rects.push(recent_server_item_rect(i));
        }

        for rect in all_rects {
            assert!(
                rect.x0 < rect.x1,
                "x0 ({}) must be < x1 ({})",
                rect.x0,
                rect.x1
            );
            assert!(
                rect.y0 < rect.y1,
                "y0 ({}) must be < y1 ({})",
                rect.y0,
                rect.y1
            );
            assert!((-1.0..=1.0).contains(&rect.x0));
            assert!((-1.0..=1.0).contains(&rect.x1));
            assert!((-1.0..=1.0).contains(&rect.y0));
            assert!((-1.0..=1.0).contains(&rect.y1));

            let cx = (rect.x0 + rect.x1) * 0.5;
            let cy = (rect.y0 + rect.y1) * 0.5;
            assert!(rect.contains(cx, cy));
            assert!(hit(cx, cy, rect.x0, rect.x1, rect.y0, rect.y1));
            assert!(!rect.contains(rect.x0 - 0.1, cy));
            assert!(!rect.contains(rect.x1 + 0.1, cy));
            assert!(!rect.contains(cx, rect.y0 - 0.1));
            assert!(!rect.contains(cx, rect.y1 + 0.1));
        }
    }

    fn lit_pixels(rows: [u8; 7]) -> usize {
        rows.into_iter().map(|row| row.count_ones() as usize).sum()
    }

    #[test]
    fn menu_text_uses_selected_font_and_builtin_fallback() {
        let mut overrides = HashMap::new();
        // A deliberately dense override makes it unambiguous that the
        // ordinary menu text path selected the bitmap font.
        overrides.insert('A', [31; 7]);
        let bitmap = FontSource::Bitmap(overrides);

        let mut custom_vertices = Vec::new();
        draw_text(
            &mut custom_vertices,
            "AB",
            0.0,
            0.0,
            0.01,
            1.0,
            [1.0; 4],
            &bitmap,
        );

        let mut builtin_vertices = Vec::new();
        draw_text(
            &mut builtin_vertices,
            "AB",
            0.0,
            0.0,
            0.01,
            1.0,
            [1.0; 4],
            &FontSource::BuiltIn,
        );

        // The selected bitmap overrides A, while the absent B override still
        // falls back to the built-in glyph table.
        assert_eq!(
            custom_vertices.len(),
            (lit_pixels([31; 7]) + lit_pixels(glyph('B'))) * 6
        );
        assert_eq!(
            builtin_vertices.len(),
            (lit_pixels(glyph('A')) + lit_pixels(glyph('B'))) * 6
        );
        assert!(custom_vertices.len() > builtin_vertices.len());

        // The regular main-menu logo helper also receives the selected font;
        // this guards against accidentally updating only the resource-pack
        // listing path.
        let mut custom_logo = Vec::new();
        draw_logo(&mut custom_logo, 1.0, &bitmap);
        let mut builtin_logo = Vec::new();
        draw_logo(&mut builtin_logo, 1.0, &FontSource::BuiltIn);
        assert!(custom_logo.len() > builtin_logo.len());
    }
