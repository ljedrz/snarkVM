// Copyright (C) 2019-2021 Aleo Systems Inc.
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

use crate::{
    algorithms::prf::*,
    traits::{
        algorithms::PRFGadget,
        utilities::{
            alloc::AllocGadget,
            boolean::{AllocatedBit, Boolean},
            eq::EqGadget,
            uint::unsigned_integer::{UInt, UInt8},
        },
    },
};
use snarkvm_algorithms::{prf::blake2s::Blake2s as B2SPRF, traits::PRF};
use snarkvm_curves::bls12_377::Fr;
use snarkvm_r1cs::{ConstraintSystem, TestConstraintSystem};

use blake2::VarBlake2s;
use digest::{Update, VariableOutput};
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

#[test]
fn test_blake2s_constraints() {
    let mut cs = TestConstraintSystem::<Fr>::new();
    let input_bits: Vec<_> = (0..512)
        .map(|i| {
            AllocatedBit::alloc(cs.ns(|| format!("input bit_gadget {}", i)), || Ok(true))
                .unwrap()
                .into()
        })
        .collect();
    blake2s_gadget(&mut cs, &input_bits).unwrap();
    assert!(cs.is_satisfied());
    assert_eq!(cs.num_constraints(), 21792);
}

#[test]
fn test_blake2s_prf() {
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);
    let mut cs = TestConstraintSystem::<Fr>::new();

    let mut seed = [0u8; 32];
    rng.fill(&mut seed);

    let mut input = [0u8; 32];
    rng.fill(&mut input);

    let seed_gadget = Blake2sGadget::new_seed(&mut cs.ns(|| "declare_seed"), &seed);
    let input_gadget = UInt8::alloc_vec(&mut cs.ns(|| "declare_input"), &input).unwrap();
    let out = B2SPRF::evaluate(&seed, &input).unwrap();
    let actual_out_gadget =
        <Blake2sGadget as PRFGadget<_, Fr>>::OutputGadget::alloc(&mut cs.ns(|| "declare_output"), || Ok(out)).unwrap();

    let output_gadget =
        Blake2sGadget::check_evaluation_gadget(&mut cs.ns(|| "eval_blake2s"), &seed_gadget, &input_gadget).unwrap();
    output_gadget.enforce_equal(&mut cs, &actual_out_gadget).unwrap();

    if !cs.is_satisfied() {
        println!("which is unsatisfied: {:?}", cs.which_is_unsatisfied().unwrap());
    }
    assert!(cs.is_satisfied());
}

#[test]
fn test_blake2s_precomp_constraints() {
    // Test that 512 fixed leading bits (constants)
    // doesn't result in more constraints.

    let mut cs = TestConstraintSystem::<Fr>::new();
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);
    let input_bits: Vec<_> = (0..512)
        .map(|_| Boolean::constant(rng.gen()))
        .chain((0..512).map(|i| {
            AllocatedBit::alloc(cs.ns(|| format!("input bit_gadget {}", i)), || Ok(true))
                .unwrap()
                .into()
        }))
        .collect();
    blake2s_gadget(&mut cs, &input_bits).unwrap();
    assert!(cs.is_satisfied());
    assert_eq!(cs.num_constraints(), 21792);
}

#[test]
fn test_blake2s_constant_constraints() {
    let mut cs = TestConstraintSystem::<Fr>::new();
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);
    let input_bits: Vec<_> = (0..512).map(|_| Boolean::constant(rng.gen())).collect();
    blake2s_gadget(&mut cs, &input_bits).unwrap();
    assert_eq!(cs.num_constraints(), 0);
}

#[test]
fn test_blake2s() {
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

    for input_len in (0..32).chain((32..256).filter(|a| a % 8 == 0)) {
        let mut h = VarBlake2s::new(32).unwrap();

        let data: Vec<u8> = (0..input_len).map(|_| rng.gen()).collect();

        h.update(&data);

        let mut hash_result = Vec::new();
        h.finalize_variable(|output| hash_result.extend_from_slice(output));

        let mut cs = TestConstraintSystem::<Fr>::new();

        let mut input_bits = vec![];

        for (byte_i, input_byte) in data.into_iter().enumerate() {
            for bit_i in 0..8 {
                let cs = cs.ns(|| format!("input bit_gadget {} {}", byte_i, bit_i));

                input_bits.push(
                    AllocatedBit::alloc(cs, || Ok((input_byte >> bit_i) & 1u8 == 1u8))
                        .unwrap()
                        .into(),
                );
            }
        }

        let r = blake2s_gadget(&mut cs, &input_bits).unwrap();

        assert!(cs.is_satisfied());

        let mut s = hash_result
            .iter()
            .flat_map(|&byte| (0..8).map(move |i| (byte >> i) & 1u8 == 1u8));

        for chunk in r {
            for b in chunk.to_bits_le() {
                match b {
                    Boolean::Is(b) => {
                        assert!(s.next().unwrap() == b.get_value().unwrap());
                    }
                    Boolean::Not(b) => {
                        assert!(s.next().unwrap() != b.get_value().unwrap());
                    }
                    Boolean::Constant(b) => {
                        assert!(input_len == 0);
                        assert!(s.next().unwrap() == b);
                    }
                }
            }
        }
    }
}
