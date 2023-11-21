use ethers::prelude::*;
use ethers::types::transaction::eip2930::AccessList;


// Holds the data for a transaction
#[derive(Debug, Clone)]
pub struct TxData {
    pub tx_call_data: Bytes,
    pub gas_used: u64,
    pub expected_amount: U256,
    pub pending_tx: Transaction,
    pub frontrun_or_backrun: U256,
    pub access_list: AccessList
}

impl TxData {
    pub fn new(
        tx_call_data: Bytes,
        gas_used: u64,
        expected_amount: U256,
        pending_tx: Transaction,
        frontrun_or_backrun: U256,
        access_list: AccessList
    ) -> Self {
        TxData {
            tx_call_data,
            gas_used,
            expected_amount,
            pending_tx,
            frontrun_or_backrun,
            access_list
        }
    }
}