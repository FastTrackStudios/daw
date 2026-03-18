//! Cross-platform abstractions for native and WASM targets.
//!
//! Provides unified APIs for async primitives that differ between platforms:
//! - **RwLock**: `tokio::sync::RwLock` on native, `async_lock::RwLock` on WASM
//! - **sleep**: `tokio::time::sleep` on native, `gloo_timers` on WASM

use std::time::Duration;

/// Sleep for the given duration, compatible with both native and WASM targets.
pub async fn sleep(duration: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::time::sleep(duration).await;
    }
    #[cfg(target_arch = "wasm32")]
    {
        gloo_timers::future::sleep(duration).await;
    }
}

// ─── RwLock abstraction ──────────────────────────────────────────────────────

/// On native: tokio's async RwLock with a named constructor for API uniformity.
/// On WASM: async-lock's RwLock (lightweight, single-threaded safe).
#[cfg(not(target_arch = "wasm32"))]
mod native_rwlock {
    use std::ops::{Deref, DerefMut};

    pub struct RwLock<T>(tokio::sync::RwLock<T>);

    impl<T> RwLock<T> {
        #[inline]
        pub fn new(_name: &'static str, value: T) -> Self {
            Self(tokio::sync::RwLock::new(value))
        }

        #[inline]
        pub async fn read(&self) -> RwLockReadGuard<'_, T> {
            RwLockReadGuard(self.0.read().await)
        }

        #[inline]
        pub async fn write(&self) -> RwLockWriteGuard<'_, T> {
            RwLockWriteGuard(self.0.write().await)
        }
    }

    pub struct RwLockReadGuard<'a, T>(tokio::sync::RwLockReadGuard<'a, T>);

    impl<T> Deref for RwLockReadGuard<'_, T> {
        type Target = T;
        #[inline]
        fn deref(&self) -> &T {
            &self.0
        }
    }

    pub struct RwLockWriteGuard<'a, T>(tokio::sync::RwLockWriteGuard<'a, T>);

    impl<T> Deref for RwLockWriteGuard<'_, T> {
        type Target = T;
        #[inline]
        fn deref(&self) -> &T {
            &self.0
        }
    }

    impl<T> DerefMut for RwLockWriteGuard<'_, T> {
        #[inline]
        fn deref_mut(&mut self) -> &mut T {
            &mut self.0
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native_rwlock::RwLock;

#[cfg(target_arch = "wasm32")]
mod wasm_rwlock {
    use std::ops::{Deref, DerefMut};

    pub struct RwLock<T>(async_lock::RwLock<T>);

    impl<T> RwLock<T> {
        #[inline]
        pub fn new(_name: &'static str, value: T) -> Self {
            Self(async_lock::RwLock::new(value))
        }

        #[inline]
        pub async fn read(&self) -> RwLockReadGuard<'_, T> {
            RwLockReadGuard(self.0.read().await)
        }

        #[inline]
        pub async fn write(&self) -> RwLockWriteGuard<'_, T> {
            RwLockWriteGuard(self.0.write().await)
        }
    }

    pub struct RwLockReadGuard<'a, T>(async_lock::RwLockReadGuard<'a, T>);

    impl<T> Deref for RwLockReadGuard<'_, T> {
        type Target = T;
        #[inline]
        fn deref(&self) -> &T {
            &self.0
        }
    }

    pub struct RwLockWriteGuard<'a, T>(async_lock::RwLockWriteGuard<'a, T>);

    impl<T> Deref for RwLockWriteGuard<'_, T> {
        type Target = T;
        #[inline]
        fn deref(&self) -> &T {
            &self.0
        }
    }

    impl<T> DerefMut for RwLockWriteGuard<'_, T> {
        #[inline]
        fn deref_mut(&mut self) -> &mut T {
            &mut self.0
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_rwlock::RwLock;
