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

use super::{push_constraints, r1cs_to_qap::R1CStoQAP, Parameters, Proof};
use crate::{cfg_into_iter, msm::VariableBaseMSM};
use snarkvm_errors::gadgets::SynthesisError;
use snarkvm_models::{
    curves::{AffineCurve, Group, One, PairingEngine, PrimeField, ProjectiveCurve, Zero},
    gadgets::{
        r1cs::{ConstraintSynthesizer, ConstraintSystem, Index, LinearCombination, Variable},
        utilities::OptionalVec,
    },
};
use snarkvm_profiler::{end_timer, start_timer};
use snarkvm_utilities::rand::UniformRand;

use rand::Rng;

#[derive(Default)]
pub struct Namespace {
    constraint_indices: Vec<usize>,
    input_indices: Vec<usize>,
    aux_indices: Vec<usize>,
}

#[cfg(feature = "parallel")]
use rayon::prelude::*;

pub struct ProvingAssignment<E: PairingEngine> {
    // Constraints
    pub(crate) at: OptionalVec<Vec<(E::Fr, Index)>>,
    pub(crate) bt: OptionalVec<Vec<(E::Fr, Index)>>,
    pub(crate) ct: OptionalVec<Vec<(E::Fr, Index)>>,

    // Assignments of variables
    pub(crate) input_assignment: OptionalVec<E::Fr>,
    pub(crate) aux_assignment: OptionalVec<E::Fr>,

    pub(crate) namespaces: Vec<Namespace>,
}

impl<E: PairingEngine> ConstraintSystem<E::Fr> for ProvingAssignment<E> {
    type Root = Self;

    #[inline]
    fn alloc<F, A, AR>(&mut self, _: A, f: F) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> Result<E::Fr, SynthesisError>,
        A: FnOnce() -> AR,
        AR: AsRef<str>,
    {
        let index = self.aux_assignment.insert(f()?);
        if let Some(ref mut ns) = self.namespaces.last_mut() {
            ns.aux_indices.push(index);
        }
        Ok(Variable::new_unchecked(Index::Aux(index)))
    }

    #[inline]
    fn alloc_input<F, A, AR>(&mut self, _: A, f: F) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> Result<E::Fr, SynthesisError>,
        A: FnOnce() -> AR,
        AR: AsRef<str>,
    {
        let index = self.input_assignment.insert(f()?);
        if let Some(ref mut ns) = self.namespaces.last_mut() {
            ns.input_indices.push(index);
        }
        Ok(Variable::new_unchecked(Index::Input(index)))
    }

    #[inline]
    fn enforce<A, AR, LA, LB, LC>(&mut self, _: A, a: LA, b: LB, c: LC)
    where
        A: FnOnce() -> AR,
        AR: AsRef<str>,
        LA: FnOnce(LinearCombination<E::Fr>) -> LinearCombination<E::Fr>,
        LB: FnOnce(LinearCombination<E::Fr>) -> LinearCombination<E::Fr>,
        LC: FnOnce(LinearCombination<E::Fr>) -> LinearCombination<E::Fr>,
    {
        // the indices are aligned; all the constraints are pushed and popped together
        let idx = push_constraints(a(LinearCombination::zero()), &mut self.at);
        push_constraints(b(LinearCombination::zero()), &mut self.bt);
        push_constraints(c(LinearCombination::zero()), &mut self.ct);

        if let Some(ref mut ns) = self.namespaces.last_mut() {
            ns.constraint_indices.push(idx);
        }
    }

    fn push_namespace<NR, N>(&mut self, _: N)
    where
        NR: AsRef<str>,
        N: FnOnce() -> NR,
    {
        self.namespaces.push(Default::default());
    }

    fn pop_namespace(&mut self) {
        if let Some(ns) = self.namespaces.pop() {
            for idx in ns.constraint_indices {
                self.at.remove(idx);
                self.bt.remove(idx);
                self.ct.remove(idx);
            }

            for idx in ns.aux_indices {
                self.aux_assignment.remove(idx);
            }

            for idx in ns.input_indices {
                self.input_assignment.remove(idx);
            }
        }
    }

    fn get_root(&mut self) -> &mut Self::Root {
        self
    }

    fn num_constraints(&self) -> usize {
        self.at.len()
    }
}

pub fn create_random_proof<E, C, R>(
    circuit: &C,
    params: &Parameters<E>,
    rng: &mut R,
) -> Result<Proof<E>, SynthesisError>
where
    E: PairingEngine,
    C: ConstraintSynthesizer<E::Fr>,
    R: Rng,
{
    let r = E::Fr::rand(rng);
    let s = E::Fr::rand(rng);

    create_proof::<E, C>(circuit, params, r, s)
}

pub fn create_proof_no_zk<E, C>(circuit: &C, params: &Parameters<E>) -> Result<Proof<E>, SynthesisError>
where
    E: PairingEngine,
    C: ConstraintSynthesizer<E::Fr>,
{
    create_proof::<E, C>(circuit, params, E::Fr::zero(), E::Fr::zero())
}

pub fn create_proof<E, C>(circuit: &C, params: &Parameters<E>, r: E::Fr, s: E::Fr) -> Result<Proof<E>, SynthesisError>
where
    E: PairingEngine,
    C: ConstraintSynthesizer<E::Fr>,
{
    let prover_time = start_timer!(|| "Prover");
    let mut prover = ProvingAssignment {
        at: Default::default(),
        bt: Default::default(),
        ct: Default::default(),
        input_assignment: Default::default(),
        aux_assignment: Default::default(),
        namespaces: Default::default(),
    };

    // Allocate the "one" input variable
    prover.alloc_input(|| "", || Ok(E::Fr::one()))?;

    // Synthesize the circuit.
    let synthesis_time = start_timer!(|| "Constraint synthesis");
    circuit.generate_constraints(&mut prover)?;
    end_timer!(synthesis_time);

    let witness_map_time = start_timer!(|| "R1CS to QAP witness map");
    let h = R1CStoQAP::witness_map::<E>(&prover)?;
    end_timer!(witness_map_time);

    let input_assignment = prover
        .input_assignment
        .iter()
        .skip(1)
        .map(|s| s.into_repr())
        .collect::<Vec<_>>();

    let aux_assignment = cfg_into_iter!(prover.aux_assignment)
        .map(|s| s.into_repr())
        .collect::<Vec<_>>();

    let assignment = [&input_assignment[..], &aux_assignment[..]].concat();

    let h_assignment = cfg_into_iter!(h).map(|s| s.into_repr()).collect::<Vec<_>>();

    // Compute A
    let a_acc_time = start_timer!(|| "Compute A");
    let a_query = &params.a_query;
    let r_g1 = params.delta_g1.mul(r);

    let g_a = calculate_coeff(r_g1, a_query, params.vk.alpha_g1, &assignment);

    end_timer!(a_acc_time);

    // Compute B in G1 if needed
    let g1_b = if r != E::Fr::zero() {
        let b_g1_acc_time = start_timer!(|| "Compute B in G1");
        let s_g1 = params.delta_g1.mul(s);
        let b_query = &params.b_g1_query;

        let g1_b = calculate_coeff(s_g1, b_query, params.beta_g1, &assignment);

        end_timer!(b_g1_acc_time);

        g1_b
    } else {
        E::G1Projective::zero()
    };

    // Compute B in G2
    let b_g2_acc_time = start_timer!(|| "Compute B in G2");
    let b_query = &params.b_g2_query;
    let s_g2 = params.vk.delta_g2.mul(s);
    let g2_b = calculate_coeff(s_g2, &b_query, params.vk.beta_g2, &assignment);

    end_timer!(b_g2_acc_time);

    // Compute C
    let c_acc_time = start_timer!(|| "Compute C");

    let h_query = &params.h_query;
    let h_acc = VariableBaseMSM::multi_scalar_mul(&h_query, &h_assignment);

    let l_aux_source = &params.l_query;
    let l_aux_acc = VariableBaseMSM::multi_scalar_mul(l_aux_source, &aux_assignment);

    let s_g_a = g_a.mul(&s);
    let r_g1_b = g1_b.mul(&r);
    let r_s_delta_g1 = params.delta_g1.into_projective().mul(&r).mul(&s);

    let mut g_c = s_g_a;
    g_c += &r_g1_b;
    g_c -= &r_s_delta_g1;
    g_c += &l_aux_acc;
    g_c += &h_acc;
    end_timer!(c_acc_time);

    end_timer!(prover_time);

    Ok(Proof {
        a: g_a.into_affine(),
        b: g2_b.into_affine(),
        c: g_c.into_affine(),
        compressed: true,
    })
}

fn calculate_coeff<G: AffineCurve>(
    initial: G::Projective,
    query: &[G],
    vk_param: G,
    assignment: &[<G::ScalarField as PrimeField>::BigInteger],
) -> G::Projective {
    let el = query[0];
    let acc = VariableBaseMSM::multi_scalar_mul(&query[1..], assignment);

    let mut res = initial;
    res.add_assign_mixed(&el);
    res += &acc;
    res.add_assign_mixed(&vk_param);

    res
}
