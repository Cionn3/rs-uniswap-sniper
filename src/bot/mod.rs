use std::sync::Arc;
use tokio::sync::RwLock;
use ethers::types::U256;
use crate::utils::types::structs::{bot::Bot, snipe_tx::SnipeTx};




pub mod bot_start;
pub mod bot_sniper;
//pub mod send_normal_tx;
pub mod send_tx;






// ** HELPER FUNCTIONS ** 


pub async fn remove_tx_from_oracles(
    bot: Arc<RwLock<Bot>>,
    snipe_tx: SnipeTx
) {
    let mut bot_guard = bot.write().await;
    bot_guard.remove_tx_data(snipe_tx.clone()).await;
    drop(bot_guard);
}

pub async fn add_tx_to_oracles(
    bot: Arc<RwLock<Bot>>,
    snipe_tx: SnipeTx
) {
    let mut bot_guard = bot.write().await;
    bot_guard.add_tx_data(snipe_tx.clone()).await;
    drop(bot_guard);
    
}



pub fn calculate_miner_tip(pending_tx_priority_fee: U256) -> U256 {
    let ten_gwei = U256::from(10000000000u128);

    let miner_tip;

    match pending_tx_priority_fee {
        // if pending fee is 0
        fee if fee == (0).into() => {
            miner_tip = ten_gwei;
        }
        // for anything else
        _ => {
            miner_tip = (pending_tx_priority_fee * 15) / 10; // +50%
        }
    }

    miner_tip
}