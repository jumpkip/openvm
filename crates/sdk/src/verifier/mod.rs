use internal::types::InternalVmVerifierPvs;
use openvm_native_circuit::NativeConfig;
use openvm_native_compiler::ir::DIGEST_SIZE;

use crate::{config::AggStarkConfig, verifier::common::types::VmVerifierPvs};

pub mod common;
pub mod internal;
pub mod leaf;
pub mod root;
pub mod utils;

const SBOX_SIZE: usize = 7;

impl AggStarkConfig {
    pub fn leaf_vm_config(&self) -> NativeConfig {
        let mut config = NativeConfig::aggregation(
            VmVerifierPvs::<u8>::width(),
            SBOX_SIZE.min(self.leaf_fri_params.max_constraint_degree()),
        );
        config.system.profiling = self.profiling;
        config
    }
    pub fn internal_vm_config(&self) -> NativeConfig {
        let mut config = NativeConfig::aggregation(
            InternalVmVerifierPvs::<u8>::width(),
            SBOX_SIZE.min(self.internal_fri_params.max_constraint_degree()),
        );
        config.system.profiling = self.profiling;
        config
    }
    pub fn root_verifier_vm_config(&self) -> NativeConfig {
        let mut config = NativeConfig::aggregation(
            // app_commit + leaf_verifier_commit + public_values
            DIGEST_SIZE * 2 + self.max_num_user_public_values,
            SBOX_SIZE.min(self.root_fri_params.max_constraint_degree()),
        );
        config.system.profiling = self.profiling;
        config
    }
}
