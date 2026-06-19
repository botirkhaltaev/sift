use std::time::{Duration, Instant};

/// Input to the coordinator state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorInput {
    FsChange,
    RefreshComplete,
    DeadlineReached,
}

/// Action produced by a state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorAction {
    None,
    StartRefresh,
    Exit,
}

/// Phase of the coordinator loop.
pub enum CoordinatorState {
    Idle { deadline: Instant },
    Debouncing { deadline: Instant },
    Refreshing { follow_up: bool },
}

impl CoordinatorState {
    pub fn new_idle(idle_timeout: Duration) -> Self {
        Self::Idle {
            deadline: Instant::now() + idle_timeout,
        }
    }

    /// The deadline at which a [`CoordinatorInput::DeadlineReached`] input
    /// should be generated.  Returns `None` when the state has no deadline
    /// (i.e. `Refreshing`, which waits for `RefreshComplete`).
    pub const fn deadline(&self) -> Option<Instant> {
        match self {
            Self::Idle { deadline } | Self::Debouncing { deadline } => Some(*deadline),
            Self::Refreshing { .. } => None,
        }
    }

    /// Pure state transition.  `debounce` and `idle_timeout` are loop config.
    pub fn transition(
        self,
        input: CoordinatorInput,
        debounce: Duration,
        idle_timeout: Duration,
    ) -> (Self, CoordinatorAction) {
        match (self, input) {
            (Self::Idle { .. } | Self::Debouncing { .. }, CoordinatorInput::FsChange) => (
                Self::Debouncing {
                    deadline: Instant::now() + debounce,
                },
                CoordinatorAction::None,
            ),
            (Self::Debouncing { .. }, CoordinatorInput::DeadlineReached)
            | (Self::Refreshing { follow_up: true }, CoordinatorInput::RefreshComplete) => (
                Self::Refreshing { follow_up: false },
                CoordinatorAction::StartRefresh,
            ),
            (Self::Idle { .. }, CoordinatorInput::DeadlineReached) => {
                (Self::new_idle(idle_timeout), CoordinatorAction::Exit)
            }
            (Self::Refreshing { .. }, CoordinatorInput::FsChange) => (
                Self::Refreshing { follow_up: true },
                CoordinatorAction::None,
            ),
            (Self::Refreshing { follow_up: false }, CoordinatorInput::RefreshComplete) => {
                (Self::new_idle(idle_timeout), CoordinatorAction::None)
            }
            (state, _) => (state, CoordinatorAction::None),
        }
    }

    pub const fn is_refreshing(&self) -> bool {
        matches!(self, Self::Refreshing { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEBOUNCE: Duration = Duration::from_mins(1);
    const IDLE: Duration = Duration::from_mins(10);

    fn idle_state() -> CoordinatorState {
        CoordinatorState::new_idle(IDLE)
    }

    #[test]
    fn idle_fs_change_transitions_to_debouncing() {
        let (next, action) = idle_state().transition(CoordinatorInput::FsChange, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Debouncing { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn idle_deadline_reached_exits() {
        let (next, action) =
            idle_state().transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Idle { .. }));
        assert_eq!(action, CoordinatorAction::Exit);
    }

    #[test]
    fn idle_refresh_complete_stays_idle() {
        let (next, action) =
            idle_state().transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Idle { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_fs_change_resets_debounce() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now(),
        };
        let (next, action) = state.transition(CoordinatorInput::FsChange, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Debouncing { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_deadline_reached_starts_refresh() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now(),
        };
        let (next, action) = state.transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn debouncing_refresh_complete_stays_debouncing() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now() + DEBOUNCE,
        };
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Debouncing { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn debouncing_deadline_reached_stays_debouncing_on_catch_all() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now() + DEBOUNCE,
        };
        let (next, action) = state.transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn refreshing_fs_change_requests_follow_up() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let (next, action) = state.transition(CoordinatorInput::FsChange, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if follow_up));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_deadline_reached_stays_refreshing() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let (next, action) = state.transition(CoordinatorInput::DeadlineReached, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_complete_without_follow_up_returns_to_idle() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Idle { .. }));
        assert_eq!(action, CoordinatorAction::None);
    }

    #[test]
    fn refreshing_complete_with_follow_up_restarts_refresh() {
        let state = CoordinatorState::Refreshing { follow_up: true };
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert!(matches!(next, CoordinatorState::Refreshing { follow_up } if !follow_up));
        assert_eq!(action, CoordinatorAction::StartRefresh);
    }

    #[test]
    fn refreshing_complete_with_follow_up_clears_flag() {
        let state = CoordinatorState::Refreshing { follow_up: true };
        let (next, _action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        match next {
            CoordinatorState::Refreshing { follow_up } => {
                assert!(!follow_up, "follow_up should be false after restart");
            }
            _ => panic!("expected Refreshing state"),
        }
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_true_for_refreshing() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        assert!(state.is_refreshing());
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_false_for_idle() {
        assert!(!idle_state().is_refreshing());
    }

    #[test]
    fn coordinator_state_is_refreshing_returns_false_for_debouncing() {
        let state = CoordinatorState::Debouncing {
            deadline: Instant::now(),
        };
        assert!(!state.is_refreshing());
    }

    #[test]
    fn refreshing_complete_resets_idle_deadline() {
        let state = CoordinatorState::Refreshing { follow_up: false };
        let before = Instant::now();
        let (next, action) = state.transition(CoordinatorInput::RefreshComplete, DEBOUNCE, IDLE);
        assert_eq!(action, CoordinatorAction::None);
        match next {
            CoordinatorState::Idle { deadline } => {
                assert!(deadline >= before + IDLE);
            }
            _ => panic!("expected Idle state"),
        }
    }
}
