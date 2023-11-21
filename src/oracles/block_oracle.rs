use std::sync::Arc;
use tokio::sync::broadcast::Sender;

use ethers::prelude::*;
use tokio::sync::RwLock;
use crate::utils::types::events::NewBlockEvent;



#[derive(Debug, Clone, Default)]
pub struct BlockInfo {
    pub number: U64,
    pub timestamp: U256,
    pub base_fee: U256,
}

impl BlockInfo {
    // Create a new `BlockInfo` instance
    pub fn new(number: U64, timestamp: U256, base_fee: U256) -> Self {
        Self {
            number,
            timestamp,
            base_fee,
        }
    }

    #[allow(dead_code)]
    // Find the next block ahead of `prev_block`
    pub fn find_next_block_info(prev_block: Block<TxHash>) -> Self {
        let number = prev_block.number.unwrap_or_default() + 1;
        let timestamp = prev_block.timestamp + 12;
        let base_fee = calculate_next_block_base_fee(prev_block);
        Self {
            number,
            timestamp,
            base_fee,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BlockOracle {
    pub latest_block: BlockInfo,
    pub next_block: BlockInfo,
}

impl BlockOracle {
    // Create new latest block oracle
    pub async fn new(client: &Arc<Provider<Ws>>) -> Result<Self, ProviderError> {
        let latest_block = match client.get_block(BlockNumber::Latest).await {
            Ok(b) => b,
            Err(e) => {
                return Err(e);
            }
        };

        let lb = if let Some(b) = latest_block {
            b
        } else {
            return Err(ProviderError::CustomError("Block not found".to_string()));
        };

        // latets block info
        let number = lb.number.unwrap();
        let timestamp = lb.timestamp;
        let base_fee = lb.base_fee_per_gas.unwrap_or_default();

        let latest_block = BlockInfo::new(number, timestamp, base_fee);

        // next block info
        let number = number + 1;
        let timestamp = timestamp + 12;
        let base_fee = calculate_next_block_base_fee(lb);

        let next_block = BlockInfo::new(number, timestamp, base_fee);

        Ok(BlockOracle {
            latest_block,
            next_block,
        })
    }

    // Updates block's number
    pub fn update_block_number(&mut self, block_number: U64) {
        self.latest_block.number = block_number;
        self.next_block.number = block_number + 1;
    }

    // Updates block's timestamp
    pub fn update_block_timestamp(&mut self, timestamp: U256) {
        self.latest_block.timestamp = timestamp;
        self.next_block.timestamp = timestamp + 12;
    }

    // Updates block's base fee
    pub fn update_base_fee(&mut self, latest_block: Block<TxHash>) {
        self.latest_block.base_fee = latest_block.base_fee_per_gas.unwrap_or_default();
        self.next_block.base_fee = calculate_next_block_base_fee(latest_block);
    }
}

// Update latest block variable whenever we recieve a new block
//
// Arguments:
// * `oracle`: oracle to update
pub fn start_block_oracle(
    oracle: &mut Arc<RwLock<BlockOracle>>,
    new_block_sender: Sender<NewBlockEvent>
) {
    let next_block_clone = oracle.clone();

    tokio::spawn(async move {
        // loop so we can reconnect if the websocket connection is lost
        loop {
            let client = crate::utils::helpers::create_local_client().await.unwrap();

            let mut block_stream = if let Ok(stream) = client.subscribe_blocks().await {
                stream
            } else {
                continue;
            };

            while let Some(block) = block_stream.next().await {
                // lock the RwLock for write access and update the variable
                {
                    let mut lock = next_block_clone.write().await;
                    lock.update_block_number(block.number.unwrap());
                    lock.update_block_timestamp(block.timestamp);
                    lock.update_base_fee(block);

                    let latest_block = &lock.latest_block;
                    let next_block = &lock.next_block;

                    
                    log::info!("New Block: {}, Next Block: {}", latest_block.number,
                        next_block.number);

                    // send the new block through channel
                    new_block_sender
                        .send(NewBlockEvent::NewBlock {
                            latest_block: latest_block.clone(),
                        })
                        .unwrap();
                } // remove write lock due to being out of scope here
            }
        }
    });
}


/// Calculate the next block base fee
// based on math provided here: https://ethereum.stackexchange.com/questions/107173/how-is-the-base-fee-per-gas-computed-for-a-new-block
fn calculate_next_block_base_fee(block: Block<TxHash>) -> U256 {
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