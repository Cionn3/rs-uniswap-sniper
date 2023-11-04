// SPDX-License-Identifier: MIT
pragma solidity >=0.7.0 <0.9.0;




interface IUniswapV2Callee {
    function uniswapV2Call(
        address sender,
         uint amount0,
          uint amount1,
           bytes calldata data
           ) external;
}


interface IUniswapV2Pair {
    function swap(
        uint amount0Out,
        uint amount1Out,
        address to,
        bytes calldata data
    ) external;

    function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);

}
