//! Fixed-point math helpers for the Fundable streaming protocol.
//!
//! All internal debt calculations use 18-decimal fixed-point arithmetic to avoid precision
//! loss when streaming tokens with few decimals (e.g. Stellar's 7-decimal SAC).
//!
//! # Scaling Strategy
//!
//! - **Internal math**: 18 decimals (1e18 = 1 token)
//! - **Token amounts**: Native token decimals (e.g. 7 for XLM)
//! - **Conversion**: `scale_amount` (token → 18) and `descale_amount` (18 → token)
//!
//! # Rounding
//!
//! Per SKILL.md §2: rounding direction is deliberate.
//! - `descale_amount` rounds **down** (integer division truncation).
//!   This means the protocol keeps dust — the recipient never receives
//!   more than they're owed.

/// The internal precision for debt calculations (18 decimals).
pub const INTERNAL_DECIMALS: u32 = 18;

/// Scale a token amount from the token's native decimals to 18-decimal
/// fixed-point representation.
///
/// # Arguments
/// * `amount` — The amount in the token's native decimals.
/// * `decimals` — The token's decimal count (must be ≤ 18).
///
/// # Returns
/// The amount scaled to 18 decimals.
///
/// # Panics
/// Panics if `decimals > 18` (should be validated at stream creation).
///
/// # Example
/// ```text
/// scale_amount(1_000_0000, 7)
/// // → 1_000_0000 * 10^(18-7) = 1_000_0000 * 10^11
/// // → 1_000_000_000_000_000_000 (1e18, i.e. 1 token in 18-dec)
/// ```
///
/// `FlowHelpers.scaleAmount(uint256 amount, uint8 decimals)`
pub fn scale_amount(amount: i128, decimals: u32) -> i128 {
    if decimals == INTERNAL_DECIMALS {
        return amount;
    }
    let scale_factor = 10i128.pow(INTERNAL_DECIMALS - decimals);
    amount.checked_mul(scale_factor).expect("scale overflow")
}

/// Descale an 18-decimal fixed-point amount back to the token's native decimals.
///
/// **Rounds down** — the recipient never receives more than owed.
///
/// # Arguments
/// * `amount` — The amount in 18-decimal fixed-point.
/// * `decimals` — The token's decimal count (must be ≤ 18).
///
/// # Returns
/// The amount in the token's native decimals (truncated).
///
/// # Example
/// ```text
/// descale_amount(1_500_000_000_000_000_000, 7)
/// // → 1_500_000_000_000_000_000 / 10^11 = 15_000_000
/// // → 1.5 tokens in 7-decimal representation
/// ```
///
/// `FlowHelpers.descaleAmount(uint256 amount, uint8 decimals)`
pub fn descale_amount(amount: i128, decimals: u32) -> i128 {
    if decimals == INTERNAL_DECIMALS {
        return amount;
    }
    let scale_factor = 10i128.pow(INTERNAL_DECIMALS - decimals);
    amount / scale_factor
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_7_decimals() {
        // 1.0 token with 7 decimals = 10_000_000
        // Scaled to 18 decimals = 10_000_000 * 10^11 = 1e18
        let scaled = scale_amount(10_000_000, 7);
        assert_eq!(scaled, 1_000_000_000_000_000_000);
    }

    #[test]
    fn test_descale_7_decimals() {
        // 1e18 in 18-decimal → 10_000_000 in 7-decimal
        let descaled = descale_amount(1_000_000_000_000_000_000, 7);
        assert_eq!(descaled, 10_000_000);
    }

    #[test]
    fn test_descale_rounds_down() {
        // 1.5e18 → should be 15_000_000 (1.5 tokens in 7-dec)
        let descaled = descale_amount(1_500_000_000_000_000_000, 7);
        assert_eq!(descaled, 15_000_000);

        // Dust: 1e18 + 1 → still 10_000_000 (rounds down)
        let descaled_dust = descale_amount(1_000_000_000_000_000_001, 7);
        assert_eq!(descaled_dust, 10_000_000);
    }

    #[test]
    fn test_18_decimal_passthrough() {
        // 18-decimal token should pass through unchanged
        let amount = 123_456_789_000_000_000_000i128;
        assert_eq!(scale_amount(amount, 18), amount);
        assert_eq!(descale_amount(amount, 18), amount);
    }

    #[test]
    fn test_scale_descale_roundtrip() {
        // Scale then descale should recover the original amount (no dust)
        let original = 42_000_0000i128; // 42 tokens with 7 decimals
        let scaled = scale_amount(original, 7);
        let recovered = descale_amount(scaled, 7);
        assert_eq!(recovered, original);
    }

    #[test]
    fn test_zero() {
        assert_eq!(scale_amount(0, 7), 0);
        assert_eq!(descale_amount(0, 7), 0);
    }

    #[test]
    fn test_scale_6_decimals() {
        // USDC-like: 6 decimals
        // 1.0 USDC = 1_000_000 → scaled = 1_000_000 * 10^12 = 1e18
        let scaled = scale_amount(1_000_000, 6);
        assert_eq!(scaled, 1_000_000_000_000_000_000);
    }
}
