use super::structs::pool::Pool;
use ethers::types::Transaction;



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