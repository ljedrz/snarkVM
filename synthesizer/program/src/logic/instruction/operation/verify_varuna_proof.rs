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

use std::{
    collections::BTreeMap,
    io::{Cursor, Seek},
};

use crate::{
    Opcode,
    Operand,
    traits::{RegistersLoad, RegistersLoadCircuit, RegistersStore, RegistersStoreCircuit, StackMatches, StackProgram},
};
use algorithms::SNARK;
use console::{
    network::prelude::*,
    program::{Literal, LiteralType, PlaintextType, Register, RegisterType},
    types::Boolean,
};
use snark::{Proof, Varuna};

/// Computes whether `signature` is valid for the given `address` and `message`.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct VerifyVarunaProof<N: Network> {
    /// The operands.
    operands: Vec<Operand<N>>,
    // The destination register.
    destination: Register<N>,
}

impl<N: Network> VerifyVarunaProof<N> {
    /// Initializes a new `sign.verify` instruction.
    #[inline]
    pub fn new(operands: Vec<Operand<N>>, destination: Register<N>) -> Result<Self> {
        // Sanity check the number of operands.
        ensure!(operands.len() == 3, "Instruction '{}' must have 3 operands", Self::opcode());
        // Return the instruction.
        Ok(Self { operands, destination })
    }

    /// Returns the opcode.
    #[inline]
    pub const fn opcode() -> Opcode {
        Opcode::VerifyVarunaProof
    }

    /// Returns the operands in the operation.
    #[inline]
    pub fn operands(&self) -> &[Operand<N>] {
        // Sanity check that there is exactly a single operand.
        debug_assert!(self.operands.len() == 3, "Instruction '{}' must have 3 operands", Self::opcode());
        // Return the operands.
        &self.operands
    }

    /// Returns the destination register.
    #[inline]
    pub fn destinations(&self) -> Vec<Register<N>> {
        vec![self.destination.clone()]
    }
}

impl<N: Network> VerifyVarunaProof<N> {
    /// Evaluates the instruction.
    #[inline]
    pub fn evaluate(
        &self,
        _stack: &(impl StackMatches<N> + StackProgram<N>),
        _registers: &mut (impl RegistersLoad<N> + RegistersStore<N>),
    ) -> Result<()> {
        bail!("Forbidden operation: varuna proofs can only be verified in a finalize context");
    }

    /// Executes the instruction.
    #[inline]
    pub fn execute<A: circuit::Aleo<Network = N>>(
        &self,
        _stack: &(impl StackMatches<N> + StackProgram<N>),
        _registers: &mut (impl RegistersLoadCircuit<N, A> + RegistersStoreCircuit<N, A>),
    ) -> Result<()> {
        bail!("Forbidden operation: varuna proofs can only be verified in a finalize context");
    }

    /// Finalizes the instruction.
    #[inline]
    pub fn finalize(
        &self,
        stack: &(impl StackMatches<N> + StackProgram<N>),
        registers: &mut (impl RegistersLoad<N> + RegistersStore<N>),
    ) -> Result<()> {
        // Ensure the number of operands is correct.
        if self.operands.len() != 3 {
            bail!("Instruction '{}' expects 3 operands, found {} operand(s)", Self::opcode(), self.operands.len())
        }

        // Retrieve the Varuna proof, its keys, and their inputs.
        let varuna_proof = match registers.load_literal(stack, &self.operands[0])? {
            Literal::Bytes(b) => b,
            _ => bail!("Expected the 1st operand to be bytes."),
        };
        let varuna_keys = match registers.load_literal(stack, &self.operands[1])? {
            Literal::Bytes(b) => b,
            _ => bail!("Expected the 2nd operand to be bytes."),
        };
        let varuna_key_inputs = match registers.load_literal(stack, &self.operands[2])? {
            Literal::Bytes(b) => b,
            _ => bail!("Expected the 3rd operand to be bytes."),
        };

        // Deserialize the proof.
        let mut reader = Cursor::new(&*varuna_proof);
        let proof = Proof::<N>::read_le(&mut reader)?;
        // Deserialize the keys.
        let mut keys_to_inputs = Vec::new();
        let mut reader = Cursor::new(&*varuna_keys);
        let mut keys = Vec::new();
        let mut number_of_keys = 0usize;
        while reader.stream_position()? < varuna_keys.len() as u64 {
            let key = <<Varuna<N> as SNARK>::VerifyingKey>::read_le(&mut reader)?;
            number_of_keys += 1;
            keys.push(key);
        }

        // Deserialize the key inputs.
        let mut reader = Cursor::new(&*varuna_key_inputs);
        let mut inputs = Vec::new();
        for _ in 0..number_of_keys {
            let number_of_key_inputs = u8::read_le(&mut reader)?;
            let mut key_inputs = Vec::new();
            for _ in 0..number_of_key_inputs {
                let number_of_fields = u8::read_le(&mut reader)?;
                let mut fields = Vec::new();
                for _ in 0..number_of_fields {
                    fields.push(N::Field::read_le(&mut reader)?);
                }
                key_inputs.push(fields);
            }
            inputs.push(key_inputs);
        }
        for (key, key_inputs) in keys.into_iter().zip(inputs) {
            keys_to_inputs.push((key, key_inputs));
        }

        // Procure the FS parameters and the universal verifier.
        let fs_parameters = N::varuna_fs_parameters();
        let universal_verifier = N::varuna_universal_verifier();

        // Verify the proof.
        let keys_to_inputs_map =
            keys_to_inputs.iter().map(|(key, inputs)| (key, inputs.as_slice())).collect::<BTreeMap<_, _>>();
        let output = Literal::Boolean(Boolean::new(
            Varuna::<N>::verify_batch(universal_verifier, fs_parameters, &keys_to_inputs_map, &proof).is_ok(),
        ));

        // Store the output.
        registers.store_literal(stack, &self.destination, output)
    }

    /// Returns the output type from the given program and input types.
    #[inline]
    pub fn output_types(
        &self,
        _stack: &impl StackProgram<N>,
        input_types: &[RegisterType<N>],
    ) -> Result<Vec<RegisterType<N>>> {
        // Ensure the number of input types is correct.
        if input_types.len() != 3 {
            bail!("Instruction '{}' expects 3 inputs, found {} input(s)", Self::opcode(), input_types.len())
        }

        // Ensure the operands are bytes.
        for i in 0..3 {
            if input_types[i] != RegisterType::Plaintext(PlaintextType::Literal(LiteralType::Bytes)) {
                bail!(
                    "Instruction '{}' expects input {} to be a 'bytes'. Found input of type '{}'",
                    Self::opcode(),
                    i + 1,
                    input_types[0]
                )
            }
        }

        Ok(vec![RegisterType::Plaintext(PlaintextType::Literal(LiteralType::Boolean))])
    }
}

impl<N: Network> Parser for VerifyVarunaProof<N> {
    /// Parses a string into an operation.
    #[inline]
    fn parse(string: &str) -> ParserResult<Self> {
        // Parse the opcode from the string.
        let (string, _) = tag(*Self::opcode())(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the first operand from the string.
        let (string, operand1) = Operand::parse(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the second operand from the string.
        let (string, operand2) = Operand::parse(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the third operand from the string.
        let (string, operand3) = Operand::parse(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the "into" from the string.
        let (string, _) = tag("into")(string)?;
        // Parse the whitespace from the string.
        let (string, _) = Sanitizer::parse_whitespaces(string)?;
        // Parse the destination register from the string.
        let (string, destination) = Register::parse(string)?;

        Ok((string, Self { operands: vec![operand1, operand2, operand3], destination }))
    }
}

impl<N: Network> FromStr for VerifyVarunaProof<N> {
    type Err = Error;

    /// Parses a string into an operation.
    #[inline]
    fn from_str(string: &str) -> Result<Self> {
        match Self::parse(string) {
            Ok((remainder, object)) => {
                // Ensure the remainder is empty.
                ensure!(remainder.is_empty(), "Failed to parse string. Found invalid character in: \"{remainder}\"");
                // Return the object.
                Ok(object)
            }
            Err(error) => bail!("Failed to parse string. {error}"),
        }
    }
}

impl<N: Network> Debug for VerifyVarunaProof<N> {
    /// Prints the operation as a string.
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl<N: Network> Display for VerifyVarunaProof<N> {
    /// Prints the operation to a string.
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        // Ensure the number of operands is 1.
        if self.operands.len() != 3 {
            return Err(fmt::Error);
        }
        // Print the operation.
        write!(f, "{} ", Self::opcode())?;
        self.operands().iter().try_for_each(|operand| write!(f, "{operand} "))?;
        write!(f, "into {}", self.destination)
    }
}

impl<N: Network> FromBytes for VerifyVarunaProof<N> {
    /// Reads the operation from a buffer.
    fn read_le<R: Read>(mut reader: R) -> IoResult<Self> {
        // Initialize the vector for the operands.
        let mut operands = Vec::with_capacity(3);
        // Read the operands.
        operands.push(Operand::read_le(&mut reader)?);
        operands.push(Operand::read_le(&mut reader)?);
        operands.push(Operand::read_le(&mut reader)?);
        // Read the destination register.
        let destination = Register::read_le(&mut reader)?;

        // Return the operation.
        Ok(Self { operands, destination })
    }
}

impl<N: Network> ToBytes for VerifyVarunaProof<N> {
    /// Writes the operation to a buffer.
    fn write_le<W: Write>(&self, mut writer: W) -> IoResult<()> {
        // Ensure the number of operands is 1.
        if self.operands.len() != 3 {
            return Err(error(format!("The number of operands must be 3, found {}", self.operands.len())));
        }
        // Write the operands.
        self.operands[0].write_le(&mut writer)?;
        self.operands[1].write_le(&mut writer)?;
        self.operands[2].write_le(&mut writer)?;
        // Write the destination register.
        self.destination.write_le(&mut writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use console::network::MainnetV0;

    type CurrentNetwork = MainnetV0;

    #[test]
    fn test_parse() {
        let (string, is) = VerifyVarunaProof::<CurrentNetwork>::parse("varuna.verify r0 r1 r2 into r3").unwrap();
        assert!(string.is_empty(), "Parser did not consume all of the string: '{string}'");
        assert_eq!(is.operands.len(), 3, "The number of operands is incorrect");
        assert_eq!(is.operands[0], Operand::Register(Register::Locator(0)), "The first operand is incorrect");
        assert_eq!(is.operands[1], Operand::Register(Register::Locator(1)), "The first operand is incorrect");
        assert_eq!(is.operands[2], Operand::Register(Register::Locator(2)), "The first operand is incorrect");
        assert_eq!(is.destination, Register::Locator(3), "The destination register is incorrect");
    }
}
