use ethers::prelude::*;
use super::pool::Pool;


// Holds the data for our snipe transaction
#[derive(Debug, Clone, PartialEq)]
pub struct SnipeTx {
    pub gas_used: u64,
    pub gas_cost: U256,
    pub pool: Pool,
    pub amount_in: U256,
    pub expected_amount_of_tokens: U256,
    pub target_amount_weth: U256,
    pub block_bought: U64,
    pub snipe_retries: u8,
    pub attempts_to_sell: u8,
    pub is_pending: bool,
    pub retry_pending: bool,
    pub got_initial_out: bool,
}

impl SnipeTx {
    // creates a new snipe tx
    pub fn new(
        gas_used: u64,
        gas_cost: U256,
        pool: Pool,
        amount_in: U256,
        expected_amount_of_tokens: U256,
        target_amount_weth: U256,
        block_bought: U64,
    ) -> Self {
        Self {
            gas_used,
            gas_cost,
            pool,
            amount_in,
            expected_amount_of_tokens,
            target_amount_weth,
            block_bought,
            snipe_retries: 0,
            attempts_to_sell: 0,
            is_pending: false,
            retry_pending: false,
            got_initial_out: false,
        }
    }

    // creates a default snipe tx
    pub fn default(
        pool: Pool,
        target_amount_weth: U256,
        block_bought: U64,
    ) -> Self {
        Self {
            gas_used: 0,
            gas_cost: U256::zero(),
            pool,
            amount_in: U256::zero(),
            expected_amount_of_tokens: U256::zero(),
            target_amount_weth,
            block_bought,
            snipe_retries: 0,
            attempts_to_sell: 0,
            is_pending: false,
            retry_pending: false,
            got_initial_out: false,
        }
    }
}