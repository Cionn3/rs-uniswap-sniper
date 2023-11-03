use crate::forked_db::fork_db::ForkDB;
use crate::oracles::block_oracle::BlockInfo;
use std::str::FromStr;

use revm::{
    primitives::{Address as rAddress, U256 as rU256},
    EVM,
};


// Setup evm blockstate
//
// Arguments:
// * `&mut evm`: mutable refernece to `EVM<ForkDB>` instance which we want to modify
// * `&next_block`: reference to `BlockInfo` of next block to set values against
//
// Returns: This function returns nothing
pub fn setup_block_state(evm: &mut EVM<ForkDB>, next_block: &BlockInfo) {
    evm.env.block.number = rU256::from(next_block.number.as_u64());
    evm.env.block.timestamp = next_block.timestamp.into();
    evm.env.block.basefee = next_block.base_fee.into();
    // use something other than default
    evm.env.block.coinbase =
        rAddress::from_str("0xDecafC0FFEe15BAD000000000000000000000000").unwrap();
}
