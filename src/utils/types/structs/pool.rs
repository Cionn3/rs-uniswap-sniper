use ethers::prelude::*;



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