# Uniswap Sniper

### Implementation of a sniping bot in Rust for Uniswap V2.

## Execution Flow

- Listen for pending transactions and extract the logs using EVM simulations.
- If a new pair is found, we send it to the sniper module where we run EVM simulations to determine if it's a honeypot or not.
- If it passes the checks, we buy the token.
- From there, we monitor the price from the sell oracle.
- We also monitor the token from the Anti-Honeypot and Anti-Rug oracles.

### Anti-Honeypot

We monitor the mempool for transactions that interact with the token contract address. We can see, for example, if we are going to be blacklisted, taxes increased to 100%, etc.

### Anti-Rug

We monitor the mempool for transactions that touch the token's pool. We can see if the token's liquidity is going to be removed, or if a big amount of tokens is going to be sold which will have the same result, etc.

#### However, keep in mind if one of these transactions is sent directly to builders, we will not be able to detect them.

## Some of its features

- Can almost detect all new pairs no matter the method they are created with.
- Send the transaction directly to builders.
- Keep track of the selling price on every block.
- Takes the initial amount in + total gas fees out as profit once the selling price met the criteria.
- Doesn't sell the token if the total gas cost is more than the WETH we are going to receive (see Anti-rug).
- Extremely fast and accurate simulations thanks to `revm`.

**Note:** I'm not recommending for anyone to try it on-chain as the chances of catching a token that will do a lot of Xs before it rugs are extremely low, plus the high gas fees of Ethereum are not helpful.

This repo is just for educational purposes only, showcasing some of the amazing capabilities of `revm` (what will happen after this state change, etc...).

## Keep in mind

- While the bot is technically working and doesn't crash there may still be some bugs that I haven't noticed.
- Every information the bot holds for the tokens we bought is kept in memory if you restart the bot all the information will be lost and you will have to manually withdraw the tokens from the contract and sell them, use with caution!

## Usage

If you want to try it:

1. Go to `contracts/src/sniper.sol` and fill in your addresses.
2. Deploy and fund your contract.
3. Go to `src/utils/constants.rs` and fill in your addresses and private keys.
4. Compile with: `RUSTFLAGS="-C target-cpu=native" cargo build --profile maxperf`
5. Navigate to the `target/maxperf`
6. And run it: `./rs-uniswap-sniper`


#### Please make sure you read and understand the codebase and adjust some values as you like. Could do some better organization of the code, any contributions are welcome!

## Example Outputs

### When we try to snipe a new token

```
[08:17:21][INFO] New Pool Found!🚀
[08:17:21][INFO] Pool Address: 0xe1f61921b4a4bce352aab57ca4c696180c0f169a
[08:17:21][INFO] Token Address: 0xc300c7145bac98cb6748b908f4d26e9cee130594
[08:17:21][ERROR] Buy or sell reverted
[08:17:21][ERROR] Sniper Failed: for token 0xc300c7145bac98cb6748b908f4d26e9cee130594 Err Snipe Failed, sent it to retry oracle
[08:17:25][ERROR] Retry Tax Check Failed: Swap Buy reverted: b""
```


### Successfully Sniped a Token

```
[08:53:56][INFO] New Pool Found!🚀
[08:53:56][INFO] Pool Address: 0x2b4d83a40ccdb6ff4af0846411732953053c3fbd
[08:53:56][INFO] Token Address: 0xdc19a59ba8308e4f55c1f24b11a63062a2733fbf
[08:53:56][INFO] Sniping with miner tip: BigDecimal("0.15")
[08:53:56][INFO] Token 0xdc19a59ba8308e4f55c1f24b11a63062a2733fbf Passed All Checks! 🚀
[08:53:56][INFO] Sending Tx...
[08:53:56][INFO] New Snipe Event Sent To Sell Oracle! 🚀
[08:53:57][INFO] Simulated Bundle Result: SimulatedBundle {hash: 0xdddc702790d4536d038091e6f5b2c7d6f9d499054d165963326df03e48c531dd, coinbase_diff: 285641200000000, coinbase_tip: 0, gas_price: 101822088, gas_used: 2805297, gas_fees: 285641200000000, simulation_block: 18404180, transactions: [SimulatedTransaction { hash: 0x1b2051f37f1ac424372fdf67118508b53ae124e53290426f8a0a38923dbb9c57, coinbase_diff: 270306700000000, coinbase_tip: 0, gas_price: 100000000, gas_used: 2703067, gas_fees: 270306700000000, from: 0x3752e7a1e18e2e297c7f139e1ea76b42eeecdfa3, to: Some(0x7a250d5630b4cf539739df2c5dacb4c659f2488d), value: Some(Bytes(0x0000000000000000000000000000000000000000204fce5e3e25026110000000000000000000000000000000000000000000000000000001a055690d9db800000000000000000000000000000000000000000000000073fc196e3c77728b3f61)), error: None, revert: None }, SimulatedTransaction { hash: 0x1deace75626eb1912f68f08099fc1b3bbc7ad91272d36872b2dc052454c5080a, coinbase_diff: 15334500000000, coinbase_tip: 0, gas_price: 150000000, gas_used: 102230, gas_fees: 15334500000000, from: 0xf6f9ea00f25cebfc6b51f0d7e0092076ad77f3eb, to: Some(0x773ea7f13c09af80ddce518aa97a0e8744a2fb78), value: Some(Bytes(0x)), error: None, revert: None }]}
[08:54:04][INFO] Is Bundle Included: true
```


### From Sell Oracle
```
[08:54:37][INFO] Token: 0xdc19a59ba8308e4f55c1f24b11a63062a2733fbf initial amount in: BigDecimal("0.025") ETH, current amount out: BigDecimal("0.024850348962065471") ETH
```

### Here we failed to catch the malicious pending tx because he probably sent it directly to builders
```
hash: 0x6b56458f61e959d5bf3912fe1fd27bea9c2f9ec0736ef94a35d4bc8fa433dea1
```

```
[08:55:49][ERROR] Failed to simulate sell for token: 0xdc19a59ba8308e4f55c1f24b11a63062a2733fbf Error: Sell Tx Reverted: b"\x08\xc3y\xa0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0 \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0&ERC20: transfer amount exceeds balance\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"
[08:55:54][ERROR] Failed to simulate Anti-Rug Before sell tx for Token: 0xdc19a59ba8308e4f55c1f24b11a63062a2733fbf Err Sell Tx Reverted: b"\x08\xc3y\xa0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0 \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0&ERC20: transfer amount exceeds balance\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"
```


## Acknowledgments

- [rusty-sando](https://github.com/mouseless-eth/rusty-sando)
- [reth](https://github.com/paradigmxyz/reth)
- [revm](https://github.com/bluealloy/revm)
- [revm-is-all-you-need](https://github.com/solidquant/revm-is-all-you-need)
