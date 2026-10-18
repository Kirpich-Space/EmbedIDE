//! Hardware-independent firmware logic. Build on the host or for an installed MCU target.
#![no_std]

/// Advance a periodic LED state without touching hardware registers.
pub fn led_on(ticks: u32) -> bool {
    ticks % 1000 < 500
}

#[cfg(test)]
mod tests {
    #[test]
    fn led_changes_at_half_period() {
        assert!(super::led_on(499));
        assert!(!super::led_on(500));
        assert!(super::led_on(1000));
    }
}
