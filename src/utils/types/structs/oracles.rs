use ethers::prelude::*;
use super::snipe_tx::SnipeTx;
use crate::forked_db::fork_db::ForkDB;
use super::pool::Pool;


// New Pair, Holds the pool and the transaction from the pair oracle
#[derive(Debug, Clone, PartialEq)]
pub struct NewPair {
    pub pool: Pool,
    pub tx: Transaction,
}

// ForkOracle
// Creates a new backend connection in every new block
#[derive(Debug, Clone)]
pub struct ForkOracle {
    pub fork_db: ForkDB,
}

impl ForkOracle {
    pub fn new(fork_db: ForkDB) -> Self {
        Self { fork_db }
    }

    pub fn update_fork_db(&mut self, fork_db: ForkDB) {
        self.fork_db = fork_db;
        
    }

    pub fn get_fork_db(&self) -> ForkDB {
        self.fork_db.clone()
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
        log::trace!("Retry Oracle: Removed {:?}", tx_data.pool.token_1);
        self.tx_data.retain(|x| x.pool.token_1 != tx_data.pool.token_1);
    }

    // Updates the retries counter
    pub fn update_retry_counter(&mut self, snipe_tx: SnipeTx) {
        for tx in &mut self.tx_data {
            if tx.pool.token_1 == snipe_tx.pool.token_1 {
                tx.snipe_retries += 1;
                log::trace!("Retry Oracle: Updated retry counter to: {:?}", tx.snipe_retries);
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
