use ethers::prelude::*;

use ethers::types::transaction::eip2930::AccessList;


// Holds the data for a transaction
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


// Holds the data for our snipe transaction
#[derive(Debug, Clone, PartialEq)]
pub struct SnipeTx {
    pub tx_call_data: Bytes,
    pub sniper_contract_address: Address,
    pub access_list: AccessList,
    pub gas_used: u64,
    pub pool: Pool,
    pub amount_in: U256,
    pub target_amount_weth: U256,
    pub block_bought: U64,
    pub pending_tx: Transaction,
}

impl SnipeTx {
    pub fn new(
        tx_call_data: Bytes,
        sniper_contract_address: Address,
        access_list: AccessList,
        gas_used: u64,
        pool: Pool,
        amount_in: U256,
        target_amount_weth: U256,
        block_bought: U64,
        pending_tx: Option<Transaction>
    ) -> Self {
        SnipeTx {
            tx_call_data,
            sniper_contract_address,
            access_list,
            gas_used,
            pool,
            amount_in,
            target_amount_weth,
            block_bought,
            pending_tx: pending_tx.unwrap_or_default(),
        }
    }
}


// Holds Pool Information
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pool {
    pub address: Address,
    pub token_0: Address,
    pub token_1: Address,
    pub weth_liquidity: U256,
}

impl Pool {
    pub fn new(address: Address, token_a: Address, token_b: Address, weth_liquidity: U256) -> Pool {
        let token_0 = token_a;
        let token_1 = token_b;

        Pool {
            address,
            token_0,
            token_1,
            weth_liquidity,
        }
    }
}


// Sell Oracle, Holds All the token information we currently want to sell
#[derive(Debug, Clone, PartialEq)]
pub struct SellOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl SellOracle {
    pub fn new() -> Self {
        SellOracle { tx_data: Vec::new() }
    }

    // Add a new tx_data to the vector
    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.push(tx_data);
    }

    // Remove a tx_data from the vector
    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.retain(|x| x != &tx_data);
    }

    // Update the target amount to sell for a specific tx_data
    pub fn update_target_amount(&mut self, snipe_tx: SnipeTx, target_amount: U256) {
        for tx in &mut self.tx_data {
            if tx == &snipe_tx {
                tx.target_amount_weth = target_amount;
            }
        }
    }
}


// Same as above but for AntiRug
#[derive(Debug, Clone, PartialEq)]
pub struct AntiRugOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl AntiRugOracle {
    pub fn new() -> Self {
        AntiRugOracle { tx_data: Vec::new() }
    }

    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.push(tx_data);
    }

    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.retain(|x| x != &tx_data);
    }
}


// Same as above but for Retry
#[derive(Debug, Clone, PartialEq)]
pub struct RetryOracle {
    pub tx_data: Vec<SnipeTx>,
}

impl RetryOracle {
    pub fn new() -> Self {
        RetryOracle { tx_data: Vec::new() }
    }

    pub fn add_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.push(tx_data);
    }

    pub fn remove_tx_data(&mut self, tx_data: SnipeTx) {
        self.tx_data.retain(|x| x != &tx_data);
    }
}
