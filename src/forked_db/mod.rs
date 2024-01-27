pub mod database_error;

pub mod global_backend;
pub use global_backend::*;

pub mod fork_db;
pub mod fork_factory;

use revm::primitives::{ExecutionResult, Output, Bytes};
use anyhow::anyhow;



// matches execution result, returns the output
pub fn match_output(result: ExecutionResult) -> Result<Bytes, anyhow::Error> {
    match result {
        ExecutionResult::Success { output, .. } =>
            match output {
                Output::Call(o) => Ok(o.into()),
                Output::Create(o, _) => Ok(o.into()),
            }
        ExecutionResult::Revert { output, gas_used } => {
            return Err(anyhow!("Call Reverted: {:?} Gas Used {}", bytes_to_string(output), gas_used));
        }
        ExecutionResult::Halt { reason,.. } => {
            return Err(anyhow!("Halt Reason: {:?}", reason));
        }
    }
}

/// matches execution result, returns the is_reverted
pub fn match_output_reverted(result: ExecutionResult) -> bool {
    let bool = match result {
         ExecutionResult::Success { .. } => false,
         ExecutionResult::Revert { output, .. } => {
              log::error!("Call Reverted: {:?}", bytes_to_string(output));
             true
         }
         ExecutionResult::Halt { .. } => true,
     };
     bool
 }


 pub fn bytes_to_string(bytes: revm::primitives::Bytes) -> String {
    if bytes.len() < 4 {
        return "EVM Returned 0x (Empty Bytes)".to_string();
    }
    let error_data = &bytes[4..];

    match String::from_utf8(error_data.to_vec()) {
        Ok(s) => s.trim_matches(char::from(0)).to_string(),
        Err(_) => "EVM Returned 0x (Empty Bytes)".to_string(),
    }
}