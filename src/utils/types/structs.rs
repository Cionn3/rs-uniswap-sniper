use ethers::prelude::*;

use ethers::types::transaction::eip2930::AccessList;

// Holds the data for a transaction
#[derive(Debug, Clone)]
pub struct TxData {
    pub tx_call_data: Bytes,
    pub access_list: AccessList,
    pub gas_used: u64,
    pub expected_amount: U256,
    pub sniper_contract_address: Address,
    pub pending_tx: Transaction,
    pub frontrun_or_backrun: U256,
}

impl TxData {
    pub fn new(
        tx_call_data: Bytes,
        access_list: AccessList,
        gas_used: u64,
        expected_amount: U256,
        sniper_contract_address: Address,
        pending_tx: Transaction,
        frontrun_or_backrun: U256
    ) -> Self {
        TxData {
            tx_call_data,
            access_list,
            gas_used,
            expected_amount,
            sniper_contract_address,
            pending_tx,
            frontrun_or_backrun,
        }
    }
}

// Holds the data for our snipe transaction
#[derive(Debug, Clone, PartialEq)]
pub struct SnipeTx {
    pub tx_call_data: Bytes,
    pub sniper_contract_address: Address,
    pub access_list: AccessList,
    pub gas_used: u64,
    pub buy_cost: U256,
    pub pool: Pool,
    pub amount_in: U256,
    pub expected_amount_of_tokens: U256,
    pub target_amount_weth: U256,
    pub block_bought: U64,
    pub pending_tx: Transaction,
    pub snipe_retries: u8,
    pub attempts_to_sell: u8,
    pub is_pending: bool,
    pub retry_pending: bool,
    pub reason: u8,
    pub got_initial_out: bool,
}

impl SnipeTx {
    pub fn new(
        tx_call_data: Bytes,
        sniper_contract_address: Address,
        access_list: AccessList,
        gas_used: u64,
        buy_cost: U256,
        pool: Pool,
        amount_in: U256,
        expected_amount_of_tokens: U256,
        target_amount_weth: U256,
        block_bought: U64,
        pending_tx: Option<Transaction>,
        snipe_retries: u8,
        attempts_to_sell: u8,
        is_pending: bool,
        retry_pending: bool,
        reason: u8,
        got_initial_out: bool
    ) -> Self {
        SnipeTx {
            tx_call_data,
            sniper_contract_address,
            access_list,
            gas_used,
            buy_cost,
            pool,
            amount_in,
            expected_amount_of_tokens,
            target_amount_weth,
            block_bought,
            pending_tx: pending_tx.unwrap_or_default(),
            snipe_retries,
            attempts_to_sell,
            is_pending,
            retry_pending,
            reason,
            got_initial_out,
        }
    }
}

// Holds Pool Information
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pool {
    pub address: Address,
    pub token_0: Address,
    pub token_1: Address,
    pub weth_liquidity: U256,
}

impl Pool {
    pub fn new(address: Address, token_a: Address, token_b: Address, weth_liquidity: U256) -> Pool {
        let token_0 = token_a;
        let token_1 = token_b;

        Pool {
            address,
            token_0,
            token_1,
            weth_liquidity,
        }
    }
}

// Nonce Oracle, Holds the nonce for the next transaction
// Before we send any tx we notify the oracle to update the nonce
#[derive(Debug, Clone, PartialEq)]
pub struct NonceOracle {
    pub nonce: U256,
}

impl NonceOracle {
    pub fn new() -> Self {
        NonceOracle { nonce: U256::zero() }
    }

    // updates the nonce
    pub fn update_nonce(&mut self, nonce: U256) {
        self.nonce = nonce;
    }

    // get the current nonce
    pub fn get_nonce(&self) -> U256 {
        self.nonce
    }
}

// Sell Oracle, Holds All the token information we currently want to sell
#[derive(Debug, Clone, PartialEq)]
pub struct SellOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl SellOracle {
    pub fn new() -> Self {
        SellOracle { tx_data: Vec::new() }
    }

    // get the lenght of the vector
    pub fn get_tx_len(&self) -> usize {
        self.tx_data.len()
    }

    // Add a new tx_data to the vector
    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        if !self.tx_data.contains(&tx_data) {
            self.tx_data.push(tx_data);
        }
    }

    // Remove a tx_data from the vector
    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        log::info!("Sell Oracle: Removed {:?}", tx_data.pool.token_1);
        self.tx_data.retain(|x| x.pool.token_1 != tx_data.pool.token_1);
    }

    // Update the target amount to sell for a specific tx_data
    pub fn update_target_amount(&mut self, snipe_tx: SnipeTx, target_amount: U256) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.target_amount_weth = target_amount;
            }
        }
    }

    // set the tx if its pending or not
    pub fn set_tx_is_pending(&mut self, snipe_tx: SnipeTx, tx_is_pending: bool) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.retry_pending = tx_is_pending;
                log::info!("Tx Set to {:?}", tx_is_pending);
            }
        }
    }

    // Updates the retries counter
    pub fn update_attempts_to_sell(&mut self, snipe_tx: SnipeTx) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.attempts_to_sell += 1;
                log::warn!("Sell Oracle: Updated retry counter to: {:?}", tx.attempts_to_sell);
            }
        }
    }

    // updates whether we have got the initial out as profit
    pub fn update_got_initial_out(&mut self, snipe_tx: SnipeTx, got_initial_out: bool) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.got_initial_out = got_initial_out;
                log::warn!("Sell Oracle: Updated got_initial_out to: {:?}", tx.got_initial_out);
            }
        }
    }
}

// Same as above but for AntiRug
#[derive(Debug, Clone, PartialEq)]
pub struct AntiRugOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl AntiRugOracle {
    pub fn new() -> Self {
        AntiRugOracle { tx_data: Vec::new() }
    }

    // get the lenght of the vector
    pub fn get_tx_len(&self) -> usize {
        self.tx_data.len()
    }

    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        if !self.tx_data.contains(&tx_data) {
            self.tx_data.push(tx_data);
        }
    }

    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        log::info!("Anti-Rug Oracle: Removed {:?}", tx_data.pool.token_1);
        self.tx_data.retain(|x| x.pool.token_1 != tx_data.pool.token_1);
    }
}

// Same as above but for Retry
#[derive(Debug, Clone, PartialEq)]
pub struct RetryOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl RetryOracle {
    pub fn new() -> Self {
        RetryOracle { tx_data: Vec::new() }
    }

    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        if !self.tx_data.contains(&tx_data) {
            self.tx_data.push(tx_data);
        }
    }

    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        log::info!("Retry Oracle: Removed {:?}", tx_data.pool.token_1);
        self.tx_data.retain(|x| x.pool.token_1 != tx_data.pool.token_1);
    }

    // Updates the retries counter
    pub fn update_retry_counter(&mut self, snipe_tx: SnipeTx) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.snipe_retries += 1;
                log::warn!("Retry Oracle: Updated retry counter to: {:?}", tx.snipe_retries);
            }
        }
    }

    // update the reason why the swap failed
    // 0 = no reason (default when SnipeTx is created)
    // 1 = swap failed (probably trading is not open yet)
    // 2 = bundle not included (probably due to competition)
    pub fn update_reason(&mut self, snipe_tx: SnipeTx, reason: u8) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.reason = reason;
                log::warn!("Retry Oracle: Updated reason to: {:?}", tx.reason);
            }
        }
    }

    // set the tx if its pending or not
    pub fn set_tx_is_pending(&mut self, snipe_tx: SnipeTx, tx_is_pending: bool) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.retry_pending = tx_is_pending;
                log::info!("Tx Set to {:?}", tx_is_pending);
            }
        }
    }
}
