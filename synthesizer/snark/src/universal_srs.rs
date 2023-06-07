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

use super::*;

use std::marker::PhantomData;

#[derive(Clone)]
pub struct UniversalSRS<N: Network>(PhantomData<N>);

impl<N: Network> UniversalSRS<N> {
    /// Initializes the universal SRS.
    pub const fn load() -> Self {
        Self(PhantomData)
    }

    /// Returns the circuit proving and verifying key.
    pub fn to_circuit_key(
        &self,
        function_name: &str,
        assignment: &circuit::Assignment<N::Field>,
    ) -> Result<(ProvingKey<N>, VerifyingKey<N>)> {
        #[cfg(feature = "aleo-cli")]
        let timer = std::time::Instant::now();

        let (proving_key, verifying_key) = Marlin::<N>::circuit_setup(self, assignment)?;

        #[cfg(feature = "aleo-cli")]
        println!("{}", format!(" • Built '{function_name}' (in {} ms)", timer.elapsed().as_millis()).dimmed());

        Ok((ProvingKey::new(Arc::new(proving_key)), VerifyingKey::new(Arc::new(verifying_key))))
    }
}

impl<N: Network> Deref for UniversalSRS<N> {
    type Target = marlin::UniversalSRS<N::PairingCurve>;

    #[allow(clippy::let_and_return)]
    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "aleo-cli")]
        let timer = std::time::Instant::now();

        // Load the universal SRS.
        let universal_srs = N::universal_srs();

        #[cfg(feature = "aleo-cli")]
        println!("{}", format!(" • Loaded universal setup (in {} ms)", timer.elapsed().as_millis()).dimmed());

        universal_srs
    }
}
