use std::{cell::RefCell, ops::Deref, rc::Rc, sync::Arc};

use afs_primitives::var_range::{bus::VariableRangeCheckerBus, VariableRangeCheckerChip};
use afs_stark_backend::{engine::VerificationData, rap::AnyRap, verifier::VerificationError};
use ax_sdk::{
    config::baby_bear_poseidon2::{self, BabyBearPoseidon2Config},
    engine::StarkEngine,
};
use p3_baby_bear::BabyBear;
use p3_field::PrimeField32;
use p3_matrix::{dense::RowMajorMatrix, Matrix};
use rand::{rngs::StdRng, SeedableRng};

use crate::{
    arch::chips::MachineChip,
    cpu::{trace::Instruction, RANGE_CHECKER_BUS},
    memory::{offline_checker::MemoryBus, MemoryChip},
    vm::config::MemoryConfig,
};

pub mod execution;
pub mod memory;

pub use execution::ExecutionTester;
pub use memory::MemoryTester;

use super::{bus::ExecutionBus, chips::InstructionExecutor};
use crate::memory::MemoryChipRef;

#[derive(Clone, Debug)]
pub struct MachineChipTestBuilder<F: PrimeField32> {
    pub memory: MemoryTester<F>,
    pub execution: ExecutionTester<F>,
}

impl<F: PrimeField32> MachineChipTestBuilder<F> {
    pub fn new(memory_chip: MemoryChipRef<F>, execution_bus: ExecutionBus, rng: StdRng) -> Self {
        Self {
            memory: MemoryTester::new(memory_chip),
            execution: ExecutionTester::new(execution_bus, rng),
        }
    }

    // Passthrough functions from ExecutionTester and MemoryTester for better dev-ex
    pub fn execute<E: InstructionExecutor<F>>(
        &mut self,
        executor: &mut E,
        instruction: Instruction<F>,
    ) {
        self.execution
            .execute(&mut self.memory, executor, instruction);
    }

    pub fn read_cell(&mut self, address_space: usize, pointer: usize) -> F {
        self.memory.read_cell(address_space, pointer)
    }

    pub fn write_cell(&mut self, address_space: usize, pointer: usize, value: F) {
        self.memory.write_cell(address_space, pointer, value);
    }

    pub fn read<const N: usize>(&mut self, address_space: usize, pointer: usize) -> [F; N] {
        self.memory.read(address_space, pointer)
    }

    pub fn write<const N: usize>(&mut self, address_space: usize, pointer: usize, value: [F; N]) {
        self.memory.write(address_space, pointer, value);
    }

    pub fn execution_bus(&self) -> ExecutionBus {
        self.execution.bus
    }

    pub fn memory_bus(&self) -> MemoryBus {
        self.memory.bus
    }

    pub fn memory_chip(&self) -> MemoryChipRef<F> {
        self.memory.chip.clone()
    }
}

impl MachineChipTestBuilder<BabyBear> {
    pub fn build(self) -> MachineChipTester {
        let tester = MachineChipTester {
            memory: Some(self.memory),
            ..Default::default()
        };
        tester.load(self.execution)
    }
}

impl<F: PrimeField32> Default for MachineChipTestBuilder<F> {
    fn default() -> Self {
        let mem_config = MemoryConfig::new(2, 29, 29, 16); // smaller testing config with smaller decomp_bits
        let range_checker = Arc::new(VariableRangeCheckerChip::new(VariableRangeCheckerBus::new(
            RANGE_CHECKER_BUS,
            mem_config.decomp,
        )));
        let memory_chip = MemoryChip::with_volatile_memory(MemoryBus(1), mem_config, range_checker);
        Self {
            memory: MemoryTester::new(Rc::new(RefCell::new(memory_chip))),
            execution: ExecutionTester::new(ExecutionBus(0), StdRng::seed_from_u64(0)),
        }
    }
}

// TODO[jpw]: generic Config
#[derive(Default)]
pub struct MachineChipTester {
    pub memory: Option<MemoryTester<BabyBear>>,
    pub airs: Vec<Box<dyn AnyRap<BabyBearPoseidon2Config>>>,
    pub traces: Vec<RowMajorMatrix<BabyBear>>,
    pub public_values: Vec<Vec<BabyBear>>,
}

impl MachineChipTester {
    pub fn load<C: MachineChip<BabyBear>>(mut self, mut chip: C) -> Self {
        self.public_values.push(chip.generate_public_values());
        self.airs.push(chip.air());
        self.traces.push(chip.generate_trace());

        self
    }

    pub fn finalize(mut self) -> Self {
        if let Some(memory_tester) = self.memory.take() {
            let manager = memory_tester.chip.clone();
            let range_checker = manager.borrow().range_checker.clone();
            self = self.load(memory_tester); // dummy memory interactions
            self = self.load(manager); // memory initial and final state
            self = self.load(range_checker); // this must be last because other trace generation mutates its state
        }
        self
    }

    pub fn load_with_custom_trace<C: MachineChip<BabyBear>>(
        mut self,
        mut chip: C,
        trace: RowMajorMatrix<BabyBear>,
    ) -> Self {
        self.traces.push(trace);
        self.public_values.push(chip.generate_public_values());
        self.airs.push(chip.air());
        self
    }

    pub fn simple_test(
        &self,
    ) -> Result<VerificationData<BabyBearPoseidon2Config>, VerificationError> {
        self.test(baby_bear_poseidon2::default_engine)
    }

    fn max_trace_height(&self) -> usize {
        self.traces
            .iter()
            .map(RowMajorMatrix::height)
            .max()
            .unwrap()
    }
    /// Given a function to produce an engine from the max trace height,
    /// runs a simple test on that engine
    pub fn test<E: StarkEngine<BabyBearPoseidon2Config>, P: Fn(usize) -> E>(
        &self, // do no take ownership so it's easier to prank
        engine_provider: P,
    ) -> Result<VerificationData<BabyBearPoseidon2Config>, VerificationError> {
        assert!(self.memory.is_none(), "Memory must be finalized");
        let chips: Vec<_> = self.airs.iter().map(|x| x.deref()).collect();
        engine_provider(self.max_trace_height()).run_simple_test(
            &chips,
            self.traces.clone(),
            &self.public_values,
        )
    }
}
