//! Keep the display and system awake while NVForever is running (#3272).
//!
//! Windows resets its display idle timer only for keyboard and mouse input
//! [documented: Microsoft Learn, `SetThreadExecutionState`], so the screen turns off
//! during gamepad play and unattended runs; with the display off and vsync on, frames
//! stall waiting for present.
//!
//! On Windows, a `Startup` system pinned to the main thread (`NonSendMarker`) calls
//! `SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)`
//! [design]. Normal shutdown (`AppExit` in `Last`) and dropping the main-thread guard
//! clear the request with `SetThreadExecutionState(ES_CONTINUOUS)` [design]; Windows
//! also clears a thread's execution state automatically when the thread exits
//! [documented: Microsoft Learn, `SetThreadExecutionState`]. Non-Windows platforms are a
//! no-op.

use bevy::prelude::*;

pub struct KeepAwakePlugin;

impl Plugin for KeepAwakePlugin {
    fn build(&self, #[allow(unused_variables)] app: &mut App) {
        #[cfg(target_os = "windows")]
        win::install(app);
    }
}

#[cfg(target_os = "windows")]
mod win {
    use std::marker::PhantomData;

    use bevy::ecs::system::NonSendMarker;
    use bevy::prelude::*;
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
        EXECUTION_STATE,
    };

    /// Flags requested while the game runs: keep the display on and the system awake
    /// until cleared or the main thread exits [design, #3272].
    pub(super) const KEEP_AWAKE_FLAGS: EXECUTION_STATE =
        ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED;

    /// Flags written on release: clear display/system requirements while leaving
    /// continuous mode reset [documented: Microsoft Learn, `SetThreadExecutionState`].
    pub(super) const RELEASE_FLAGS: EXECUTION_STATE = ES_CONTINUOUS;

    pub(super) struct KeepAwakeGuard {
        active: bool,
        set_state: fn(EXECUTION_STATE) -> EXECUTION_STATE,
        _not_send: PhantomData<*const ()>,
    }

    impl KeepAwakeGuard {
        pub(super) fn new(set_state: fn(EXECUTION_STATE) -> EXECUTION_STATE) -> Self {
            Self { active: false, set_state, _not_send: PhantomData }
        }

        pub(super) fn acquire(&mut self) -> Option<EXECUTION_STATE> {
            if self.active {
                return None;
            }
            let prev = (self.set_state)(KEEP_AWAKE_FLAGS);
            if prev == 0 {
                warn!("keep_awake: SetThreadExecutionState failed to inhibit display sleep");
                return None;
            }
            self.active = true;
            info!("keep_awake: display and system sleep inhibited while running");
            Some(prev)
        }

        pub(super) fn release(&mut self) -> Option<EXECUTION_STATE> {
            if !self.active {
                return None;
            }
            self.active = false;
            let prev = (self.set_state)(RELEASE_FLAGS);
            if prev == 0 {
                warn!("keep_awake: SetThreadExecutionState failed to restore sleep state");
                return None;
            }
            Some(prev)
        }
    }

    impl Drop for KeepAwakeGuard {
        fn drop(&mut self) {
            self.release();
        }
    }

    fn win32_set_execution_state(flags: EXECUTION_STATE) -> EXECUTION_STATE {
        // SAFETY: `SetThreadExecutionState` is a plain Win32 call on the current thread
        // taking a bitmask of `EXECUTION_STATE` flags and returning the prior state (or 0).
        unsafe { SetThreadExecutionState(flags) }
    }

    pub(super) fn install(app: &mut App) {
        app.insert_non_send(KeepAwakeGuard::new(win32_set_execution_state))
            .add_systems(Startup, acquire)
            .add_systems(Last, release);
    }

    fn acquire(mut guard: NonSendMut<KeepAwakeGuard>, _main_thread: NonSendMarker) {
        guard.acquire();
    }

    fn release(
        mut exit: MessageReader<AppExit>,
        mut guard: NonSendMut<KeepAwakeGuard>,
        _main_thread: NonSendMarker,
    ) {
        if exit.read().next().is_some() {
            guard.release();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::RefCell;

        thread_local! {
            static CALLS: RefCell<Vec<EXECUTION_STATE>> = const { RefCell::new(Vec::new()) };
            static RETURN_VALUE: RefCell<EXECUTION_STATE> = const { RefCell::new(ES_CONTINUOUS) };
        }

        fn mock_set_state(flags: EXECUTION_STATE) -> EXECUTION_STATE {
            CALLS.with(|c| c.borrow_mut().push(flags));
            RETURN_VALUE.with(|r| *r.borrow())
        }

        fn reset_mock(ret: EXECUTION_STATE) {
            CALLS.with(|c| c.borrow_mut().clear());
            RETURN_VALUE.with(|r| *r.borrow_mut() = ret);
        }

        fn take_calls() -> Vec<EXECUTION_STATE> {
            CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
        }

        #[test]
        fn keep_awake_flags_include_continuous_display_and_system() {
            assert_eq!(
                KEEP_AWAKE_FLAGS,
                ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
            );
            assert_eq!(RELEASE_FLAGS, ES_CONTINUOUS);
            assert_ne!(KEEP_AWAKE_FLAGS & ES_CONTINUOUS, 0);
            assert_ne!(KEEP_AWAKE_FLAGS & ES_DISPLAY_REQUIRED, 0);
            assert_ne!(KEEP_AWAKE_FLAGS & ES_SYSTEM_REQUIRED, 0);
        }

        #[test]
        fn guard_acquires_once_and_releases_on_explicit_release_or_drop() {
            reset_mock(ES_CONTINUOUS);
            {
                let mut guard = KeepAwakeGuard::new(mock_set_state);
                assert_eq!(guard.acquire(), Some(ES_CONTINUOUS));
                assert_eq!(guard.acquire(), None, "second acquire is a no-op while active");
                assert_eq!(guard.release(), Some(ES_CONTINUOUS));
                assert_eq!(guard.release(), None, "second release is idempotent");
            }
            assert_eq!(take_calls(), vec![KEEP_AWAKE_FLAGS, RELEASE_FLAGS]);

            // Drop clears an unreleased guard.
            reset_mock(ES_CONTINUOUS);
            {
                let mut guard = KeepAwakeGuard::new(mock_set_state);
                assert_eq!(guard.acquire(), Some(ES_CONTINUOUS));
            }
            assert_eq!(take_calls(), vec![KEEP_AWAKE_FLAGS, RELEASE_FLAGS]);
        }

        #[test]
        fn guard_does_not_release_after_failed_acquire() {
            reset_mock(0);
            {
                let mut guard = KeepAwakeGuard::new(mock_set_state);
                assert_eq!(guard.acquire(), None);
                assert_eq!(guard.release(), None);
            }
            assert_eq!(take_calls(), vec![KEEP_AWAKE_FLAGS]);
        }

        #[test]
        fn plugin_inhibits_on_startup_and_restores_on_app_exit() {
            let mut app = App::new();
            app.add_message::<AppExit>().add_plugins(super::super::KeepAwakePlugin);
            app.update();
            assert!(app.world().non_send::<KeepAwakeGuard>().active);

            app.world_mut().write_message(AppExit::Success);
            app.update();
            assert!(!app.world().non_send::<KeepAwakeGuard>().active);
        }

        #[test]
        fn win32_round_trip_sets_and_clears_display_and_system_flags() {
            let mut guard = KeepAwakeGuard::new(win32_set_execution_state);
            assert!(guard.acquire().is_some());
            let prev_at_release = guard.release().expect("release succeeds");
            assert_eq!(
                prev_at_release & KEEP_AWAKE_FLAGS,
                KEEP_AWAKE_FLAGS,
                "Windows reported our requested execution state before release"
            );
        }
    }
}
