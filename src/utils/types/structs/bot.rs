use ethers::prelude::*;

use tokio::sync::RwLock;
use std::sync::Arc;
use crate::oracles::block_oracle::{ BlockOracle, BlockInfo };
use super::oracles::*;
use crate::forked_db::fork_db::ForkDB;

use super::snipe_tx::SnipeTx;

// Holds all oracles for the bot
#[derive(Debug, Clone)]
pub struct Bot {
    pub block_oracle: Arc<RwLock<BlockOracle>>,
    pub nonce_oracle: Arc<RwLock<NonceOracle>>,
    pub sell_oracle: Arc<RwLock<SellOracle>>,
    pub anti_rug_oracle: Arc<RwLock<AntiRugOracle>>,
    pub retry_oracle: Arc<RwLock<RetryOracle>>,
    pub fork_db_oracle: Arc<RwLock<ForkOracle>>,
}

impl Bot {
    // creates a new instance of bot holding the oracles
    pub fn new(
        block_oracle: Arc<RwLock<BlockOracle>>,
        nonce_oracle: Arc<RwLock<NonceOracle>>,
        sell_oracle: Arc<RwLock<SellOracle>>,
        anti_rug_oracle: Arc<RwLock<AntiRugOracle>>,
        retry_oracle: Arc<RwLock<RetryOracle>>,
        fork_db_oracle: Arc<RwLock<ForkOracle>>
    ) -> Self {
        Bot {
            block_oracle,
            nonce_oracle,
            sell_oracle,
            anti_rug_oracle,
            retry_oracle,
            fork_db_oracle,
        }
    }
    // gets the fork_db
    pub async fn get_fork_db(&self) -> ForkDB {
        let fork_oracle = self.fork_db_oracle.write().await;
        let fork_db = fork_oracle.get_fork_db();
        drop(fork_oracle);

        fork_db
    }

    // returns latest and next block info
    pub async fn get_block_info(&self) -> (BlockInfo, BlockInfo) {
        let block_oracle = self.block_oracle.read().await;
        let latest_block = block_oracle.latest_block.clone();
        let next_block = block_oracle.next_block.clone();
        drop(block_oracle);

        (latest_block, next_block)
    }

    // returns the nonce and updates it
    pub async fn get_nonce(&mut self) -> U256 {
        let mut nonce_oracle = self.nonce_oracle.write().await;
        let nonce = nonce_oracle.get_nonce();
        nonce_oracle.update_nonce(nonce + 1);
        drop(nonce_oracle);

        nonce
    }

    // get tx len of sell oracle
    pub async fn get_sell_oracle_tx_len(&self) -> usize {
        let sell_oracle = self.sell_oracle.write().await;
        let tx_len = sell_oracle.get_tx_len();
        drop(sell_oracle);

        tx_len
    }

    // get tx len of anti-rug oracle
    pub async fn get_anti_rug_oracle_tx_len(&self) -> usize {
        let anti_rug_oracle = self.anti_rug_oracle.write().await;
        let tx_len = anti_rug_oracle.get_tx_len();
        drop(anti_rug_oracle);

        tx_len
    }

    // get all snipe tx data from sell oracle
    pub async fn get_sell_oracle_tx_data(&self) -> Vec<SnipeTx> {
        let sell_oracle = self.sell_oracle.write().await;
        let tx_data = sell_oracle.tx_data.clone();
        drop(sell_oracle);

        tx_data
    }

    // adds a new tx to the sell oracle
    pub async fn add_tx_data(&mut self, tx_data: SnipeTx) {
        let mut sell_oracle = self.sell_oracle.write().await;
        sell_oracle.add_tx_data(tx_data);
        drop(sell_oracle);
    }

    // adds a new tx to the anti-rug oracle
    pub async fn add_anti_rug_tx_data(&mut self, tx_data: SnipeTx) {
        let mut anti_rug_oracle = self.anti_rug_oracle.write().await;
        anti_rug_oracle.add_tx_data(tx_data);
        drop(anti_rug_oracle);
    }

    // removes a tx from the sell oracle
    pub async fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        let mut sell_oracle = self.sell_oracle.write().await;
        sell_oracle.remove_tx_data(tx_data);
        drop(sell_oracle);
    }

    // removes a tx from the anti-rug oracle
    pub async fn remove_anti_rug_tx_data(&mut self, tx_data: SnipeTx) {
        let mut anti_rug_oracle = self.anti_rug_oracle.write().await;
        anti_rug_oracle.remove_tx_data(tx_data);
        drop(anti_rug_oracle);
    }

    #[allow(dead_code)]
    // updated target amount to sell for a specific tx
    pub async fn update_target_amount(&mut self, snipe_tx: SnipeTx, target_amount: U256) {
        let mut sell_oracle = self.sell_oracle.write().await;
        sell_oracle.update_target_amount(snipe_tx, target_amount);
        drop(sell_oracle);
    }

    // sets if a tx is pending or not
    pub async fn set_tx_is_pending(&mut self, snipe_tx: SnipeTx, tx_is_pending: bool) {
        let mut sell_oracle = self.sell_oracle.write().await;
        sell_oracle.set_tx_is_pending(snipe_tx, tx_is_pending);
        drop(sell_oracle);
    }

    // updates attempts to sell counter
    pub async fn update_attempts_to_sell(&mut self, snipe_tx: SnipeTx) {
        let mut sell_oracle = self.sell_oracle.write().await;
        sell_oracle.update_attempts_to_sell(snipe_tx);
        drop(sell_oracle);
    }

    // updates whether we have got the initial out as profit
    pub async fn update_got_initial_out(&mut self, snipe_tx: SnipeTx, got_initial_out: bool) {
        let mut sell_oracle = self.sell_oracle.write().await;
        sell_oracle.update_got_initial_out(snipe_tx, got_initial_out);
        drop(sell_oracle);
    }

    // adds tx data to retry oracle
    pub async fn add_tx_to_retry_oracle(&mut self, tx_data: SnipeTx) {
        let mut retry_oracle = self.retry_oracle.write().await;
        retry_oracle.add_tx_data(tx_data);
        drop(retry_oracle);
    }

    // removes tx data from retry oracle
    pub async fn remove_tx_from_retry_oracle(&mut self, tx_data: SnipeTx) {
        let mut retry_oracle = self.retry_oracle.write().await;
        retry_oracle.remove_tx_data(tx_data);
        drop(retry_oracle);
    }

    // gets all the tx data from retry oracle
    pub async fn get_retry_oracle_tx_data(&self) -> Vec<SnipeTx> {
        let retry_oracle = self.retry_oracle.write().await;
        let tx_data = retry_oracle.tx_data.clone();
        drop(retry_oracle);

        tx_data
    }

    // updates the retry counter for retry oracle
    pub async fn update_retry_counter(&mut self, tx_data: SnipeTx) {
        let mut retry_oracle = self.retry_oracle.write().await;
        retry_oracle.update_retry_counter(tx_data);
        drop(retry_oracle);
    }

    // updates if the tx is pending or not for retry oracle
    pub async fn update_retry_pending(&mut self, tx_data: SnipeTx, pending: bool) {
        let mut retry_oracle = self.retry_oracle.write().await;
        retry_oracle.set_tx_is_pending(tx_data, pending);
        drop(retry_oracle);
    }
}
