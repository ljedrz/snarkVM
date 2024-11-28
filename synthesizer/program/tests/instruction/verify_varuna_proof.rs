// Copyright 2024 Aleo Network Foundation
// This file is part of the snarkVM library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:

// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::helpers::sample::sample_finalize_registers;

use algorithms::SNARK;
use console::{
    network::MainnetV0,
    prelude::*,
    program::{Identifier, Literal, Plaintext, Register, Value}, types::BytesType,
};
use snarkvm_synthesizer_program::{Operand, Program, RegistersLoad, VerifyVarunaProof};
use synthesizer_process::{Process, Stack};
use snark::{
    test_helpers::{sample_assignment, sample_keys},
    Proof, 
    Varuna
};

use std::borrow::Borrow;

type CurrentNetwork = MainnetV0;

/// Samples the stack. Note: Do not replicate this for real program use, it is insecure.
#[allow(clippy::type_complexity)]
fn sample_stack(
) -> Result<(Stack<CurrentNetwork>, Vec<Operand<CurrentNetwork>>)> {
    // Initialize the program.
    let program = Program::from_str(&format!(
        "program testing.aleo;
            function test_verify_varuna:
                input r0 as bytes.public;
                input r1 as bytes.public;
                input r2 as bytes.public;
                varuna.verify r0 r1 r2 into r3;
                async test_verify_varuna r0 r1 r2 into r4;
                output r4 as testing.aleo/test_verify_varuna.future;

            finalize test_verify_varuna:
                input r0 as bytes.public;
                input r1 as bytes.public;
                input r2 as bytes.public;
                varuna.verify r0 r1 r2 into r3;
        "
    ))?;

    // Initialize the registers.
    let r0 = Register::Locator(0);
    let r1 = Register::Locator(1);
    let r2 = Register::Locator(2);

    // Initialize the operands.
    let operand_proof = Operand::Register(r0);
    let operand_keys = Operand::Register(r1);
    let operand_key_inputs = Operand::Register(r2);
    let operands = vec![operand_proof, operand_keys, operand_key_inputs];

    // Initialize the stack.
    let stack = Stack::new(&Process::load_testing_only()?, &program)?;

    Ok((stack, operands))
}

fn check_varuna_verify<B: Borrow<<Varuna<CurrentNetwork> as SNARK>::VerifierInput>>(
    proof: &Proof<CurrentNetwork>,
    keys: &[<Varuna<CurrentNetwork> as SNARK>::VerifyingKey],
    key_inputs: &[B],
) -> Result<bool> {
    // Serialize the params.
    let proof_bytes = proof.to_bytes_le()?;
    let keys_bytes = keys.to_bytes_le()?;
    let mut key_inputs_bytes = Vec::new();
    (key_inputs.len() as u8).write_le(&mut key_inputs_bytes)?;
    for input in key_inputs {
        let input = input.borrow();
        (input.len() as u8).write_le(&mut key_inputs_bytes)?;
        input.write_le(&mut key_inputs_bytes)?;
    }

    // Initialize the stack.
    let (stack, operands) = sample_stack()?;

    // Initialize the function name.
    let function_name = Identifier::from_str("test_verify_varuna")?;

    // Initialize the instruction.
    let destination = Register::Locator(3);
    let instruction = VerifyVarunaProof::new(operands, destination.clone())?;

    // Initialize the literals.
    let proof_literal = Literal::Bytes(BytesType::new(proof_bytes));
    let keys_literal = Literal::Bytes(BytesType::new(keys_bytes));
    let key_inputs_literal = Literal::Bytes(BytesType::new(key_inputs_bytes));

    // Construct the finalize registers.
    let mut finalize_registers = sample_finalize_registers(&stack, &function_name, &[&proof_literal, &keys_literal, &key_inputs_literal]).unwrap();

    // Attempt to finalize.
    instruction.finalize(&stack, &mut finalize_registers).unwrap();

    // Get the result from the destination register.
    let Value::Plaintext(Plaintext::Literal(Literal::Boolean(result), _)) = finalize_registers.load(&stack, &Operand::Register(destination))? else {
        unreachable!()
    };

    Ok(*result)
}

#[test]
fn test_single_instance_varuna_verify() {
    // Sample an RNG.
    let rng = &mut TestRng::default();

    // Sample a set of keys, inputs, and a proof.
    let (pk, vk) = sample_keys();
    let assignment = sample_assignment();
    let inputs = vec![assignment.public_inputs().iter().map(|(_, field)| *field).collect_vec()];
    let proof = pk.prove("test_verify_varuna", &assignment, rng).unwrap();

    // Check that varuna.verify succeeds.
    assert!(check_varuna_verify(&proof, &[(&*vk).clone()], &inputs).unwrap());
}
