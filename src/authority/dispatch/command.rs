use super::*;

impl AuthorityCore {
    /// Commands that mutate authenticated session or world state. `State` only
    /// projects the resulting snapshot and never edits pose or game mode as
    /// authority. `/respawn` stays a string match so it is not folded into the
    /// operator-only leftover Command list; TCP `ClientRespawnRequest` is a
    /// separate non-op entry.
    pub(super) fn apply_command(
        &mut self,
        session_id: PlayerId,
        command: &str,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let Some(dimension) = self
            .sessions
            .get(&session_id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return Err(RejectReason::Unauthorized);
        };
        if command.trim().eq_ignore_ascii_case("/respawn") {
            return if self.respawn_session(session_id) {
                Ok(None)
            } else {
                Err(RejectReason::Unauthorized)
            };
        }
        let parsed = crate::commands::parse(command).map_err(|_| RejectReason::InvalidState)?;
        if matches!(
            parsed.surface(),
            crate::commands::CommandSurface::ConsoleOnly
        ) {
            return Err(RejectReason::Unsupported);
        }
        match parsed {
            crate::commands::Command::GameMode { mode, target } => {
                if target.is_some_and(|target| {
                    !matches!(
                        target,
                        crate::commands::CommandTarget::SelfPlayer
                            | crate::commands::CommandTarget::NearestPlayer
                    )
                }) {
                    return Err(RejectReason::PermissionDenied);
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                session.game_mode = mode;
                Ok(None)
            }
            crate::commands::Command::Teleport { target, position } => {
                if !matches!(
                    target,
                    crate::commands::CommandTarget::SelfPlayer
                        | crate::commands::CommandTarget::NearestPlayer
                ) {
                    return Err(RejectReason::PermissionDenied);
                }
                if !self.world(dimension).dimension.height().contains_y(position[1]) {
                    return Err(RejectReason::InvalidCoordinate);
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                session.position = [
                    position[0] as f32 + 0.5,
                    position[1] as f32,
                    position[2] as f32 + 0.5,
                ];
                Ok(None)
            }
            crate::commands::Command::Give {
                target,
                item,
                count,
            } => {
                if !matches!(
                    target,
                    crate::commands::CommandTarget::SelfPlayer
                        | crate::commands::CommandTarget::NearestPlayer
                ) {
                    return Err(RejectReason::PermissionDenied);
                }
                let Some(session) = self.sessions.get_mut(&session_id) else {
                    return Err(RejectReason::Unauthorized);
                };
                let stack = crate::inventory::ItemStack::new(item, count);
                let slot = SessionInventorySlot::from_wire(
                    crate::network::protocol::ItemWire::from_stack(&stack),
                    stack.can_break,
                    stack.can_place_on,
                );
                if !session.gameplay.add_slot(slot) {
                    return Err(RejectReason::InvalidState);
                }
                Ok(None)
            }
            crate::commands::Command::GameRule { rule, value } => {
                self.world_mut_expect(dimension)
                    .set_gamerule(&rule, value.as_deref())?;
                Ok(None)
            }
            crate::commands::Command::Time(crate::commands::TimeCommand::Set(time)) => {
                self.world_mut_expect(dimension).set_time(time);
                Ok(None)
            }
            crate::commands::Command::Time(crate::commands::TimeCommand::Add(time)) => {
                self.world_mut_expect(dimension).add_time(time);
                Ok(None)
            }
            // ConsoleOnly arms are rejected above; keep a defensive catch-all.
            _ => Err(RejectReason::Unsupported),
        }
    }

    pub(super) fn rejected(&mut self, request_id: u128, reason: RejectReason) -> GameplayResponse {
        GameplayResponse {
            request_id,
            server_sequence: self.current_revision(self.config.dimension),
            outcome: GameplayOutcome::Rejected { reason },
        }
    }

    pub(super) fn reject_for_session(
        &mut self,
        session_id: PlayerId,
        request_id: u128,
        reason: RejectReason,
        consumed_sequence: Option<u64>,
    ) -> GameplayResponse {
        let dimension = self
            .sessions
            .get(&session_id)
            .and_then(|session| Dimension::from_wire(session.dimension))
            .unwrap_or(self.config.dimension);
        let response = GameplayResponse {
            request_id,
            server_sequence: self.current_revision(dimension),
            outcome: GameplayOutcome::Rejected { reason },
        };
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if let Some(sequence) = consumed_sequence {
                session.last_client_sequence = sequence;
            }
            session.cache_response(response.clone());
        }
        response
    }
}
