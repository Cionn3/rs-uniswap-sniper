use revm::interpreter::{opcode, Interpreter};
use revm::interpreter::{CallInputs, CreateInputs, Gas, InstructionResult};
use revm::primitives::{Bytes, B160};
use revm::{Database, EVMData, Inspector};

#[derive(Debug)]
pub enum IsSafu {
    Safu,
    NotSafu(Vec<OpCode>),
}

#[derive(Debug, Clone)]
pub struct OpCode {
    name: String,
    code: u8,
}

impl OpCode {
    // creat a new opcode instance from numeric opcode
    //
    // Arguments:
    // * `code`: numberic opcode
    //
    // Returns:
    // `OpCode`: new opcode instance
    fn new_from_code(code: u8) -> Self {
        let name = match opcode::OPCODE_JUMPMAP[code as usize] {
            Some(name) => name.to_string(),
            None => "UNKNOWN".to_string(),
        };

        OpCode { code, name }
    }
}

pub struct SalmonellaInspectoooor {
    suspicious_opcodes: Vec<OpCode>,
    gas_opcode_counter: u64,
    call_opcode_counter: u64,
}

impl SalmonellaInspectoooor {
    // create new salmonella inspector
    pub fn new() -> Self {
        Self {
            suspicious_opcodes: Vec::new(),
            gas_opcode_counter: 0,
            call_opcode_counter: 0,
        }
    }

    // checks if opportunity is safu
    //
    // Arguments:
    // `self`: consumes self during calculation
    //
    // Returns:
    // IsSandoSafu: enum that is either Safu or NotSafu
    pub fn is_safu(self) -> IsSafu {
        // if more gas opcodes used then call then we know that the contract is checking gas_used
        let mut suspicious_opcodes = self.suspicious_opcodes.clone();
        if self.gas_opcode_counter < self.call_opcode_counter {
            let gas_opcode = OpCode::new_from_code(opcode::GAS);
            suspicious_opcodes.insert(0, gas_opcode);
        }

        match self.suspicious_opcodes.len() == 0 {
            true => IsSafu::Safu,
            false => IsSafu::NotSafu(suspicious_opcodes),
        }
    }
}

impl<DB: Database> Inspector<DB> for SalmonellaInspectoooor {
    fn initialize_interp(
        &mut self,
        _interp: &mut Interpreter,
        _data: &mut EVMData<'_, DB>,
        _is_static: bool,
    ) -> InstructionResult {
        InstructionResult::Continue
    }

    // get opcode by calling `interp.contract.opcode(interp.program_counter())`.
    // all other information can be obtained from interp.
    fn step(
        &mut self,
        interp: &mut Interpreter,
        _data: &mut EVMData<'_, DB>,
        _is_static: bool,
    ) -> InstructionResult {
        let executed_opcode = interp.current_opcode();

        let mut add_suspicious = |opcode: OpCode| self.suspicious_opcodes.push(opcode);
        let mut increment_call_counter = || self.call_opcode_counter += 1;

        let executed_opcode = OpCode::new_from_code(executed_opcode);

        match executed_opcode.code {
            // these opcodes can be used to divert execution flow when ran locally vs on mainnet
            // extra safe version, can easily ignore half of these checks if ur up for it
            opcode::BALANCE => add_suspicious(executed_opcode.clone()),
            opcode::GASPRICE => add_suspicious(executed_opcode.clone()),
            opcode::EXTCODEHASH => add_suspicious(executed_opcode.clone()),
            opcode::BLOCKHASH => add_suspicious(executed_opcode.clone()),
            opcode::COINBASE => add_suspicious(executed_opcode.clone()),
            opcode::DIFFICULTY => add_suspicious(executed_opcode.clone()),
            opcode::GASLIMIT => add_suspicious(executed_opcode.clone()),
            opcode::SELFBALANCE => add_suspicious(executed_opcode.clone()),
            opcode::BASEFEE => add_suspicious(executed_opcode.clone()),
            opcode::CREATE => add_suspicious(executed_opcode.clone()),
            opcode::CREATE2 => add_suspicious(executed_opcode.clone()),
            opcode::SELFDESTRUCT => add_suspicious(executed_opcode.clone()),
            // add one to call counter
            opcode::CALL => increment_call_counter(),
            opcode::DELEGATECALL => increment_call_counter(),
            opcode::STATICCALL => increment_call_counter(),
            // add one to gas opcode counter
            opcode::GAS => self.gas_opcode_counter += 1,
            _ => { /* this opcode is safu */ }
        }

        // changed the search to contains
        if executed_opcode.name.to_lowercase().contains("unknown") {
            add_suspicious(executed_opcode);
        }
        

        InstructionResult::Continue
    }

    fn step_end(
        &mut self,
        _interp: &mut Interpreter,
        _data: &mut EVMData<'_, DB>,
        _is_static: bool,
        _eval: InstructionResult,
    ) -> InstructionResult {
        InstructionResult::Continue
    }

    fn call(
        &mut self,
        _data: &mut EVMData<'_, DB>,
        _inputs: &mut CallInputs,
        _is_static: bool,
    ) -> (InstructionResult, Gas, Bytes) {
        (InstructionResult::Continue, Gas::new(0), Bytes::new())
    }

    fn call_end(
        &mut self,
        _data: &mut EVMData<'_, DB>,
        _inputs: &CallInputs,
        remaining_gas: Gas,
        ret: InstructionResult,
        out: Bytes,
        _is_static: bool,
    ) -> (InstructionResult, Gas, Bytes) {
        (ret, remaining_gas, out)
    }

    fn create_end(
        &mut self,
        _data: &mut EVMData<'_, DB>,
        _inputs: &CreateInputs,
        ret: InstructionResult,
        address: Option<B160>,
        remaining_gas: Gas,
        out: Bytes,
    ) -> (InstructionResult, Option<B160>, Gas, Bytes) {
        (ret, address, remaining_gas, out)
    }
}
