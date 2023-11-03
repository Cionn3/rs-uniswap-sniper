pub mod bot_config;
pub mod bot_runner;
pub mod send_tx;
pub mod send_normal_tx;

use ethers::prelude::*;
use ethers::types::transaction::eip2930::AccessList;


#[derive(Debug, Clone)]
pub struct TxData {
    pub tx_call_data: Bytes,
    pub access_list: AccessList,
    pub gas_used: u64,
    pub expected_amount: U256,
    pub sniper_contract_address: Address,
    pub pending_tx: Transaction,
    pub frontrun_or_backrun: U256,
}

impl TxData {
    pub fn new(
        tx_call_data: Bytes,
        access_list: AccessList,
        gas_used: u64,
        expected_amount: U256,
        sniper_contract_address: Address,
        pending_tx: Transaction,
        frontrun_or_backrun: U256,
    ) -> Self {
        TxData {
            tx_call_data,
            access_list,
            gas_used,
            expected_amount,
            sniper_contract_address,
            pending_tx,
            frontrun_or_backrun,
        }
    }
}
