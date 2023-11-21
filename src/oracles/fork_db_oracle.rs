use ethers::prelude::*;
use tokio::sync::broadcast;
use revm::db::{ CacheDB, EmptyDB };
use crate::utils::helpers::create_local_client;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::types::{structs::oracles::ForkOracle, events::NewBlockEvent};
use std::sync::Arc;
use tokio::sync::Mutex;



pub fn start_forkdb_oracle(
    oracle: Arc<Mutex<ForkOracle>>,
    mut new_block_receive: broadcast::Receiver<NewBlockEvent>
) {
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

            while let Ok(event) = new_block_receive.recv().await {
                let latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };

                let latest_block_number = Some(
                    BlockId::Number(BlockNumber::Number(latest_block.number))
                );

                // initialize an empty cache db
                let cache_db = CacheDB::new(EmptyDB::default());

                // setup the backend
                let fork_factory = ForkFactory::new_sandbox_factory(
                    client.clone(),
                    cache_db,
                    latest_block_number
                );

                let fork_db = fork_factory.new_sandbox_fork();
                
                // update fork_db
                let mut oracle_guard = oracle.lock().await;
                oracle_guard.update_fork_db(fork_db);
                drop(oracle_guard);
            } // end of while loop
        } // end of loop
    }); // end of tokio::spawn
}
