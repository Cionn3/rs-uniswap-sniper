use tokio::sync::broadcast::Sender;
use tokio::sync::RwLock;
use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::utils::{ helpers::*, types::structs::{ pool::Pool, bot::Bot }, types::events::* };
use crate::utils::constants::*;
use crate::utils::evm::simulate::sim::get_pair;



// Monitor pending txs for new pairs created
pub fn start_pair_oracle(
    bot: Arc<RwLock<Bot>>,
    new_pair_sender: Sender<NewPairEvent>,
    mut new_mempool_receiver: broadcast::Receiver<MemPoolEvent>
) {
    tokio::spawn(async move {
        loop {

            // define transfer method
            let transfer_id: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];
            let approve: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];


            while let Ok(event) = new_mempool_receiver.recv().await {
                let tx = match event {
                    MemPoolEvent::NewTx { tx } => tx,
                };

                // skip transfer transactions
                if tx.input.0.len() >= 4 && tx.input.0[0..4] == transfer_id {
                    // log::info!("skipped Tx with Transfer method: {:?}", tx.hash);
                    continue;
                }

                // skip approve transactions
                if tx.input.0.len() >= 4 && tx.input.0[0..4] == approve {
                    // log::info!("skipped Tx with Transfer method: {:?}", tx.hash);
                    continue;
                }

                // get the block info
                let bot_guard = bot.read().await;
                let (_, next_block) = bot_guard.get_block_info().await;
                let fork_db = bot_guard.get_fork_db().await;
                drop(bot_guard);

                let new_pair_sender = new_pair_sender.clone();

                // now we need to simulate the tx with revm to get the pair address from the logs
                let (pool_address, weth, token_1, weth_reserve) = match
                    get_pair(
                        next_block,
                        &tx,
                        fork_db
                    )
                {
                    Ok(address) => address,
                    Err(_e) => {
                        // log::error!(" {:?}", e);
                        continue;
                    }
                };

                if pool_address == Address::zero() {
                    continue;
                }

                // adjust these numbers as you like
                if weth_reserve < *MIN_WETH_RESERVE {
                    log::error!(
                        "Weth Reserve < {:?} MIN_WETH Token Address:{:?}",
                        convert_wei_to_ether(*MIN_WETH_RESERVE),
                        token_1
                    );
                    continue;
                }

                if weth_reserve > *MAX_WETH_RESERVE {
                    log::error!(
                        "Weth Reserve > {:?} MAX_WETH Token Address {:?}",
                        convert_wei_to_ether(*MAX_WETH_RESERVE),
                        token_1
                    );
                    continue;
                }

                // create a new pool
                // token_a is always weth
                let pool = Pool::new(
                    pool_address,
                    weth, // token_0 is always weth
                    token_1, // token_1 is the shitcoin
                    weth_reserve
                );

                log::info!("New Pool Found!🚀");
                log::info!("Pool Address: {:?}", pool.address);
                log::info!("Token Address: {:?}", pool.token_1);

                // send the new pair event
                new_pair_sender
                    .send(NewPairEvent::NewPairWithTx { pool: pool, tx: tx.clone() })
                    .unwrap();
            }
        } // end of main loop
    });
}
