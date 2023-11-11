use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::utils::helpers::{ create_local_client, get_nonce, get_my_address };
use crate::utils::types::structs::NonceOracle;
use crate::utils::types::events::NewBlockEvent;

pub fn start_nonce_oracle(
    oracle: Arc<Mutex<NonceOracle>>,
    mut new_block_receive: tokio::sync::broadcast::Receiver<NewBlockEvent>
) {
    let oracle = oracle.clone();
    tokio::spawn(async move {
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    // we reconnect by restarting the loop
                    continue;
                }
            };

            // start the nonce oracle by subscribing to new blocks
            while let Ok(event) = new_block_receive.recv().await {

                let _latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };

                // get the nonce
                let nonce: Option<U256> = match get_nonce(client.clone(), get_my_address()).await {
                    Ok(Some(nonce)) => Some(U256::from(nonce)), // Convert u64 to U256 here
                    Ok(None) => None,
                    Err(e) => {
                        log::info!("Error getting nonce: {}", e);
                        None // Return a default value
                    }
                };

                // update the nonce
                {
                    let mut oracle_guard = oracle.lock().await;
                    oracle_guard.update_nonce(nonce.unwrap_or_default());
                }

            } // end of while loop
        } // end of loop
    }); // end of tokio::spawn
}
