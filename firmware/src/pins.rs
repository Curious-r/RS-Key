// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! Runtime GPIO pin allocation via [`AnyPin::steal`].
//!
//! Embassy's type-level pin model guarantees exclusive access through concrete
//! types (`p.PIN_0` …) consumed at init. For run-time-selected pins — the GPIO
//! LED backend and the presence button — we use `AnyPin::steal()` to obtain a
//! type-erased pin by GPIO number.
//!
//! # Safety rationale
//!
//! `steal` is unsafe because it can alias a concrete pin that another driver
//! also owns. We avoid aliasing because:
//!
//! - The three LED backends (GPIO, Pimoroni PWM, WS2812 PIO) are mutually
//!   exclusive at runtime — only one match arm executes.
//! - The presence-button GPIO arm is separate from the LED arm; the phy record
//!   must not assign the same pin to both.
//! - These constraints are enforced by the phy-record schema (single PIN per
//!   field) and by code review.
//!
//! # PIO limitation
//!
//! `AnyPin` implements [`Pin`] but *not* [`PioPin`](embassy_rp::pio::PioPin).
//! The WS2812 backend still uses a concrete-type match.

use embassy_rp::Peri;
use embassy_rp::gpio::AnyPin;

/// Allocate a type-erased GPIO pin by number. Panics if `gpio > 29`.
///
/// # Safety
///
/// The caller must ensure the pin is not already in use by another driver.
/// See the [module-level docs](self) for the safety model.
#[allow(unused)]
pub unsafe fn take(gpio: u8) -> Peri<'static, AnyPin> {
    assert!(gpio <= 29, "GPIO pin {} out of range (0-29)", gpio);
    unsafe { AnyPin::steal(gpio) }
}
