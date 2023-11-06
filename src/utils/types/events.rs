use super::structs::*;
use crate::oracles::block_oracle::BlockInfo;
use ethers::types::Transaction;



// New block event from the block oracle
#[derive(Debug, Clone)]
pub enum NewBlockEvent {
    NewBlock {
        latest_block: BlockInfo,
    },
}

// New pair event from the pair oracle
#[derive(Debug, Clone)]
pub enum NewPairEvent {
    NewPairWithTx {
        pool: Pool,
        tx: Transaction,
    },
}


// New mempool event from the mempool stream
#[derive(Debug, Clone)]
pub enum MemPoolEvent {
    NewTx {
        tx: Transaction,
    },
}