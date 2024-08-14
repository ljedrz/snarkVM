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

use std::str::FromStr;
use snarkvm::prelude::{MainnetV0 as CurrentNetwork, FromBytes, PrivateKey, Process, Program, TestRng, ToBytes, Value};

use afl;

type CurrentAleo = snarkvm::circuit::network::AleoV0;

fn main() {
    let rng = &mut TestRng::fixed(7777777);
    let private_key = PrivateKey::<CurrentNetwork>::new(rng).unwrap();

    afl::fuzz_nohook!(|program_inputs: (Program<CurrentNetwork>, Vec<Value<CurrentNetwork>>)| {
        let (program, inputs) = program_inputs;

        if program.functions().is_empty() {
            return;
        }
        match std::panic::catch_unwind(|| {
            let program_string = program.to_string();
            let Ok(program_from_string) = Program::from_str(&program_string) else {
                return false;
            };

            if program != program_from_string {
                return false;
            }
            
            true
        }) {
            Ok(true) => {},
            _ => return,
        };

        let Ok(program_bytes) = program.to_bytes_le() else {
            return;
        };
        let Ok(program_from_bytes) = Program::from_bytes_le(&program_bytes) else {
            return;
        };
        if program != program_from_bytes {
            return;
        };
        
        let Some(function_name) = program.functions().values().next().map(|foo| foo.name()) else {
            return;
        };

        let mut process = Process::load().unwrap();
        if process.add_program(&program).is_err() {
            return;
        }

        let Ok(authorization) =
            process.authorize::<CurrentAleo, _>(&private_key, program.id(), function_name, inputs.into_iter(), rng) else {
                return;
        };

        let _ = process.execute::<CurrentAleo, _>(authorization, rng);
    });
}
