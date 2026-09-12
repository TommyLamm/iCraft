use super::*;

impl ServerWorld {
    /// Advance exactly one 20 Hz tick.  All iteration order is normalized so
    /// the checksum and mutation revisions are topology-independent.
    /// `simulation_chunks` is the interest union supplied by the runtime (or a
    /// test helper); plate occupants rebuild only when players cross columns.
    pub fn tick(
        &mut self,
        players: &[(PlayerId, [f32; 3], f32, f32)],
        simulation_chunks: &BTreeSet<(i32, i32)>,
    ) -> Vec<WorldMutation> {
        if self.rules.do_daylight_cycle {
            self.time = self.time.wrapping_add(1);
        }
        let mut mutations = Vec::new();

        self.refresh_plate_occupants(players);
        let occupants = self.plate_occupants.clone();

        let redstone = self.redstone.tick(&mut self.chunks, &occupants);
        let mut redstone_mutations = redstone.mutations;
        self.pending_redstone_actions.extend(
            redstone
                .actions
                .into_iter()
                .filter(|action| matches!(action, RedstoneAction::Dispense { .. })),
        );
        redstone_mutations.sort_by_key(|mutation| mutation.pos);
        for mutation in redstone_mutations {
            if !self.column_in_set(simulation_chunks, mutation.pos.0, mutation.pos.2) {
                continue;
            }
            if let Ok(Some(event)) = self.set_block(
                mutation.pos.0,
                mutation.pos.1,
                mutation.pos.2,
                mutation.new_block,
                0,
            ) {
                mutations.push(event);
            }
        }

        // These systems mutate actual block entities/chunks, not a shadow map.
        // Simulation-union only — always pass Some(union) to *_in_columns.
        let hopper = crate::world_tick::tick_hoppers_in_columns(
            &mut self.chunks,
            Some(&mut self.entities),
            MAX_AUTOMATION_TRANSFERS,
            Some(simulation_chunks),
        );
        for pos in hopper.changed_positions {
            self.redstone.mark_container_changed(&self.chunks, pos);
        }
        for is_lava in [false, true] {
            let (_, fluid_mutations) = crate::fluid::tick_fluids_in_columns(
                &mut self.chunks,
                is_lava,
                MAX_FLUID_UPDATES,
                Some(simulation_chunks),
            );
            for mutation in fluid_mutations {
                mutations.push(self.record_fluid_mutation(mutation));
            }
        }

        // Random ticks (crop growth, fire and leaf decay) run in the same
        // deterministic headless world as redstone/fluid automation.  The
        // renderer never performs a second random-tick pass for a boundary.
        let (mut random_ticks, _) = crate::world_tick::sample_random_ticks_in_columns(
            &self.chunks,
            simulation_chunks,
            self.seed as u64,
            self.time,
            self.dimension as u8,
            128,
        );
        if !self.rules.do_fire_tick {
            random_ticks.retain(|mutation| {
                self.get_block(mutation.pos.0, mutation.pos.1, mutation.pos.2) != BlockType::Fire
            });
        }
        random_ticks.sort_by_key(|mutation| mutation.pos);
        for mutation in random_ticks {
            if !self.column_in_set(simulation_chunks, mutation.pos.0, mutation.pos.2) {
                continue;
            }
            if let Ok(Some(event)) = self.set_block(
                mutation.pos.0,
                mutation.pos.1,
                mutation.pos.2,
                mutation.new_block,
                mutation.new_state,
            ) {
                mutations.push(event);
            }
        }

        mutations.extend(self.tick_furnaces(simulation_chunks));

        self.tick_entities(players);
        mutations.sort_by_key(|mutation| mutation.revision);
        mutations
    }

    /// Test / standalone helper: build the simulation union from `players` then tick.
    pub fn tick_players(
        &mut self,
        players: &[(PlayerId, [f32; 3], f32, f32)],
    ) -> Vec<WorldMutation> {
        let union = self.simulation_union_from_players(players);
        self.tick(players, &union)
    }

}

