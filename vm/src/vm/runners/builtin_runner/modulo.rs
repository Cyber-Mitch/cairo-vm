use crate::{
    air_private_input::{ModInput, ModInputInstance, ModInputMemoryVars, PrivateInput},
    math_utils::{div_mod_unsigned, safe_div_usize},
    stdlib::{borrow::Cow, collections::BTreeMap},
    types::{
        errors::math_errors::MathError,
        instance_definitions::mod_instance_def::{ModInstanceDef, N_WORDS},
        relocatable::{MaybeRelocatable, Relocatable},
    },
    vm::{
        errors::{
            memory_errors::MemoryError, runner_errors::RunnerError, vm_errors::VirtualMachineError,
        },
        vm_core::VirtualMachine,
        vm_memory::{memory::Memory, memory_segments::MemorySegmentManager},
    },
    Felt252,
};
use core::{fmt::Display, ops::Shl};
use num_bigint::BigUint;
use num_integer::div_ceil;
use num_integer::Integer;
use num_traits::One;
use num_traits::Zero;

//The maximum n value that the function fill_memory accepts.
const FILL_MEMORY_MAX: usize = 100000;

const INPUT_CELLS: usize = 7;

const VALUES_PTR_OFFSET: u32 = 4;
const OFFSETS_PTR_OFFSET: u32 = 5;
const N_OFFSET: u32 = 6;

#[derive(Debug, Clone)]
pub struct ModBuiltinRunner {
    builtin_type: ModBuiltinType,
    base: usize,
    pub(crate) stop_ptr: Option<usize>,
    instance_def: ModInstanceDef,
    pub(crate) included: bool,
    zero_segment_index: usize,
    zero_segment_size: usize,
    // Precomputed powers used for reading and writing values that are represented as n_words words of word_bit_len bits each.
    shift: BigUint,
    shift_powers: [BigUint; N_WORDS],
}

#[derive(Debug, Clone)]
pub enum ModBuiltinType {
    Mul,
    Add,
}

#[derive(Debug)]
pub enum Operation {
    Mul,
    Add,
    Sub,
    DivMod(BigUint),
}

impl Display for Operation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Operation::Mul => "*".fmt(f),
            Operation::Add => "+".fmt(f),
            Operation::Sub => "-".fmt(f),
            Operation::DivMod(_) => "/".fmt(f),
        }
    }
}

#[derive(Debug, Default)]
struct Inputs {
    p: BigUint,
    p_values: [Felt252; N_WORDS],
    values_ptr: Relocatable,
    offsets_ptr: Relocatable,
    n: usize,
}

impl ModBuiltinRunner {
    pub(crate) fn new_add_mod(instance_def: &ModInstanceDef, included: bool) -> Self {
        Self::new(instance_def.clone(), included, ModBuiltinType::Add)
    }

    pub(crate) fn new_mul_mod(instance_def: &ModInstanceDef, included: bool) -> Self {
        Self::new(instance_def.clone(), included, ModBuiltinType::Mul)
    }

    fn new(instance_def: ModInstanceDef, included: bool, builtin_type: ModBuiltinType) -> Self {
        let shift = BigUint::one().shl(instance_def.word_bit_len);
        let shift_powers = core::array::from_fn(|i| shift.pow(i as u32));
        let zero_segment_size = core::cmp::max(N_WORDS, instance_def.batch_size * 3);
        Self {
            builtin_type,
            base: 0,
            stop_ptr: None,
            instance_def,
            included,
            zero_segment_index: 0,
            zero_segment_size,
            shift,
            shift_powers,
        }
    }

    pub fn name(&self) -> &'static str {
        match self.builtin_type {
            ModBuiltinType::Mul => super::MUL_MOD_BUILTIN_NAME,
            ModBuiltinType::Add => super::ADD_MOD_BUILTIN_NAME,
        }
    }

    pub fn initialize_segments(&mut self, segments: &mut MemorySegmentManager) {
        self.base = segments.add().segment_index as usize; // segments.add() always returns a positive index
        self.zero_segment_index = segments.add_zero_segment(self.zero_segment_size)
    }

    pub fn initial_stack(&self) -> Vec<MaybeRelocatable> {
        if self.included {
            vec![MaybeRelocatable::from((self.base as isize, 0))]
        } else {
            vec![]
        }
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn ratio(&self) -> Option<u32> {
        self.instance_def.ratio
    }

    pub fn cells_per_instance(&self) -> u32 {
        INPUT_CELLS as u32
    }

    pub fn n_input_cells(&self) -> u32 {
        INPUT_CELLS as u32
    }

    pub fn batch_size(&self) -> usize {
        self.instance_def.batch_size
    }

    pub fn get_used_cells(&self, segments: &MemorySegmentManager) -> Result<usize, MemoryError> {
        segments
            .get_segment_used_size(self.base)
            .ok_or(MemoryError::MissingSegmentUsedSizes)
    }

    pub fn get_used_instances(
        &self,
        segments: &MemorySegmentManager,
    ) -> Result<usize, MemoryError> {
        let used_cells = self.get_used_cells(segments)?;
        Ok(div_ceil(used_cells, self.cells_per_instance() as usize))
    }

    pub(crate) fn air_private_input(&self, segments: &MemorySegmentManager) -> Vec<PrivateInput> {
        let segment_index = self.base as isize;
        let segment_size = segments
            .get_segment_used_size(self.base)
            .unwrap_or_default();
        let mut instances = Vec::<ModInputInstance>::new();
        for instance in 0..segment_size.checked_div(INPUT_CELLS).unwrap_or_default() {
            let instance_addr_offset = instance * INPUT_CELLS;
            let values_ptr = segments
                .memory
                .get_relocatable(
                    (
                        segment_index,
                        instance_addr_offset + VALUES_PTR_OFFSET as usize,
                    )
                        .into(),
                )
                .unwrap_or_default();
            let offsets_ptr = segments
                .memory
                .get_relocatable(
                    (
                        segment_index,
                        instance_addr_offset + OFFSETS_PTR_OFFSET as usize,
                    )
                        .into(),
                )
                .unwrap_or_default();
            let n = segments
                .memory
                .get_usize((segment_index, instance_addr_offset + N_OFFSET as usize).into())
                .unwrap_or_default();
            let p_values: [Felt252; N_WORDS] = core::array::from_fn(|i| {
                segments
                    .memory
                    .get_integer((segment_index, instance_addr_offset + i).into())
                    .unwrap_or_default()
                    .into_owned()
            });
            let mut batch = BTreeMap::<usize, ModInputMemoryVars>::new();
            let fetch_offset_and_words = |var_index: usize,
                                          index_in_batch: usize|
             -> (usize, [Felt252; N_WORDS]) {
                let offset = segments
                    .memory
                    .get_usize((offsets_ptr + (3 * index_in_batch + var_index)).unwrap_or_default())
                    .unwrap_or_default();
                let words: [Felt252; N_WORDS] = core::array::from_fn(|i| {
                    segments
                        .memory
                        .get_integer((values_ptr + (offset + i)).unwrap_or_default())
                        .unwrap_or_default()
                        .into_owned()
                });
                (offset, words)
            };
            for index_in_batch in 0..self.batch_size() {
                let (a_offset, a_values) = fetch_offset_and_words(0, index_in_batch);
                let (b_offset, b_values) = fetch_offset_and_words(1, index_in_batch);
                let (c_offset, c_values) = fetch_offset_and_words(2, index_in_batch);
                batch.insert(
                    index_in_batch,
                    ModInputMemoryVars {
                        a_offset,
                        b_offset,
                        c_offset,
                        a0: a_values[0],
                        a1: a_values[1],
                        a2: a_values[2],
                        a3: a_values[3],
                        b0: b_values[0],
                        b1: b_values[1],
                        b2: b_values[2],
                        b3: b_values[3],
                        c0: c_values[0],
                        c1: c_values[1],
                        c2: c_values[2],
                        c3: c_values[3],
                    },
                );
            }
            instances.push(ModInputInstance {
                index: instance,
                p0: p_values[0],
                p1: p_values[1],
                p2: p_values[2],
                p3: p_values[3],
                values_ptr,
                offsets_ptr,
                n,
                batch,
            });
        }

        vec![PrivateInput::Mod(ModInput {
            instances,
            zero_value_address: segments
                .relocate_segments()
                .ok()
                .and_then(|t| t.get(self.zero_segment_index).cloned())
                .unwrap_or_default(),
        })]
    }

    // Reads N_WORDS from memory, starting at address=addr.
    // Returns the words and the value if all words are in memory.
    // Verifies that all words are integers and are bounded by 2**self.instance_def.word_bit_len.
    fn read_n_words_value(
        &self,
        memory: &Memory,
        addr: Relocatable,
    ) -> Result<([Felt252; N_WORDS], Option<BigUint>), RunnerError> {
        let mut words = Default::default();
        let mut value = BigUint::zero();
        for i in 0..N_WORDS {
            let addr_i = (addr + i)?;
            match memory.get(&addr_i).map(Cow::into_owned) {
                None => return Ok((words, None)),
                Some(MaybeRelocatable::RelocatableValue(_)) => {
                    return Err(MemoryError::ExpectedInteger(Box::new(addr_i)).into())
                }
                Some(MaybeRelocatable::Int(word)) => {
                    let biguint_word = word.to_biguint();
                    if biguint_word >= self.shift {
                        return Err(RunnerError::WordExceedsModBuiltinWordBitLen(
                            addr_i,
                            self.instance_def.word_bit_len,
                            word,
                        ));
                    }
                    words[i] = word;
                    value += biguint_word * &self.shift_powers[i];
                }
            }
        }
        Ok((words, Some(value)))
    }

    // Reads the inputs to the builtin (see Inputs) from the memory at address=addr.
    // Returns a struct with the inputs. Asserts that it exists in memory.
    // Returns also the value of p, not just its words.
    fn read_inputs(&self, memory: &Memory, addr: Relocatable) -> Result<Inputs, RunnerError> {
        let values_ptr = memory.get_relocatable((addr + VALUES_PTR_OFFSET)?)?;
        let offsets_ptr = memory.get_relocatable((addr + OFFSETS_PTR_OFFSET)?)?;
        let n = memory.get_usize((addr + N_OFFSET)?)?;
        if n < 1 {
            return Err(RunnerError::ModBuiltinNLessThanOne(
                self.name().to_string(),
                n,
            ));
        }
        let (p_values, p) = self.read_n_words_value(memory, addr)?;
        let p = p.ok_or_else(|| {
            RunnerError::ModBuiltinMissingValue(
                self.name().to_string(),
                (addr + N_WORDS).unwrap_or_default(),
            )
        })?;
        Ok(Inputs {
            p,
            p_values,
            values_ptr,
            offsets_ptr,
            n,
        })
    }

    // Reads the memory variables to the builtin (see MEMORY_VARS) from the memory given
    // the inputs (specifically, values_ptr and offsets_ptr).
    // Computes and returns the values of a, b, and c.
    fn read_memory_vars(
        &self,
        memory: &Memory,
        values_ptr: Relocatable,
        offsets_ptr: Relocatable,
        index_in_batch: usize,
    ) -> Result<(BigUint, BigUint, BigUint), RunnerError> {
        let compute_value = |index: usize| -> Result<BigUint, RunnerError> {
            let offset = memory.get_usize((offsets_ptr + (index + 3 * index_in_batch))?)?;
            let value_addr = (values_ptr + offset)?;
            let (_, value) = self.read_n_words_value(memory, value_addr)?;
            let value = value.ok_or_else(|| {
                RunnerError::ModBuiltinMissingValue(
                    self.name().to_string(),
                    (value_addr + N_WORDS).unwrap_or_default(),
                )
            })?;
            Ok(value)
        };

        let a = compute_value(0)?;
        let b = compute_value(1)?;
        let c = compute_value(2)?;
        Ok((a, b, c))
    }

    fn fill_inputs(
        &self,
        memory: &mut Memory,
        builtin_ptr: Relocatable,
        inputs: &Inputs,
    ) -> Result<(), RunnerError> {
        if inputs.n > FILL_MEMORY_MAX {
            return Err(RunnerError::FillMemoryMaxExceeded(
                self.name().to_string(),
                FILL_MEMORY_MAX,
            ));
        }
        let n_instances = safe_div_usize(inputs.n, self.instance_def.batch_size)?;
        for instance in 1..n_instances {
            let instance_ptr = (builtin_ptr + instance * INPUT_CELLS)?;
            for i in 0..N_WORDS {
                memory.insert_as_accessed((instance_ptr + i)?, &inputs.p_values[i])?;
            }
            memory.insert_as_accessed((instance_ptr + VALUES_PTR_OFFSET)?, &inputs.values_ptr)?;
            memory.insert_as_accessed(
                (instance_ptr + OFFSETS_PTR_OFFSET)?,
                (inputs.offsets_ptr + (3 * instance * self.instance_def.batch_size))?,
            )?;
            memory.insert_as_accessed(
                (instance_ptr + N_OFFSET)?,
                inputs
                    .n
                    .saturating_sub(instance * self.instance_def.batch_size),
            )?;
        }
        Ok(())
    }

    // Copies the first offsets in the offsets table to its end, n_copies times.
    fn fill_offsets(
        &self,
        memory: &mut Memory,
        offsets_ptr: Relocatable,
        index: usize,
        n_copies: usize,
    ) -> Result<(), RunnerError> {
        if n_copies.is_zero() {
            return Ok(());
        }
        for i in 0..3_usize {
            let addr = (offsets_ptr + i)?;
            let offset = memory
                .get(&((offsets_ptr + i)?))
                .ok_or_else(|| MemoryError::UnknownMemoryCell(Box::new(addr)))?
                .into_owned();
            for copy_i in 0..n_copies {
                memory.insert_as_accessed((offsets_ptr + (3 * (index + copy_i) + i))?, &offset)?;
            }
        }
        Ok(())
    }

    // Given a value, writes its n_words to memory, starting at address=addr.
    fn write_n_words_value(
        &self,
        memory: &mut Memory,
        addr: Relocatable,
        value: BigUint,
    ) -> Result<(), RunnerError> {
        let mut value = value;
        for i in 0..N_WORDS {
            let word = value.mod_floor(&self.shift);
            memory.insert_as_accessed((addr + i)?, Felt252::from(word))?;
            value = value.div_floor(&self.shift)
        }
        if !value.is_zero() {
            return Err(RunnerError::WriteNWordsValueNotZero(
                self.name().to_string(),
            ));
        }
        Ok(())
    }

    // Fills a value in the values table, if exactly one value is missing.
    // Returns true on success or if all values are already known.
    fn fill_value(
        &self,
        memory: &mut Memory,
        inputs: &Inputs,
        index: usize,
        op: &Operation,
        inv_op: &Operation,
    ) -> Result<bool, RunnerError> {
        let mut addresses = Vec::new();
        let mut values = Vec::new();
        for i in 0..3 {
            let addr = (inputs.values_ptr
                + memory
                    .get_integer((inputs.offsets_ptr + (3 * index + i))?)?
                    .as_ref())?;
            addresses.push(addr);
            let (_, value) = self.read_n_words_value(memory, addr)?;
            values.push(value)
        }
        let (a, b, c) = (&values[0], &values[1], &values[2]);
        match (a, b, c) {
            // Deduce c from a and b and write it to memory.
            (Some(a), Some(b), None) => {
                let value = apply_op(a, b, op)?.mod_floor(&inputs.p);
                self.write_n_words_value(memory, addresses[2], value)?;
                Ok(true)
            }
            // Deduce b from a and c and write it to memory.
            (Some(a), None, Some(c)) => {
                let value = apply_op(c, a, inv_op)?.mod_floor(&inputs.p);
                self.write_n_words_value(memory, addresses[1], value)?;
                Ok(true)
            }
            // Deduce a from b and c and write it to memory.
            (None, Some(b), Some(c)) => {
                let value = apply_op(c, b, inv_op)?.mod_floor(&inputs.p);
                self.write_n_words_value(memory, addresses[0], value)?;
                Ok(true)
            }
            // All values are already known.
            (Some(_), Some(_), Some(_)) => Ok(true),
            _ => Ok(false),
        }
    }

    /// NOTE: It is advisable to use VirtualMachine::mod_builtin_fill_memory instead of this method directly
    /// when implementing hints to avoid cloning the runners

    /// Fills the memory with inputs to the builtin instances based on the inputs to the
    /// first instance, pads the offsets table to fit the number of operations writen in the
    /// input to the first instance, and caculates missing values in the values table.

    /// For each builtin, the given tuple is of the form (builtin_ptr, builtin_runner, n),
    /// where n is the number of operations in the offsets table (i.e., the length of the
    /// offsets table is 3*n).

    /// The number of operations written to the input of the first instance n' should be at
    /// least n and a multiple of batch_size. Previous offsets are copied to the end of the
    /// offsets table to make its length 3n'.
    pub fn fill_memory(
        memory: &mut Memory,
        add_mod: Option<(Relocatable, &ModBuiltinRunner, usize)>,
        mul_mod: Option<(Relocatable, &ModBuiltinRunner, usize)>,
    ) -> Result<(), RunnerError> {
        if add_mod.is_none() && mul_mod.is_none() {
            return Err(RunnerError::FillMemoryNoBuiltinSet);
        }
        // Check that the instance definitions of the builtins are the same.
        if let (Some((_, add_mod, _)), Some((_, mul_mod, _))) = (add_mod, mul_mod) {
            if add_mod.instance_def.word_bit_len != mul_mod.instance_def.word_bit_len {
                return Err(RunnerError::ModBuiltinsMismatchedInstanceDef);
            }
        }
        // Fill the inputs to the builtins.
        let (add_mod_inputs, add_mod_n) =
            if let Some((add_mod_addr, add_mod, add_mod_index)) = add_mod {
                let add_mod_inputs = add_mod.read_inputs(memory, add_mod_addr)?;
                add_mod.fill_inputs(memory, add_mod_addr, &add_mod_inputs)?;
                add_mod.fill_offsets(
                    memory,
                    add_mod_inputs.offsets_ptr,
                    add_mod_index,
                    add_mod_inputs.n.saturating_sub(add_mod_index),
                )?;
                (add_mod_inputs, add_mod_index)
            } else {
                Default::default()
            };

        let (mul_mod_inputs, mul_mod_n) =
            if let Some((mul_mod_addr, mul_mod, mul_mod_index)) = mul_mod {
                let mul_mod_inputs = mul_mod.read_inputs(memory, mul_mod_addr)?;
                mul_mod.fill_inputs(memory, mul_mod_addr, &mul_mod_inputs)?;
                mul_mod.fill_offsets(
                    memory,
                    mul_mod_inputs.offsets_ptr,
                    mul_mod_index,
                    mul_mod_inputs.n.saturating_sub(mul_mod_index),
                )?;
                (mul_mod_inputs, mul_mod_index)
            } else {
                Default::default()
            };

        //  Get one of the builtin runners - the rest of this function doesn't depend on batch_size.
        let mod_runner = if let Some((_, add_mod, _)) = add_mod {
            add_mod
        } else {
            mul_mod.unwrap().1
        };
        // Fill the values table.
        let mut add_mod_index = 0;
        let mut mul_mod_index = 0;
        // Create operation here to avoid cloning p in the loop
        let div_operation = Operation::DivMod(mul_mod_inputs.p.clone());
        while add_mod_index < add_mod_n || mul_mod_index < mul_mod_n {
            if add_mod_index < add_mod_n
                && mod_runner.fill_value(
                    memory,
                    &add_mod_inputs,
                    add_mod_index,
                    &Operation::Add,
                    &Operation::Sub,
                )?
            {
                add_mod_index += 1;
            } else if mul_mod_index < mul_mod_n
                && mod_runner.fill_value(
                    memory,
                    &mul_mod_inputs,
                    mul_mod_index,
                    &Operation::Mul,
                    &div_operation,
                )?
            {
                mul_mod_index += 1;
            } else {
                return Err(RunnerError::FillMemoryCoudNotFillTable(
                    add_mod_index,
                    mul_mod_index,
                ));
            }
        }
        Ok(())
    }

    // Additional checks added to the standard builtin runner security checks
    pub(crate) fn run_additional_security_checks(
        &self,
        vm: &VirtualMachine,
    ) -> Result<(), VirtualMachineError> {
        let segment_size = vm
            .get_segment_used_size(self.base)
            .ok_or(MemoryError::MissingSegmentUsedSizes)?;
        let n_instances = div_ceil(segment_size, INPUT_CELLS);
        let mut prev_inputs = Inputs::default();
        for instance in 0..n_instances {
            let inputs = self.read_inputs(
                &vm.segments.memory,
                (self.base as isize, instance * INPUT_CELLS).into(),
            )?;
            if !instance.is_zero() && prev_inputs.n > self.instance_def.batch_size {
                for i in 0..N_WORDS {
                    if inputs.p_values[i] != prev_inputs.p_values[i] {
                        return Err(RunnerError::ModBuiltinSecurityCheck(self.name().to_string(), format!("inputs.p_values[i] != prev_inputs.p_values[i]. Got: i={}, inputs.p_values[i]={}, prev_inputs.p_values[i]={}",
                    i, inputs.p_values[i], prev_inputs.p_values[i])).into());
                    }
                }
                if inputs.values_ptr != prev_inputs.values_ptr {
                    return Err(RunnerError::ModBuiltinSecurityCheck(self.name().to_string(), format!("inputs.values_ptr != prev_inputs.values_ptr. Got: inputs.values_ptr={}, prev_inputs.values_ptr={}",
                inputs.values_ptr, prev_inputs.values_ptr)).into());
                }
                if inputs.offsets_ptr
                    != (prev_inputs.offsets_ptr + (3 * self.instance_def.batch_size))?
                {
                    return Err(RunnerError::ModBuiltinSecurityCheck(self.name().to_string(), format!("inputs.offsets_ptr != prev_inputs.offsets_ptr + 3 * batch_size. Got: inputs.offsets_ptr={}, prev_inputs.offsets_ptr={}, batch_size={}",
                inputs.values_ptr, prev_inputs.values_ptr, self.instance_def.batch_size)).into());
                }
                if inputs.n != prev_inputs.n.saturating_sub(self.instance_def.batch_size) {
                    return Err(RunnerError::ModBuiltinSecurityCheck(self.name().to_string(), format!("inputs.n != prev_inputs.n - batch_size. Got: inputs.n={}, prev_inputs.n={}, batch_size={}",
                inputs.n, prev_inputs.n, self.instance_def.batch_size)).into());
                }
            }
            for index_in_batch in 0..self.instance_def.batch_size {
                let (a, b, c) = self.read_memory_vars(
                    &vm.segments.memory,
                    inputs.values_ptr,
                    inputs.offsets_ptr,
                    index_in_batch,
                )?;
                let op = match self.builtin_type {
                    ModBuiltinType::Add => Operation::Add,
                    ModBuiltinType::Mul => Operation::Mul,
                };
                let a_op_b = apply_op(&a, &b, &op)?.mod_floor(&inputs.p);
                if a_op_b != c.mod_floor(&inputs.p) {
                    // Build error string
                    let p = inputs.p;
                    let error_string = format!("Expected a {op} b == c (mod p). Got: instance={instance}, batch={index_in_batch}, p={p}, a={a}, b={b}, c={c}.");
                    return Err(RunnerError::ModBuiltinSecurityCheck(
                        self.name().to_string(),
                        error_string,
                    )
                    .into());
                }
            }
            prev_inputs = inputs;
        }
        if !n_instances.is_zero() && prev_inputs.n != self.instance_def.batch_size {
            return Err(RunnerError::ModBuiltinSecurityCheck(
                self.name().to_string(),
                format!(
                    "prev_inputs.n != batch_size Got: prev_inputs.n={}, batch_size={}",
                    prev_inputs.n, self.instance_def.batch_size
                ),
            )
            .into());
        }
        Ok(())
    }
}

fn apply_op(lhs: &BigUint, rhs: &BigUint, op: &Operation) -> Result<BigUint, MathError> {
    Ok(match op {
        Operation::Mul => lhs * rhs,
        Operation::Add => lhs + rhs,
        Operation::Sub => lhs - rhs,
        Operation::DivMod(ref p) => div_mod_unsigned(lhs, rhs, p)?,
    })
}
