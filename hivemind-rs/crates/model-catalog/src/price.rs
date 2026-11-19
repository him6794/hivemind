//! Prepaid token quota for LLM inference.
//!
//! An account buys quota per model, separately for prompt (input) and
//! completion (output) tokens. Each kind is bought in blocks of
//! `TOKENS_PER_BLOCK` tokens, and a block is charged in advance at the model's
//! price per million tokens for that kind.
//!
//! A request reserves what it could use: a full context window of prompt
//! tokens and `max_new_tokens` of completion tokens. The server rejects a
//! prompt that does not fit in the window before it generates, and it stops
//! generation at `max_new_tokens`, so neither reservation can be exceeded.
//! A block is charged only when the quota already paid for cannot cover the
//! reservation. Tokens the request does not use return to the quota when it
//! settles.

use crate::catalog::ModelEntry;

/// Tokens in one billing block. Input and output are each bought in blocks of this size.
pub const TOKENS_PER_BLOCK: u64 = 1_000_000;

/// Refusals from the quota functions. A refused call leaves the quota unchanged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuotaError {
    #[error("the charge does not fit in the CPT ledger")]
    Overflow,
    #[error("max_new_tokens must be at least 1 and below the model's context window")]
    InvalidMaxTokens,
    #[error("the model's price is negative")]
    InvalidPrice,
    #[error("the debit was refused")]
    DebitRefused,
    #[error("the request reported more tokens than it reserved")]
    Overrun,
}

/// A request admitted against the quota. Pass it to `BlockQuota::settle` exactly once.
#[derive(Debug, PartialEq, Eq)]
pub struct Admission {
    input_blocks: u64,
    output_blocks: u64,
    charge_cpt: i64,
    reserved_input: u64,
    reserved_output: u64,
}

impl Admission {
    /// Input blocks charged when the request was admitted.
    pub fn input_blocks(&self) -> u64 {
        self.input_blocks
    }

    /// Output blocks charged when the request was admitted.
    pub fn output_blocks(&self) -> u64 {
        self.output_blocks
    }

    /// CPT debited for the blocks above.
    pub fn charge_cpt(&self) -> i64 {
        self.charge_cpt
    }

    /// Prompt tokens reserved: the model's context window.
    pub fn reserved_input_tokens(&self) -> u64 {
        self.reserved_input
    }

    /// Completion tokens reserved: `max_new_tokens`.
    pub fn reserved_output_tokens(&self) -> u64 {
        self.reserved_output
    }
}

/// Prepaid, unused quota for one account and one model.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BlockQuota {
    input_available: u64,
    output_available: u64,
}

impl BlockQuota {
    /// Prompt tokens paid for and not yet used or reserved.
    pub fn input_available(&self) -> u64 {
        self.input_available
    }

    /// Completion tokens paid for and not yet used or reserved.
    pub fn output_available(&self) -> u64 {
        self.output_available
    }

    /// Admits a request. Blocks are charged for any shortfall in the
    /// reservation. `debit` is called once with the total CPT, which is 0 when
    /// no block is needed. If `debit` returns false, nothing changes.
    pub fn admit(
        &mut self,
        model: &ModelEntry,
        max_new_tokens: u32,
        debit: impl FnOnce(i64) -> bool,
    ) -> Result<Admission, QuotaError> {
        let window = u64::from(model.context_length);
        let max_new = u64::from(max_new_tokens);
        if max_new == 0 || max_new >= window {
            return Err(QuotaError::InvalidMaxTokens);
        }
        let input_price = price(model.prices.input_cpt_per_million_tokens)?;
        let output_price = price(model.prices.output_cpt_per_million_tokens)?;

        let input_blocks = blocks_short(window, self.input_available);
        let output_blocks = blocks_short(max_new, self.output_available);
        let charge_cpt = block_charge(input_blocks, input_price)?
            .checked_add(block_charge(output_blocks, output_price)?)
            .ok_or(QuotaError::Overflow)?;
        let input_after = remaining_after(self.input_available, input_blocks, window)?;
        let output_after = remaining_after(self.output_available, output_blocks, max_new)?;

        if !debit(charge_cpt) {
            return Err(QuotaError::DebitRefused);
        }
        self.input_available = input_after;
        self.output_available = output_after;
        Ok(Admission {
            input_blocks,
            output_blocks,
            charge_cpt,
            reserved_input: window,
            reserved_output: max_new,
        })
    }

    /// Closes an admission and returns the unused reservation to the quota.
    ///
    /// If the request reports more tokens than it reserved, the call fails and
    /// the reservation stays spent. The reservation is the most the request can
    /// legitimately use, so an overrun is a fault, not a refund.
    pub fn settle(
        &mut self,
        admission: Admission,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<(), QuotaError> {
        if input_tokens > admission.reserved_input || output_tokens > admission.reserved_output {
            return Err(QuotaError::Overrun);
        }
        let input = self
            .input_available
            .checked_add(admission.reserved_input - input_tokens)
            .ok_or(QuotaError::Overflow)?;
        let output = self
            .output_available
            .checked_add(admission.reserved_output - output_tokens)
            .ok_or(QuotaError::Overflow)?;
        self.input_available = input;
        self.output_available = output;
        Ok(())
    }
}

/// Blocks needed so that `available` covers `reserved`.
fn blocks_short(reserved: u64, available: u64) -> u64 {
    reserved
        .saturating_sub(available)
        .div_ceil(TOKENS_PER_BLOCK)
}

/// Quota left after buying `blocks` and reserving `reserved` tokens.
fn remaining_after(available: u64, blocks: u64, reserved: u64) -> Result<u64, QuotaError> {
    blocks
        .checked_mul(TOKENS_PER_BLOCK)
        .and_then(|bought| available.checked_add(bought))
        .and_then(|total| total.checked_sub(reserved))
        .ok_or(QuotaError::Overflow)
}

fn block_charge(blocks: u64, price: i64) -> Result<i64, QuotaError> {
    let blocks = i64::try_from(blocks).map_err(|_| QuotaError::Overflow)?;
    blocks.checked_mul(price).ok_or(QuotaError::Overflow)
}

fn price(value: i64) -> Result<i64, QuotaError> {
    if value < 0 {
        Err(QuotaError::InvalidPrice)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{GgufPin, LicensePin, PricePin, SwarmPin};

    const WINDOW: u32 = 8_192;

    fn model(input: i64, output: i64) -> ModelEntry {
        ModelEntry {
            model_id: "test-model".to_owned(),
            display_name: "Test".to_owned(),
            quantization: "Q8_0".to_owned(),
            context_length: WINDOW,
            ram_bytes: 1 << 30,
            gguf: GgufPin {
                file_name: "test.gguf".to_owned(),
                size_bytes: 1 << 20,
                sha256: "0".repeat(64),
            },
            swarm: SwarmPin {
                metainfo_sha256: "0".repeat(64),
                infohash: "0".repeat(40),
                piece_length: 16 << 10,
            },
            license: LicensePin {
                spdx: "MIT".to_owned(),
                text_sha256: "0".repeat(64),
                text_source: "https://example.org/license".to_owned(),
                model_card: "https://example.org/card".to_owned(),
            },
            prices: PricePin {
                input_cpt_per_million_tokens: input,
                output_cpt_per_million_tokens: output,
            },
        }
    }

    #[test]
    fn a_new_account_prepays_one_block_of_each_kind() {
        let m = model(50, 200);
        let mut quota = BlockQuota::default();
        let admission = quota
            .admit(&m, 256, |charge| charge == 250)
            .expect("admitted");
        assert_eq!(admission.input_blocks(), 1);
        assert_eq!(admission.output_blocks(), 1);
        assert_eq!(admission.charge_cpt(), 250);
        assert_eq!(admission.reserved_input_tokens(), u64::from(WINDOW));
        assert_eq!(admission.reserved_output_tokens(), 256);
        assert_eq!(
            quota.input_available(),
            TOKENS_PER_BLOCK - u64::from(WINDOW)
        );
        assert_eq!(quota.output_available(), TOKENS_PER_BLOCK - 256);
    }

    #[test]
    fn a_request_inside_the_paid_quota_is_charged_nothing() {
        let m = model(50, 200);
        let mut quota = BlockQuota::default();
        let first = quota.admit(&m, 256, |_| true).expect("first");
        quota.settle(first, 5, 32).expect("first settles");

        let mut debited = None;
        let second = quota
            .admit(&m, 256, |charge| {
                debited = Some(charge);
                true
            })
            .expect("second");
        assert_eq!(debited, Some(0));
        assert_eq!(second.input_blocks(), 0);
        assert_eq!(second.output_blocks(), 0);
        assert_eq!(
            quota.input_available(),
            TOKENS_PER_BLOCK - 5 - u64::from(WINDOW)
        );
        assert_eq!(quota.output_available(), TOKENS_PER_BLOCK - 32 - 256);
    }

    #[test]
    fn a_block_is_charged_only_when_the_paid_quota_cannot_cover_the_reservation() {
        let m = model(50, 200);

        let mut almost_out = BlockQuota {
            input_available: u64::from(WINDOW) - 1,
            output_available: 256,
        };
        let admission = almost_out
            .admit(&m, 256, |charge| charge == 50)
            .expect("one input block");
        assert_eq!(admission.input_blocks(), 1);
        assert_eq!(admission.output_blocks(), 0);

        let mut exactly_enough = BlockQuota {
            input_available: u64::from(WINDOW),
            output_available: 256,
        };
        let admission = exactly_enough
            .admit(&m, 256, |charge| charge == 0)
            .expect("no block");
        assert_eq!(admission.input_blocks(), 0);
        assert_eq!(admission.output_blocks(), 0);
    }

    #[test]
    fn a_refused_debit_leaves_the_quota_unchanged() {
        let m = model(50, 200);
        let mut quota = BlockQuota::default();
        let before = quota;
        assert_eq!(
            quota.admit(&m, 256, |_| false),
            Err(QuotaError::DebitRefused)
        );
        assert_eq!(quota, before);
    }

    #[test]
    fn a_request_that_produced_nothing_returns_its_whole_reservation() {
        let m = model(50, 200);
        let mut quota = BlockQuota::default();
        let admission = quota.admit(&m, 256, |_| true).expect("admitted");
        quota.settle(admission, 0, 0).expect("settles");
        assert_eq!(quota.input_available(), TOKENS_PER_BLOCK);
        assert_eq!(quota.output_available(), TOKENS_PER_BLOCK);
    }

    #[test]
    fn usage_above_the_reservation_fails_and_the_reservation_stays_spent() {
        let m = model(50, 200);
        let mut quota = BlockQuota::default();
        let admission = quota.admit(&m, 256, |_| true).expect("admitted");
        let spent = quota;
        assert_eq!(
            quota.settle(admission, u64::from(WINDOW) + 1, 0),
            Err(QuotaError::Overrun)
        );
        assert_eq!(quota, spent);
    }

    #[test]
    fn max_new_tokens_must_leave_room_in_the_window() {
        let m = model(50, 200);
        let mut quota = BlockQuota::default();
        assert_eq!(
            quota.admit(&m, 0, |_| true),
            Err(QuotaError::InvalidMaxTokens)
        );
        assert_eq!(
            quota.admit(&m, WINDOW, |_| true),
            Err(QuotaError::InvalidMaxTokens)
        );
        assert!(quota.admit(&m, WINDOW - 1, |_| true).is_ok());
    }

    #[test]
    fn a_negative_price_is_refused_before_anything_changes() {
        let m = model(-1, 200);
        let mut quota = BlockQuota::default();
        assert_eq!(
            quota.admit(&m, 256, |_| true),
            Err(QuotaError::InvalidPrice)
        );
        assert_eq!(quota, BlockQuota::default());
    }

    #[test]
    fn a_charge_beyond_the_ledger_range_is_refused_without_wrapping() {
        let m = model(i64::MAX, i64::MAX);
        let mut quota = BlockQuota::default();
        assert_eq!(quota.admit(&m, 256, |_| true), Err(QuotaError::Overflow));
        assert_eq!(quota, BlockQuota::default());
    }

    #[test]
    fn every_block_is_charged_once_and_the_books_balance() {
        let m = model(50, 200);
        let mut quota = BlockQuota::default();
        let mut charged = 0i64;
        let (mut paid_in, mut paid_out) = (0u64, 0u64);
        let (mut used_in, mut used_out) = (0u64, 0u64);
        for i in 0..10_000u64 {
            let max_new = 1 + (i % 300) as u32;
            let admission = quota
                .admit(&m, max_new, |amount| {
                    charged += amount;
                    true
                })
                .expect("admitted");
            paid_in += admission.input_blocks() * TOKENS_PER_BLOCK;
            paid_out += admission.output_blocks() * TOKENS_PER_BLOCK;

            let prompt = 1 + (i * 7_919) % 8_191;
            let completion = (i * 104_729) % (u64::from(max_new) + 1);
            used_in += prompt;
            used_out += completion;
            quota
                .settle(admission, prompt, completion)
                .expect("usage within the reservation");

            assert_eq!(quota.input_available(), paid_in - used_in);
            assert_eq!(quota.output_available(), paid_out - used_out);
            // Quota never exceeds one block plus one reservation of prepaid slack.
            assert!(quota.input_available() < TOKENS_PER_BLOCK + u64::from(WINDOW));
            assert!(quota.output_available() < TOKENS_PER_BLOCK + 300);
        }
        assert!(
            paid_in / TOKENS_PER_BLOCK > 1,
            "exercises several input blocks"
        );
        let blocks_in = i64::try_from(paid_in / TOKENS_PER_BLOCK).expect("small");
        let blocks_out = i64::try_from(paid_out / TOKENS_PER_BLOCK).expect("small");
        assert_eq!(charged, blocks_in * 50 + blocks_out * 200);
    }
}
