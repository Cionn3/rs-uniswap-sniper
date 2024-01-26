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
    bot_guard.remove_anti_rug_tx_data(snipe_tx.clone()).await;
    drop(bot_guard);
}

pub async fn add_tx_to_oracles(
    bot: Arc<RwLock<Bot>>,
    snipe_tx: SnipeTx
) {
    let mut bot_guard = bot.write().await;
    bot_guard.add_tx_data(snipe_tx.clone()).await;
    bot_guard.add_anti_rug_tx_data(snipe_tx.clone()).await;
    drop(bot_guard);
    
}



pub fn calculate_miner_tip(pending_tx_priority_fee: U256) -> U256 {
    let point_one_gwei = U256::from(100000000u128); // 0.1 gwei
    let point_five_gwei = U256::from(500000000u128); // 0.5 gwei
    let one_gwei = U256::from(1000000000u128); // 1 gwei
    let two_gwei = U256::from(2000000000u128); // 2 gwei
    let three_gwei = U256::from(3000000000u128); // 3
    let ten_gwei = U256::from(10000000000u128); // 10 gwei

    let miner_tip;

    // match pending_tx_priorite_fee to the different lvls we set
    match pending_tx_priority_fee {
        // if pending fee is 0
        fee if fee == (0).into() => {
            miner_tip = ten_gwei; // 10 gwei
        }
        // if pending fee is between  0 ish and 0.1 gwei
        fee if fee < point_one_gwei => {
            miner_tip = fee * 200; // maximum 20 gwei
        }
        // if pending fee is between 0.1 and 0.5 gwei
        fee if fee >= point_one_gwei && fee < point_five_gwei => {
            miner_tip = fee * 50; // maximum 25 gwei
        }
        // if fee is between 0.5 and 1 gwei
        fee if fee >= point_five_gwei && fee < one_gwei => {
            miner_tip = fee * 20; // maximum 20 gwei
        }
        // if pending fee is between 1 and 2 gwei
        fee if fee >= one_gwei && fee < two_gwei => {
            miner_tip = fee * 10; // maximum 20 gwei
        }
        // if fee is between 2 and 3 gwei
        fee if fee >= two_gwei && fee < three_gwei => {
            miner_tip = fee * 10; // maximum 30 gwei
        }
        fee if fee >= three_gwei && fee < ten_gwei => {
            miner_tip = fee * 5; // maximum 50 gwei
        }
        // for anything else
        _ => {
            miner_tip = (pending_tx_priority_fee * 15) / 10; // +50%
        }
    }

    miner_tip
}