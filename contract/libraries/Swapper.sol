// SPDX-License-Identifier: MIT
pragma solidity >=0.7.0 <0.9.0;


// Uniswap v2 interface
import {IUniswapV2Pair} from '../interfaces/Swaps.sol';  

// ERC20 interface
import '../interfaces/IERC20.sol'; 
import {SafeERC20} from '../interfaces//SafeERC20/SafeERC20.sol';



library Swapper {
using SafeERC20 for IERC20;



// credits: https://github.com/mouseless-eth/rusty-sando/blob/master/contract/src/LilRouter.sol
// swap input token for output token on uniswap v2 and forks, returns real balance of output token
function _swap_on_V2(
 address input_token,
 address output_token,
 uint256 amount_in,
  address pool
  ) internal returns(uint256) {

   
        // Optimistically send amountIn of inputToken to targetPair
        IERC20 token = IERC20(input_token);
        token.safeTransfer(pool, amount_in);

        // Prepare variables for calculating expected amount out
        uint reserveIn;
        uint reserveOut;


        { // Avoid stack too deep error
        (uint reserve0, uint reserve1,) = IUniswapV2Pair(pool).getReserves();

        // sort reserves
        if (input_token < output_token) {
            // Token0 is equal to inputToken
            // Token1 is equal to outputToken
            reserveIn = reserve0;
            reserveOut = reserve1;
        } else {
            // Token0 is equal to outputToken
            // Token1 is equal to inputToken
            reserveIn = reserve1;
            reserveOut = reserve0;
        }
        }



        // Find the actual amountIn sent to pair (accounts for tax if any) and amountOut
       uint actualAmountIn = IERC20(input_token).balanceOf(address(pool)) - reserveIn;
       uint256 amountOut = _getAmountOut(actualAmountIn, reserveIn, reserveOut);

        // Prepare swap variables and call pair.swap()
        (uint amount0Out, uint amount1Out) = input_token < output_token ? (uint(0), amountOut) : (amountOut, uint(0));
        IUniswapV2Pair(pool).swap(amount0Out, amount1Out, address(this), new bytes(0));

     return IERC20(output_token).balanceOf(address(this));

}




function _getAmountOut(uint amountIn, uint reserveIn, uint reserveOut) internal pure returns (uint amountOut) {
    require(amountIn > 0, 'UniswapV2Library: INSUFFICIENT_INPUT_AMOUNT');
    require(reserveIn > 0 && reserveOut > 0, 'UniswapV2Library: INSUFFICIENT_LIQUIDITY');
    uint amountInWithFee = amountIn * 997;
    uint numerator = amountInWithFee * reserveOut;
    uint denominator = reserveIn * 1000 + amountInWithFee;
    amountOut = numerator / denominator;
}
}