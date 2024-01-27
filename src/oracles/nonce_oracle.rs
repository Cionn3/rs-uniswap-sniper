use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::utils::helpers::create_local_client;
use crate::utils::types::structs::oracles::NonceOracle;
use crate::utils::constants::CALLER_ADDRESS;


use super::block_oracle::BlockInfo;

pub fn start_nonce_oracle(
    oracle: Arc<RwLock<NonceOracle>>,
    mut new_block_receive: tokio::sync::broadcast::Receiver<BlockInfo>
) {
    let oracle = oracle.clone();
    tokio::spawn(async move {
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    // we reconnect by restarting the loop
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            // start the nonce oracle by subscribing to new blocks
            while let Ok(latest_block) = new_block_receive.recv().await {
                let block_id = Some(BlockId::Number(BlockNumber::Number(latest_block.number)));

                let mut oracle_guard = oracle.write().await;
                // get the nonce
                let nonce = match client
                    .get_transaction_count(*CALLER_ADDRESS, block_id).await {
                        Ok(nonce) => nonce,
                        Err(e) => {
                            // this should not happen
                            log::error!("Failed to get nonce: {}", e);
                            continue;
                        }
                    };
                    

                // update the nonce
                oracle_guard.update_nonce(nonce);
                drop(oracle_guard);
            } // end of while loop
        } // end of loop
    }); // end of tokio::spawn
}
