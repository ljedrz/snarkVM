// Copyright (C) 2019-2023 Aleo Systems Inc.
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

use snarkvm::prelude::{MainnetV0 as CurrentNetwork, Network, PrivateKey, Process, Program, TestRng, Value};

use afl;

type CurrentAleo = snarkvm::circuit::network::AleoV0;

struct ProgramWithInputs<N: Network> {
    program: Program<N>,
    inputs: Vec<Value<N>>,
}

impl<'a, N: Network + arbitrary::Arbitrary<'a>> arbitrary::Arbitrary<'a> for ProgramWithInputs<N>
{
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let program = <Program<N> as arbitrary::Arbitrary>::arbitrary(u)?;
        let expected_input_count = program.functions()[0].inputs().len();

        let mut inputs = Vec::new();
        for _ in 0..expected_input_count {
            let input = <Value<N> as arbitrary::Arbitrary>::arbitrary(u)?;
            inputs.push(input);
        }

        Ok(Self { program, inputs })
    }
}

fn main() {
    let rng = &mut TestRng::fixed(7777777);
    let private_key = PrivateKey::<CurrentNetwork>::new(rng).unwrap();

    afl::fuzz_nohook!(|program_inputs: ProgramWithInputs<CurrentNetwork>| {
        let ProgramWithInputs { program, inputs } = program_inputs;

        let mut process = Process::load().unwrap();
        if process.add_program(&program).is_err() {
            return;
        }

        for function_name in program.functions().values().map(|foo| foo.name()) {
            let Ok(authorization) =
                process.authorize::<CurrentAleo, _>(&private_key, program.id(), function_name, inputs.clone().into_iter(), rng) else {
                    continue;
            };

            let _ = process.execute::<CurrentAleo, _>(authorization, rng);
        };
    });
}
