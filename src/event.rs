use core::cell::UnsafeCell;
use std::sync::Arc;

use anyhow::Result;
use esp_idf_sys::{
    pthread_cond_broadcast, pthread_cond_destroy, pthread_cond_init, pthread_cond_t,
    pthread_cond_wait, pthread_mutex_destroy, pthread_mutex_init, pthread_mutex_lock,
    pthread_mutex_t, pthread_mutex_unlock,
};

// NOTE: ESP-IDF-specific (taken from esp_idf_hal::mutex)
const PTHREAD_MUTEX_INITIALIZER: u32 = 0xFFFFFFFF;

pub struct Condition {
    cond: UnsafeCell<pthread_cond_t>,
    mutex: UnsafeCell<pthread_mutex_t>,
}

impl Condition {
    pub fn new() -> Result<Arc<Self>> {
        let condition = Self {
            cond: UnsafeCell::new(0),
            mutex: UnsafeCell::new(PTHREAD_MUTEX_INITIALIZER),
            // mutex: UnsafeCell::new(PTHREAD_MUTEX_INITIALIZER as _),
        };
        condition.init()?;
        Ok(Arc::new(condition))
    }

    fn init(&self) -> Result<()> {
        if unsafe { pthread_mutex_init(self.mutex.get(), std::ptr::null()) } != 0 {
            anyhow::bail!("Event: pthread_mutex_init error");
        }
        if unsafe { pthread_cond_init(self.cond.get(), std::ptr::null()) } != 0 {
            anyhow::bail!("Event: pthread_cond_init error");
        }
        Ok(())
    }

    pub fn wait(&self) {
        log::info!("Condition::wait starting ...");
        if unsafe { pthread_mutex_lock(self.mutex.get()) } != 0 {
            panic!("Event: pthread_mutex_lock error");
        }
        if unsafe { pthread_cond_wait(self.cond.get(), self.mutex.get()) } != 0 {
            panic!("Event: pthread_cond_wait error");
        }
        if unsafe { pthread_mutex_unlock(self.mutex.get()) } != 0 {
            panic!("Event: pthread_mutex_unlock error");
        }
        log::info!("Condition::wait done!");
    }

    // fn wait_timeout_ms(&self, duration: std::time::Duration) {
    //     panic!("Condition: wait_timeout_ms not implemented");
    // }

    // fn notify_one(&self) {
    //     // This isn't trivial since we need to handle spurious wake ups.
    //     panic!("Condition: notify_one not implemented");
    //     // if unsafe { pthread_cond_signal(self.cond.get_mut()) } != 0 {
    //     //     panic!("Event: pthread_cond_signal error");
    //     // }
    // }

    pub fn notify_all(&self) {
        log::info!("Condition::notify_all");
        if unsafe { pthread_cond_broadcast(self.cond.get()) } != 0 {
            panic!("Event: pthread_cond_broadcast error");
        }
        log::info!("Condition::notify_all done!");
    }
}

impl Drop for Condition {
    fn drop(&mut self) {
        if unsafe { pthread_mutex_destroy(self.mutex.get()) } != 0 {
            panic!("Event: pthread_mutex_destroy error");
        }
        if unsafe { pthread_cond_destroy(self.cond.get()) } != 0 {
            panic!("Event: pthread_cond_destroy error");
        }
    }
}
