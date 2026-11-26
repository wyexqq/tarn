use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const ES_CONTINUOUS: u32 = 0x8000_0000;
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
const STATE_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetThreadExecutionState(flags: u32) -> u32;
}

#[cfg(windows)]
fn apply_to_this_thread(flags: u32) {
    unsafe {
        SetThreadExecutionState(flags);
    }
}

#[cfg(not(windows))]
fn apply_to_this_thread(_flags: u32) {}

fn flags_for(stay_awake: bool) -> u32 {
    if stay_awake {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED
    } else {
        ES_CONTINUOUS
    }
}

#[derive(Default)]
struct Shared {
    running_work: AtomicUsize,
    is_enabled: AtomicBool,
}

impl Shared {
    fn should_stay_awake(&self) -> bool {
        self.is_enabled.load(Ordering::SeqCst) && self.running_work.load(Ordering::SeqCst) > 0
    }
}

pub struct KeepAwake {
    shared: Arc<Shared>,
    changed: Mutex<Sender<()>>,
}

pub struct RunningWork {
    shared: Arc<Shared>,
    changed: Sender<()>,
}

impl Drop for RunningWork {
    fn drop(&mut self) {
        self.shared.running_work.fetch_sub(1, Ordering::SeqCst);
        let _ = self.changed.send(());
    }
}

impl KeepAwake {
    pub fn start(is_enabled: bool) -> KeepAwake {
        let shared = Arc::new(Shared::default());
        shared.is_enabled.store(is_enabled, Ordering::SeqCst);
        let (sender, receiver) = channel::<()>();
        let shared_for_thread = shared.clone();
        std::thread::spawn(move || loop {
            apply_to_this_thread(flags_for(shared_for_thread.should_stay_awake()));
            if matches!(receiver.recv_timeout(STATE_REFRESH_INTERVAL), Err(RecvTimeoutError::Disconnected)) {
                apply_to_this_thread(flags_for(false));
                break;
            }
        });
        KeepAwake { shared, changed: Mutex::new(sender) }
    }

    fn sender(&self) -> Sender<()> {
        self.changed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    pub fn set_enabled(&self, is_enabled: bool) {
        self.shared.is_enabled.store(is_enabled, Ordering::SeqCst);
        let _ = self.sender().send(());
    }

    pub fn begin_work(&self) -> RunningWork {
        self.shared.running_work.fetch_add(1, Ordering::SeqCst);
        let changed = self.sender();
        let _ = changed.send(());
        RunningWork { shared: self.shared.clone(), changed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_awake_only_while_enabled_and_working() {
        let keeper = KeepAwake::start(true);
        assert!(!keeper.shared.should_stay_awake());
        let first = keeper.begin_work();
        let second = keeper.begin_work();
        assert!(keeper.shared.should_stay_awake());
        drop(first);
        assert!(keeper.shared.should_stay_awake());
        keeper.set_enabled(false);
        assert!(!keeper.shared.should_stay_awake());
        keeper.set_enabled(true);
        drop(second);
        assert!(!keeper.shared.should_stay_awake());
    }

    #[test]
    fn asks_windows_to_keep_the_system_on_without_holding_the_screen() {
        assert_eq!(flags_for(true), 0x8000_0001);
        assert_eq!(flags_for(false), 0x8000_0000);
    }
}
