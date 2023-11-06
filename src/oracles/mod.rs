use ethers::prelude::*;
use crate::utils::types::{structs::*, events::NewBlockEvent};
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};

pub mod block_oracle;
pub mod pair_oracle;

pub use block_oracle::*;
pub use pair_oracle::*;

pub mod sell_oracle;
pub use sell_oracle::*;

pub mod anti_rug_oracle;
pub use anti_rug_oracle::*;


pub mod mempool_stream;
pub use mempool_stream::*;


// monitor the status of the oracles
// for now we just log the number of tokens we hold
pub fn oracle_status(
    sell_oracle: Arc<Mutex<SellOracle>>,
    anti_rug_oracle: Arc<Mutex<AntiRugOracle>>,
    mut new_block_receive: broadcast::Receiver<NewBlockEvent>
) {

    tokio::spawn(async move {

        loop {
            
            while let Ok(event) = new_block_receive.recv().await {
                let _latest_block = match event {
                    NewBlockEvent::NewBlock { latest_block } => latest_block,
                };
                
                let sell_oracle = sell_oracle.lock().await;
                let sell_oracle_tx_len = sell_oracle.get_tx_len();
                drop(sell_oracle);

                let anti_rug_oracle = anti_rug_oracle.lock().await;
                let anti_rug_oracle_tx_len = anti_rug_oracle.get_tx_len();
                drop(anti_rug_oracle);

                log::info!("Sell Oracle: {:?} txs", sell_oracle_tx_len);
                log::info!("Anti Rug Oracle: {:?} txs", anti_rug_oracle_tx_len);

            }
            
        }
    });
    
}




/// Calculate the next block base fee
// based on math provided here: https://ethereum.stackexchange.com/questions/107173/how-is-the-base-fee-per-gas-computed-for-a-new-block
pub fn calculate_next_block_base_fee(block: Block<TxHash>) -> U256 {
    // Get the block base fee per gas
    let current_base_fee_per_gas = block.base_fee_per_gas.unwrap_or_default();

    // Get the mount of gas used in the block
    let current_gas_used = block.gas_used;

    let current_gas_target = block.gas_limit / 2;

    if current_gas_used == current_gas_target {
        current_base_fee_per_gas
    } else if current_gas_used > current_gas_target {
        let gas_used_delta = current_gas_used - current_gas_target;
        let base_fee_per_gas_delta =
            current_base_fee_per_gas * gas_used_delta / current_gas_target / 8;

        return current_base_fee_per_gas + base_fee_per_gas_delta;
    } else {
        let gas_used_delta = current_gas_target - current_gas_used;
        let base_fee_per_gas_delta =
            current_base_fee_per_gas * gas_used_delta / current_gas_target / 8;

        return current_base_fee_per_gas - base_fee_per_gas_delta;
    }
}