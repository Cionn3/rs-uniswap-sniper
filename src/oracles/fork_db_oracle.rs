use ethers::prelude::*;
use tokio::sync::broadcast;
use revm::db::{ CacheDB, EmptyDB };
use crate::utils::helpers::create_local_client;
use crate::forked_db::fork_factory::ForkFactory;
use crate::utils::types::structs::oracles::ForkOracle;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::block_oracle::BlockInfo;



pub fn start_forkdb_oracle(
    oracle: Arc<RwLock<ForkOracle>>,
    mut new_block_receive: broadcast::Receiver<BlockInfo>
) {
    tokio::spawn(async move {
        loop {
            let client = match create_local_client().await {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to create local client: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            while let Ok(latest_block) = new_block_receive.recv().await {
                
                let mut oracle_guard = oracle.write().await;
                
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
                oracle_guard.update_fork_db(fork_db);
                drop(oracle_guard);
            } // end of while loop
        } // end of loop
    }); // end of tokio::spawn
}
