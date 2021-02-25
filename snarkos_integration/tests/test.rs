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

use snarkos_storage::mem::MemDb;
use snarkos_testing::storage::*;
use snarkvm_algorithms::merkle_tree::MerklePath;
use snarkvm_curves::bls12_377::{Fq, Fr};
use snarkvm_dpc::{
    base_dpc::{
        execute_inner_proof_gadget,
        execute_outer_proof_gadget,
        inner_circuit::InnerCircuit,
        parameters::{NoopProgramSNARKParameters, SystemParameters},
        program::*,
        record::record_encryption::*,
        record_payload::RecordPayload,
        BaseDPCComponents,
        TransactionKernel,
        DPC,
    },
    instantiated::*,
};
use snarkvm_models::{
    algorithms::{MerkleParameters, CRH, SNARK},
    dpc::{DPCScheme, Program, Record},
    gadgets::r1cs::{ConstraintSystem, TestConstraintSystem},
    objects::{AccountScheme, LedgerScheme},
};
use snarkvm_objects::{
    dpc::DPCTransactions,
    Account,
    Block,
    BlockHeader,
    BlockHeaderHash,
    MerkleRootHash,
    PedersenMerkleRootHash,
    ProofOfSuccinctWork,
};
use snarkvm_utilities::{
    bytes::{FromBytes, ToBytes},
    to_bytes,
};

use itertools::Itertools;
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

type L = Ledger<Tx, CommitmentMerkleParameters, MemDb>;

/// Generates and returns noop program parameters and its corresponding program id.
fn generate_test_noop_program_parameters<R: Rng>(
    system_parameters: &SystemParameters<Components>,
    rng: &mut R,
) -> (NoopProgramSNARKParameters<Components>, Vec<u8>) {
    let noop_program_snark_pp =
        InstantiatedDPC::generate_noop_program_snark_parameters(&system_parameters, rng).unwrap();

    let noop_program_id = to_bytes![
        ProgramVerificationKeyCRH::hash(
            &system_parameters.program_verification_key_crh,
            &to_bytes![noop_program_snark_pp.verification_key].unwrap()
        )
        .unwrap()
    ]
    .unwrap();

    (noop_program_snark_pp, noop_program_id)
}

#[test]
fn test_transaction_kernel_serialization() {
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

    // Generate parameters for the ledger, commitment schemes, CRH, and the
    // "always-accept" program.
    let system_parameters = InstantiatedDPC::generate_system_parameters(&mut rng).unwrap();

    let (_noop_program_snark_pp, noop_program_id) = generate_test_noop_program_parameters(&system_parameters, &mut rng);

    // Generate metadata and an account for a dummy initial record.
    let test_account = Account::new(
        &system_parameters.account_signature,
        &system_parameters.account_commitment,
        &system_parameters.account_encryption,
        &mut rng,
    )
    .unwrap();

    let sn_nonce = SerialNumberNonce::hash(&system_parameters.serial_number_nonce, &[0u8; 1]).unwrap();
    let old_record = DPC::generate_record(
        &system_parameters,
        sn_nonce,
        test_account.address.clone(),
        true,
        0,
        RecordPayload::default(),
        noop_program_id.clone(),
        noop_program_id.clone(),
        &mut rng,
    )
    .unwrap();

    // Set the input records for our transaction to be the initial dummy records.
    let old_records = vec![old_record; NUM_INPUT_RECORDS];
    let old_account_private_keys = vec![test_account.private_key.clone(); NUM_INPUT_RECORDS];

    // Construct new records.

    let new_record_owners = vec![test_account.address; NUM_OUTPUT_RECORDS];
    let new_is_dummy_flags = vec![false; NUM_OUTPUT_RECORDS];
    let new_values = vec![10; NUM_OUTPUT_RECORDS];
    let new_payloads = vec![RecordPayload::default(); NUM_OUTPUT_RECORDS];
    let new_birth_program_ids = vec![noop_program_id.clone(); NUM_OUTPUT_RECORDS];
    let new_death_program_ids = vec![noop_program_id.clone(); NUM_OUTPUT_RECORDS];
    let memo = [0u8; 32];

    // Generate transaction kernel
    let transaction_kernel = <InstantiatedDPC as DPCScheme<L>>::execute_offline(
        system_parameters.clone(),
        old_records,
        old_account_private_keys,
        new_record_owners,
        &new_is_dummy_flags,
        &new_values,
        new_payloads,
        new_birth_program_ids,
        new_death_program_ids,
        memo,
        0,
        &mut rng,
    )
    .unwrap();

    // Serialize the transaction kernel
    let transaction_kernel_bytes = to_bytes![&transaction_kernel].unwrap();

    let recovered_transaction_kernel: <InstantiatedDPC as DPCScheme<L>>::TransactionKernel =
        FromBytes::read(&transaction_kernel_bytes[..]).unwrap();

    assert_eq!(transaction_kernel, recovered_transaction_kernel);
}

#[test]
fn test_execute_base_dpc_constraints() {
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

    // Specify network_id
    let network_id: u8 = 0;

    // Generate parameters for the ledger, commitment schemes, CRH, and the
    // "always-accept" program.
    let ledger_parameters = CommitmentMerkleParameters::setup(&mut rng);
    let system_parameters = InstantiatedDPC::generate_system_parameters(&mut rng).unwrap();

    let (noop_program_snark_pp, noop_program_id) = generate_test_noop_program_parameters(&system_parameters, &mut rng);
    let (alternate_noop_program_snark_pp, alternate_noop_program_id) =
        generate_test_noop_program_parameters(&system_parameters, &mut rng);

    let signature_parameters = &system_parameters.account_signature;
    let commitment_parameters = &system_parameters.account_commitment;
    let encryption_parameters = &system_parameters.account_encryption;

    // Generate metadata and an account for a dummy initial record.
    let dummy_account = Account::new(
        signature_parameters,
        commitment_parameters,
        encryption_parameters,
        &mut rng,
    )
    .unwrap();

    let genesis_block = Block {
        header: BlockHeader {
            previous_block_hash: BlockHeaderHash([0u8; 32]),
            merkle_root_hash: MerkleRootHash([0u8; 32]),
            time: 0,
            difficulty_target: 0x07FF_FFFF_FFFF_FFFF_u64,
            nonce: 0,
            pedersen_merkle_root_hash: PedersenMerkleRootHash([0u8; 32]),
            proof: ProofOfSuccinctWork::default(),
        },
        transactions: DPCTransactions::new(),
    };

    // Use genesis record, serial number, and memo to initialize the ledger.
    let ledger = initialize_test_blockchain::<Tx, CommitmentMerkleParameters, MemDb>(ledger_parameters, genesis_block);

    let sn_nonce = SerialNumberNonce::hash(&system_parameters.serial_number_nonce, &[0u8; 1]).unwrap();
    let old_record = DPC::generate_record(
        &system_parameters,
        sn_nonce,
        dummy_account.address,
        true,
        0,
        RecordPayload::default(),
        alternate_noop_program_id.clone(),
        alternate_noop_program_id.clone(),
        &mut rng,
    )
    .unwrap();

    // Set the input records for our transaction to be the initial dummy records.
    let old_records = vec![old_record; NUM_INPUT_RECORDS];
    let old_account_private_keys = vec![dummy_account.private_key; NUM_INPUT_RECORDS];

    // Construct new records.

    // Create an account for an actual new record.

    let new_account = Account::new(
        signature_parameters,
        commitment_parameters,
        encryption_parameters,
        &mut rng,
    )
    .unwrap();

    // Set the new record's program to be the "always-accept" program.

    let new_record_owners = vec![new_account.address; NUM_OUTPUT_RECORDS];
    let new_is_dummy_flags = vec![false; NUM_OUTPUT_RECORDS];
    let new_values = vec![10; NUM_OUTPUT_RECORDS];
    let new_payloads = vec![RecordPayload::default(); NUM_OUTPUT_RECORDS];
    let new_birth_program_ids = vec![noop_program_id.clone(); NUM_OUTPUT_RECORDS];
    let new_death_program_ids = vec![noop_program_id.clone(); NUM_OUTPUT_RECORDS];
    let memo = [0u8; 32];

    let transaction_kernel = <InstantiatedDPC as DPCScheme<L>>::execute_offline(
        system_parameters.clone(),
        old_records,
        old_account_private_keys,
        new_record_owners,
        &new_is_dummy_flags,
        &new_values,
        new_payloads,
        new_birth_program_ids,
        new_death_program_ids,
        memo,
        network_id,
        &mut rng,
    )
    .unwrap();

    let local_data = transaction_kernel.into_local_data();

    // Generate the program proofs

    let noop_program = NoopProgram::<_, <Components as BaseDPCComponents>::NoopProgramSNARK>::new(noop_program_id);
    let alternate_noop_program =
        NoopProgram::<_, <Components as BaseDPCComponents>::NoopProgramSNARK>::new(alternate_noop_program_id);

    let mut old_proof_and_vk = vec![];
    for i in 0..NUM_INPUT_RECORDS {
        let private_input = alternate_noop_program
            .execute(
                &alternate_noop_program_snark_pp.proving_key,
                &alternate_noop_program_snark_pp.verification_key,
                &local_data,
                i as u8,
                &mut rng,
            )
            .unwrap();

        old_proof_and_vk.push(private_input);
    }

    let mut new_proof_and_vk = vec![];
    for j in 0..NUM_OUTPUT_RECORDS {
        let private_input = noop_program
            .execute(
                &noop_program_snark_pp.proving_key,
                &noop_program_snark_pp.verification_key,
                &local_data,
                (NUM_INPUT_RECORDS + j) as u8,
                &mut rng,
            )
            .unwrap();

        new_proof_and_vk.push(private_input);
    }

    let TransactionKernel {
        system_parameters: _,

        old_records,
        old_account_private_keys,
        old_serial_numbers,
        old_randomizers: _,

        new_records,
        new_sn_nonce_randomness,
        new_commitments,

        new_records_encryption_randomness,
        new_encrypted_records: _,
        new_encrypted_record_hashes,

        program_commitment,
        program_randomness,
        local_data_merkle_tree,
        local_data_commitment_randomizers,
        value_balance,
        memorandum,
        network_id,
    } = transaction_kernel;

    let local_data_root = local_data_merkle_tree.root();

    // Construct the ledger witnesses
    let ledger_digest = ledger.digest().expect("could not get digest");

    // Generate the ledger membership witnesses
    let mut old_witnesses = Vec::with_capacity(NUM_INPUT_RECORDS);

    // Compute the ledger membership witness and serial number from the old records.
    for record in old_records.iter() {
        if record.is_dummy() {
            old_witnesses.push(MerklePath::default());
        } else {
            let witness = ledger.prove_cm(&record.commitment()).unwrap();
            old_witnesses.push(witness);
        }
    }

    // Prepare record encryption components used in the inner SNARK
    let mut new_records_encryption_gadget_components = Vec::with_capacity(NUM_OUTPUT_RECORDS);
    for (record, ciphertext_randomness) in new_records.iter().zip_eq(&new_records_encryption_randomness) {
        let record_encryption_gadget_components =
            RecordEncryption::prepare_encryption_gadget_components(&system_parameters, &record, ciphertext_randomness)
                .unwrap();

        new_records_encryption_gadget_components.push(record_encryption_gadget_components);
    }

    //////////////////////////////////////////////////////////////////////////
    // Check that the core check constraint system was satisfied.
    let mut core_cs = TestConstraintSystem::<Fr>::new();

    execute_inner_proof_gadget::<_, _>(
        &mut core_cs.ns(|| "Core checks"),
        &system_parameters,
        ledger.parameters(),
        &ledger_digest,
        &old_records,
        &old_witnesses,
        &old_account_private_keys,
        &old_serial_numbers,
        &new_records,
        &new_sn_nonce_randomness,
        &new_commitments,
        &new_records_encryption_randomness,
        &new_records_encryption_gadget_components,
        &new_encrypted_record_hashes,
        &program_commitment,
        &program_randomness,
        &local_data_root,
        &local_data_commitment_randomizers,
        &memo,
        value_balance,
        network_id,
    )
    .unwrap();

    if !core_cs.is_satisfied() {
        println!("=========================================================");
        println!("num constraints: {:?}", core_cs.num_constraints());
        println!("Unsatisfied constraints:");
        println!("{}", core_cs.which_is_unsatisfied().unwrap());
        println!("=========================================================");
    }

    if core_cs.is_satisfied() {
        println!("\n\n\n\nAll Core check constraints:");
        //        core_cs.print_named_objects();
        println!("num constraints: {:?}", core_cs.num_constraints());
    }
    println!("=========================================================");
    println!("=========================================================");
    println!("=========================================================\n\n\n");

    assert!(core_cs.is_satisfied());

    // Generate inner snark parameters and proof for verification in the outer snark
    let inner_snark_parameters = <Components as BaseDPCComponents>::InnerSNARK::setup(
        &InnerCircuit::blank(&system_parameters, ledger.parameters()),
        &mut rng,
    )
    .unwrap();

    let inner_snark_vk: <<Components as BaseDPCComponents>::InnerSNARK as SNARK>::VerificationParameters =
        inner_snark_parameters.1.clone().into();

    let inner_snark_id = InnerSNARKVerificationKeyCRH::hash(
        &system_parameters.inner_snark_verification_key_crh,
        &to_bytes![inner_snark_vk].unwrap(),
    )
    .unwrap();

    let inner_snark_proof = <Components as BaseDPCComponents>::InnerSNARK::prove(
        &inner_snark_parameters.0,
        &InnerCircuit::new(
            system_parameters.clone(),
            ledger.parameters().clone(),
            ledger_digest,
            old_records,
            old_witnesses,
            old_account_private_keys,
            old_serial_numbers.clone(),
            new_records,
            new_sn_nonce_randomness,
            new_commitments.clone(),
            new_records_encryption_randomness,
            new_records_encryption_gadget_components,
            new_encrypted_record_hashes.clone(),
            program_commitment,
            program_randomness,
            local_data_root,
            local_data_commitment_randomizers,
            memo,
            value_balance,
            network_id,
        ),
        &mut rng,
    )
    .unwrap();

    // Check that the proof check constraint system was satisfied.
    let mut pf_check_cs = TestConstraintSystem::<Fq>::new();

    execute_outer_proof_gadget::<_, _>(
        &mut pf_check_cs.ns(|| "Check program proofs"),
        &system_parameters,
        ledger.parameters(),
        &ledger_digest,
        &old_serial_numbers,
        &new_commitments,
        &new_encrypted_record_hashes,
        &memorandum,
        value_balance,
        network_id,
        &inner_snark_vk,
        &inner_snark_proof,
        &old_proof_and_vk,
        &new_proof_and_vk,
        &program_commitment,
        &program_randomness,
        &local_data_root,
        &inner_snark_id,
    )
    .unwrap();

    if !pf_check_cs.is_satisfied() {
        println!("=========================================================");
        println!("num constraints: {:?}", pf_check_cs.num_constraints());
        println!("Unsatisfied constraints:");
        println!("{}", pf_check_cs.which_is_unsatisfied().unwrap());
        println!("=========================================================");
    }
    if pf_check_cs.is_satisfied() {
        println!("\n\n\n\nAll Proof check constraints:");
        // pf_check_cs.print_named_objects();
        println!("num constraints: {:?}", pf_check_cs.num_constraints());
    }
    println!("=========================================================");
    println!("=========================================================");
    println!("=========================================================");

    assert!(pf_check_cs.is_satisfied());
}
