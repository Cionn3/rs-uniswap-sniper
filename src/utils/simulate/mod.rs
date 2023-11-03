pub mod simulate;
pub use simulate::*;

pub mod is_safu;
pub use is_safu::*;

pub mod access_list;
pub use access_list::*;
pub mod sim_fns;

use ethers::prelude::*;
use ethers::abi::parse_abi;
use ethers::types::transaction::eip2930::AccessList;
use revm::EVM;
use crate::forked_db::fork_db::ForkDB;
use crate::oracles::block_oracle::BlockInfo;
use revm::primitives::{ ExecutionResult, Output, TransactTo, AccountInfo };
use revm::db::{ CacheDB, EmptyDB };
use crate::utils::helpers::*;
use std::sync::Arc;
use crate::oracles::pair_oracle::Pool;
use revm::primitives::{ Address as rAddress, Bytecode, U256 as rU256 };
use anyhow::anyhow;

// simulate a token call to the pool address
// returns token0 and token1
fn simulate_token_call(
    pool_address: H160,
    evm: &mut EVM<ForkDB>
) -> Result<(H160, H160), anyhow::Error> {
    let token_0_call = BaseContract::from(
        parse_abi(&["function token0() external view returns (address)"]).unwrap()
    );
    let token_1_call = BaseContract::from(
        parse_abi(&["function token1() external view returns (address)"]).unwrap()
    );

    // get the token0 address

    evm.env.tx.caller = crate::utils::helpers::get_my_address().0.into();
    evm.env.tx.transact_to = TransactTo::Call(pool_address.0.into());
    evm.env.tx.data = token_0_call.encode("token0", ()).unwrap().0;
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 5000000;

    let result = match evm.transact_ref() {
        Ok(result) => result.result,
        Err(e) => {
            return Err(anyhow!("Error when getting balance: {:?}", e));
        }
    };

    let output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Revert when getting token0: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Halted when getting token0: {:?}", reason));
        }
    };

    let token_0 = match token_0_call.decode_output("token0", &output) {
        Ok(token) => token,
        Err(e) => {
            return Err(anyhow!("Failed to decode token0: {:?}", e));
        }
    };

    // now get the token1

    evm.env.tx.caller = crate::utils::helpers::get_my_address().0.into();
    evm.env.tx.transact_to = TransactTo::Call(pool_address.0.into());
    evm.env.tx.data = token_1_call.encode("token1", ()).unwrap().0;
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 5000000;

    let result = match evm.transact_ref() {
        Ok(result) => result.result,
        Err(e) => {
            return Err(anyhow!("Error when getting balance: {:?}", e));
        }
    };

    let output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Revert when getting token1: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Halted when getting token1: {:?}", reason));
        }
    };

    let token_1 = match token_1_call.decode_output("token1", &output) {
        Ok(token) => token,
        Err(e) => {
            return Err(anyhow!("Failed to decode token1: {:?}", e));
        }
    };

    Ok((token_0, token_1))
}

// Get token balance
//
// Arguments:
// * `token`: erc20 token to query
// * `owner`: address to find balance of
// * `next_block`: block to query balance at
// * `evm`: evm instance to run query on
//
// Returns:
// `Ok(balance: U256)` if successful, Err(SimulationError) otherwise
pub fn get_balance_of_evm(
    token: Address,
    owner: Address,
    next_block: &BlockInfo,
    evm: &mut EVM<ForkDB>
) -> Result<U256, anyhow::Error> {
    let erc20 = BaseContract::from(
        parse_abi(&["function balanceOf(address) external returns (uint)"]).unwrap()
    );

    evm.env.tx.transact_to = TransactTo::Call(token.0.into());
    evm.env.tx.data = erc20.encode("balanceOf", owner).unwrap().0;
    evm.env.tx.caller = crate::utils::helpers::get_my_address().0.into();
    evm.env.tx.gas_price = next_block.base_fee.into();
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.value = rU256::ZERO;

    let result = match evm.transact_ref() {
        Ok(result) => result.result,
        Err(e) => {
            return Err(anyhow!("Error when getting balance: {:?}", e));
        }
    };

    let output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Err when getting balance: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Halted when getting balance: {:?}", reason));
        }
    };

    match erc20.decode_output("balanceOf", &output) {
        Ok(tokens) => {
            return Ok(tokens);
        }
        Err(e) => {
            return Err(anyhow!("Failed to decode balanceOf: {:?}", e));
        }
    }
}

fn commit_pending_tx(evm: &mut EVM<ForkDB>, tx: &Transaction) -> Result<(), anyhow::Error> {
    // disable checks
    evm.env.cfg.disable_base_fee = true;
    evm.env.cfg.disable_block_gas_limit = true;
    evm.env.cfg.disable_balance_check = true;

    evm.env.tx.caller = rAddress::from_slice(&tx.from.0);
    evm.env.tx.transact_to = TransactTo::Call(rAddress::from_slice(&tx.to.unwrap_or_default().0));
    evm.env.tx.data = tx.input.0.clone();
    evm.env.tx.value = tx.value.into();
    evm.env.tx.gas_limit = 5000000;

    let res = match evm.transact_commit() {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Failed to commit pending tx: {:?}", e));
        }
    };

    let _output: Bytes = match res {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Pending tx reverted: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Pending tx halted: {:?}", reason));
        }
    };

    Ok(())
}

fn commit_tx(
    evm: &mut EVM<ForkDB>,
    call_data: Vec<u8>,
    next_block: &BlockInfo
) -> Result<(), anyhow::Error> {
    // setup evm for swap
    evm.env.tx.caller = get_my_address().into();
    evm.env.tx.transact_to = TransactTo::Call(get_snipe_contract_address().0.into());
    evm.env.tx.data = call_data.into();
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    let result = match evm.transact_commit() {
        Ok(res) => res,
        Err(e) => {
            return Err(anyhow!("Error Commiting tx: {:?}", e));
        }
    };

    let _output: Bytes = match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => o.into(),
                Output::Create(o, _) => o.into(),
            }
        ExecutionResult::Revert { output, .. } => {
            return Err(anyhow!("Sell Tx Reverted: {:?}", output));
        }
        ExecutionResult::Halt { reason, .. } => {
            return Err(anyhow!("Sell Tx Halted: {:?}", reason));
        }
    };

    Ok(())
}

fn commit_tx_with_inspector(
    evm: &mut EVM<ForkDB>,
    call_data: Vec<u8>,
    next_block: &BlockInfo,
    token: &Address
) -> Result<bool, anyhow::Error> {
    evm.env.tx.caller = get_my_address().into();
    evm.env.tx.transact_to = TransactTo::Call(get_snipe_contract_address().0.into());
    evm.env.tx.data = call_data.into();
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    let mut salmonella_inspector = SalmonellaInspectoooor::new();

    // simulate tx and write to db
    let result = match evm.inspect_commit(&mut salmonella_inspector) {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Failed to commit Tax Check: {:?}", e));
        }
    };

    // define a bool if the tx is reverted
    let is_tx_reverted = match result {
        ExecutionResult::Success { .. } => false,
        ExecutionResult::Revert { .. } => true,
        ExecutionResult::Halt { .. } => true,
    };

    // match the inspector to see if token is safu

    match salmonella_inspector.is_safu() {
        IsSafu::Safu => {}
        IsSafu::NotSafu(not_safu_opcodes) => {
            return Err(
                anyhow!(
                    "Token {:?} is not safu, found the following opcodes: {:?}",
                    token,
                    not_safu_opcodes
                )
            );
        }
    }

    Ok(is_tx_reverted)
}

// commit tx with access list inspector
// returns access list and gas used
fn commit_tx_with_access_list(
    evm: &mut EVM<ForkDB>,
    call_data: Vec<u8>,
    next_block: &BlockInfo
) -> Result<(AccessList, u64), anyhow::Error> {
    // setup evm for swap
    evm.env.tx.caller = get_my_address().into();
    evm.env.tx.transact_to = TransactTo::Call(get_snipe_contract_address().0.into());
    evm.env.tx.data = call_data.clone().into();
    evm.env.tx.value = rU256::ZERO;
    evm.env.tx.gas_limit = 1000000;
    evm.env.tx.gas_price = next_block.base_fee.into();

    // get access list
    let mut access_list_inspector = AccessListInspector::new(
        get_my_address().into(),
        get_snipe_contract_address().0.into()
    );

    // sim tx without writing to db
    evm.inspect_ref(&mut access_list_inspector)
        .map_err(|e| anyhow!("Error when getting Access List: {:?}", e))
        .unwrap();

    // get access list
    let access_list = access_list_inspector.into_access_list();

    evm.env.tx.access_list = access_list.clone();

    // run once again and commit changes to db
    let tx_result = match evm.transact_commit() {
        Ok(result) => result,
        Err(e) => {
            return Err(anyhow!("Error when commiting Final Simulation: {:?}", e).into());
        }
    };

    let gas_used = tx_result.gas_used();

    Ok((convert_access_list(access_list), gas_used))
}

pub async fn insert_pool_storage(
    client: Arc<Provider<Ws>>,
    pool: Pool,
    fork_block: Option<BlockId>
) -> Result<CacheDB<EmptyDB>, ProviderError> {
    let mut cache_db = CacheDB::new(EmptyDB::default());

    let slot_8 = rU256::from(8);

    // fetch the acc info once for each pool
    let pool_acc_info = get_acc_info(client.clone(), pool.address, fork_block).await?;

    cache_db.insert_account_info(pool.address.into(), pool_acc_info);

    let pool_value = get_storage(client.clone(), pool.address, slot_8, fork_block).await?;

    cache_db.insert_account_storage(pool.address.into(), slot_8, pool_value).unwrap();

    Ok(cache_db)
}

pub async fn get_acc_info(
    client: Arc<Provider<Ws>>,
    address: Address,
    fork_block: Option<BlockId>
) -> Result<AccountInfo, ProviderError> {
    let nonce = client.get_transaction_count(address, fork_block).await?;
    let balance = client.get_balance(address, fork_block).await?;
    let code = client.get_code(address, fork_block).await?;

    let acc_info = AccountInfo::new(balance.into(), nonce.as_u64(), Bytecode::new_raw(code.0));

    Ok(acc_info)
}

pub async fn get_storage(
    client: Arc<Provider<Ws>>,
    address: Address,
    slot: rU256,
    fork_block: Option<BlockId>
) -> Result<rU256, ProviderError> {
    let slot = H256::from(slot.to_be_bytes());
    let storage = client.get_storage_at(address, slot, fork_block).await.unwrap();
    Ok(rU256::from_be_bytes(storage.to_fixed_bytes()))
}
