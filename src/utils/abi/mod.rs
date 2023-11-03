
use ethers::
    prelude::*;


abigen!(
    UniswapV2Pair,
    "src/utils/abi/IUniswapV2Pair.abi",
    event_derives(serde::Deserialize, serde::Serialize)
);

abigen!(
    UniswapV2Router,
    "src/utils/abi/IUniswapV2Router.abi",
    event_derives(serde::Deserialize, serde::Serialize)
);