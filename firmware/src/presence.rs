// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! Physical user presence: either the dedicated BOOTSEL button (sampled via the
//! QSPI-CS-to-Hi-Z trick in a RAM function) or a plain GPIO input.
//! The wait blocks the worker while the high-priority transports stream keepalives
//! reporting `UPNEEDED` ([`up_pending`]). One [`PresenceButton`] serves every
//! applet's `UserPresence` trait; a touch is required by default, and the opt-in
//! `no-touch` feature makes `request` confirm instantly.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_rp::Peri;
use embassy_rp::peripherals::BOOTSEL;
#[allow(unused_imports)]
use embassy_rp::gpio::Input;

#[cfg(not(feature = "no-touch"))]
use embassy_rp::bootsel::is_bootsel_pressed;
#[cfg(not(feature = "no-touch"))]
use embassy_time::{Duration, Instant, block_for};

/// Set while the worker is blocked in a button wait — read by the CTAPHID keepalive
/// to report `UPNEEDED` (0x02) instead of `PROCESSING` (0x01).
static UP_PENDING: AtomicBool = AtomicBool::new(false);

/// Set by the CTAPHID transport (high-priority executor) when a `CTAPHID_CANCEL`
/// arrives for the request the worker is processing. The button wait — running on
/// the worker executor — polls it each iteration and abandons with `Cancelled`.
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The CTAPHID keepalive hook: is a touch being awaited?
pub fn up_pending() -> bool {
    UP_PENDING.load(Ordering::Relaxed)
}

/// The CTAPHID cancel hook: request that an in-flight touch wait be abandoned.
pub fn request_cancel() {
    CANCEL_REQUESTED.store(true, Ordering::Relaxed);
}

// Poll cadence and the press timeout.
#[cfg(not(feature = "no-touch"))]
const POLL_MS: u64 = 16;
#[cfg(not(feature = "no-touch"))]
const TIMEOUT_MS: u64 = 30_000;

/// Neutral wait result, mapped to each applet's own `Presence` enum.
#[cfg(not(feature = "no-touch"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Confirmed,
    Timeout,
    Cancelled,
}

/// Polarity of a GPIO-attached button: `ActiveLow` pulls the pin low when pressed
/// (button to GND), `ActiveHigh` sees a high level (e.g. a capacitive touch module).
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Polarity {
    ActiveLow,
    ActiveHigh,
}

/// User-presence button: either the RP2350's dedicated BOOTSEL or a GPIO pin with
/// configurable polarity. The `no-touch` build short-cuts everything to `Confirmed`.
#[allow(dead_code)]
pub enum PresenceButton {
    Bootsel(Peri<'static, BOOTSEL>),
    Gpio(Input<'static>, Polarity),
}

impl PresenceButton {
    /// Wrap the dedicated BOOTSEL pin.
    pub fn new_bootsel(bootsel: Peri<'static, BOOTSEL>) -> Self {
        Self::Bootsel(bootsel)
    }

    /// Wrap a GPIO pin as a presence button with the given polarity.
    #[allow(dead_code)]
    pub fn new_gpio(pin: Input<'static>, pol: Polarity) -> Self {
        Self::Gpio(pin, pol)
    }

    /// One non-blocking sample of the button level.
    pub fn poll_pressed(&mut self) -> bool {
        #[cfg(not(feature = "no-touch"))]
        match self {
            Self::Bootsel(pin) => is_bootsel_pressed(pin.reborrow()),
            Self::Gpio(pin, Polarity::ActiveLow) => pin.is_low(),
            Self::Gpio(pin, Polarity::ActiveHigh) => pin.is_high(),
        }
        #[cfg(feature = "no-touch")]
        false
    }

    /// Block until button press, timeout, or cancellation.
    #[cfg(not(feature = "no-touch"))]
    fn wait(&mut self) -> Outcome {
        let saved = crate::led::status();
        crate::led::set_status(crate::led::STATUS_TOUCH);
        CANCEL_REQUESTED.store(false, Ordering::Relaxed);
        UP_PENDING.store(true, Ordering::Relaxed);
        let start = Instant::now();
        let result = loop {
            if self.poll_pressed() {
                break Outcome::Confirmed;
            }
            if CANCEL_REQUESTED.load(Ordering::Relaxed) {
                break Outcome::Cancelled;
            }
            if start.elapsed() >= Duration::from_millis(TIMEOUT_MS) {
                break Outcome::Timeout;
            }
            block_for(Duration::from_millis(POLL_MS));
        };
        // Debounce: wait for release (bounded).
        if result == Outcome::Confirmed {
            let release = Instant::now();
            while self.poll_pressed() {
                if release.elapsed() >= Duration::from_millis(TIMEOUT_MS) {
                    break;
                }
                block_for(Duration::from_millis(POLL_MS));
            }
        }
        UP_PENDING.store(false, Ordering::Relaxed);
        CANCEL_REQUESTED.store(false, Ordering::Relaxed);
        crate::led::set_status(saved);
        result
    }
}

// Use a macro for the three nearly-identical UserPresence impls.
macro_rules! impl_user_presence {
    ($trait_path:path, $presence_path:path, $confirmed:path, $timeout:path, $cancelled:path) => {
        impl $trait_path for PresenceButton {
            fn request(&mut self) -> $presence_path {
                #[cfg(not(feature = "no-touch"))]
                match self.wait() {
                    Outcome::Confirmed => $confirmed,
                    Outcome::Timeout => $timeout,
                    Outcome::Cancelled => $cancelled,
                }
                #[cfg(feature = "no-touch")]
                $confirmed
            }
        }
    };
}

impl_user_presence!(
    rsk_fido::UserPresence,
    rsk_fido::Presence,
    rsk_fido::Presence::Confirmed,
    rsk_fido::Presence::Timeout,
    rsk_fido::Presence::Cancelled
);

impl_user_presence!(
    rsk_openpgp::UserPresence,
    rsk_openpgp::Presence,
    rsk_openpgp::Presence::Confirmed,
    rsk_openpgp::Presence::Timeout,
    rsk_openpgp::Presence::Timeout
);

impl_user_presence!(
    rsk_otp::UserPresence,
    rsk_otp::Presence,
    rsk_otp::Presence::Confirmed,
    rsk_otp::Presence::Timeout,
    rsk_otp::Presence::Timeout
);

impl_user_presence!(
    rsk_oath::UserPresence,
    rsk_oath::Presence,
    rsk_oath::Presence::Confirmed,
    rsk_oath::Presence::Timeout,
    rsk_oath::Presence::Timeout
);
