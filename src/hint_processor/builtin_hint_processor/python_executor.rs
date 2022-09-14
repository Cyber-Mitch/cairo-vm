use std::{any::Any, collections::HashMap, thread};

use crate::{
    bigint, bigint_str,
    hint_processor::{
        builtin_hint_processor::python_executor_helpers::get_value_from_reference,
        hint_processor_definition::HintReference, hint_processor_utils::bigint_to_usize,
    },
    serde::deserialize_program::ApTracking,
    types::relocatable::{MaybeRelocatable, Relocatable},
    vm::{errors::vm_errors::VirtualMachineError, vm_core::VirtualMachine},
};

use super::{
    builtin_hint_processor_definition::HintProcessorData,
    python_executor_helpers::{compute_addr_from_reference, write_py_vec_args},
};
use crossbeam_channel::{unbounded, Receiver, Sender};
use num_bigint::BigInt;
use pyo3::{prelude::*, types::PyDict};

#[derive(Debug)]
#[pyclass]
pub struct PyBigInt {
    value: String,
}

impl ToPyObject for PyBigInt {
    fn to_object(&self, py: Python<'_>) -> PyObject {
        let number_string = self.value.clone();
        let pystring = number_string.into_py(py);
        let locals = PyDict::new(py);
        locals.set_item("pystring", pystring).unwrap();
        let result = py.eval("int(pystring)", None, Some(locals)).unwrap();
        result.to_object(py)
    }
}

impl<'a> FromPyObject<'a> for PyBigInt {
    fn extract(ob: &'a PyAny) -> PyResult<Self> {
        let ob_as_string = ob.to_string();
        Ok(PyBigInt {
            value: ob_as_string,
        })
    }
}

impl From<PyBigInt> for BigInt {
    fn from(py_object: PyBigInt) -> Self {
        bigint_str!(py_object.value.as_bytes())
    }
}

impl From<&PyBigInt> for BigInt {
    fn from(py_object: &PyBigInt) -> Self {
        bigint_str!(py_object.value.as_bytes())
    }
}

impl From<BigInt> for PyBigInt {
    fn from(bi: BigInt) -> Self {
        PyBigInt {
            value: bi.to_string(),
        }
    }
}

impl From<&BigInt> for PyBigInt {
    fn from(bi: &BigInt) -> Self {
        PyBigInt {
            value: bi.to_string(),
        }
    }
}

#[derive(FromPyObject, Debug)]
pub enum PyMaybeRelocatable {
    Int(PyBigInt),
    RelocatableValue(PyRelocatable),
}

impl From<MaybeRelocatable> for PyMaybeRelocatable {
    fn from(val: MaybeRelocatable) -> Self {
        match val {
            MaybeRelocatable::RelocatableValue(rel) => PyMaybeRelocatable::RelocatableValue(
                PyRelocatable::new((rel.segment_index, rel.offset)),
            ),
            MaybeRelocatable::Int(num) => PyMaybeRelocatable::Int(PyBigInt::from(num)),
        }
    }
}

impl From<&MaybeRelocatable> for PyMaybeRelocatable {
    fn from(val: &MaybeRelocatable) -> Self {
        match val {
            MaybeRelocatable::RelocatableValue(rel) => PyMaybeRelocatable::RelocatableValue(
                PyRelocatable::new((rel.segment_index, rel.offset)),
            ),
            MaybeRelocatable::Int(num) => PyMaybeRelocatable::Int(PyBigInt::from(num)),
        }
    }
}

impl From<PyMaybeRelocatable> for MaybeRelocatable {
    fn from(val: PyMaybeRelocatable) -> Self {
        match val {
            PyMaybeRelocatable::RelocatableValue(rel) => {
                MaybeRelocatable::RelocatableValue(Relocatable::from((rel.index, rel.offset)))
            }
            PyMaybeRelocatable::Int(num) => MaybeRelocatable::Int(BigInt::from(num)),
        }
    }
}

impl From<&PyMaybeRelocatable> for MaybeRelocatable {
    fn from(val: &PyMaybeRelocatable) -> Self {
        match val {
            PyMaybeRelocatable::RelocatableValue(rel) => {
                MaybeRelocatable::RelocatableValue(Relocatable::from((rel.index, rel.offset)))
            }
            PyMaybeRelocatable::Int(num) => MaybeRelocatable::Int(BigInt::from(num)),
        }
    }
}

#[pyclass(name = "Relocatable")]
#[derive(Clone, Debug)]
pub struct PyRelocatable {
    index: usize,
    offset: usize,
}

#[pymethods]
impl PyRelocatable {
    #[new]
    pub fn new(tuple: (usize, usize)) -> PyRelocatable {
        PyRelocatable {
            index: tuple.0,
            offset: tuple.1,
        }
    }

    pub fn __add__(&self, value: usize) -> PyRelocatable {
        PyRelocatable {
            index: self.index,
            offset: self.offset + value,
        }
    }

    pub fn __sub__(&self, value: PyMaybeRelocatable, py: Python) -> PyResult<PyObject> {
        match value {
            PyMaybeRelocatable::Int(value) => {
                Ok(PyMaybeRelocatable::RelocatableValue(PyRelocatable {
                    index: self.index,
                    offset: self.offset - bigint_to_usize(&BigInt::from(value)).unwrap(),
                })
                .to_object(py))
            }
            PyMaybeRelocatable::RelocatableValue(address) => {
                if self.index == address.index && self.offset >= address.offset {
                    return Ok(PyMaybeRelocatable::Int(PyBigInt::from(bigint!(
                        self.offset - address.offset
                    )))
                    .to_object(py));
                }
                todo!()
            }
        }
    }

    pub fn __repr__(&self) -> String {
        format!("({}, {})", self.index, self.offset)
    }
}

impl PyRelocatable {
    pub fn to_relocatable(&self) -> Relocatable {
        Relocatable {
            segment_index: self.index,
            offset: self.offset,
        }
    }
}

impl ToPyObject for PyMaybeRelocatable {
    fn to_object(&self, py: Python<'_>) -> PyObject {
        match self {
            PyMaybeRelocatable::RelocatableValue(address) => address.clone().into_py(py),
            PyMaybeRelocatable::Int(value) => {
                let cloned_value = PyBigInt {
                    value: value.value.clone(),
                };
                cloned_value.into_py(py)
            }
        }
    }
}

#[derive(Debug)]
pub enum Operation {
    AddSegment,
    WriteMemory(PyRelocatable, PyMaybeRelocatable),
    ReadMemory(PyRelocatable),
    ReadIds(String),
    WriteIds(String, PyMaybeRelocatable),
    WriteVecArg(PyRelocatable, Vec<PyMaybeRelocatable>),
    End,
}

#[derive(Debug)]
pub enum OperationResult {
    Reading(PyMaybeRelocatable),
    Segment(PyRelocatable),
    Success,
}

#[pyclass]
pub struct PySegmentManager {
    operation_sender: Sender<Operation>,
    result_receiver: Receiver<OperationResult>,
}

#[pymethods]
impl PySegmentManager {
    pub fn add(&self) -> PyResult<PyRelocatable> {
        self.operation_sender.send(Operation::AddSegment).unwrap();
        if let OperationResult::Segment(result) = self.result_receiver.recv().unwrap() {
            return Ok(result);
        }
        todo!()
    }

    pub fn write_arg(&self, ptr: PyRelocatable, arg: Vec<PyMaybeRelocatable>) -> PyResult<()> {
        self.operation_sender
            .send(Operation::WriteVecArg(ptr, arg))
            .unwrap();
        if let OperationResult::Success = self.result_receiver.recv().unwrap() {
            return Ok(());
        }
        todo!()
    }
}

impl PySegmentManager {
    pub fn new(
        operation_sender: Sender<Operation>,
        result_receiver: Receiver<OperationResult>,
    ) -> PySegmentManager {
        PySegmentManager {
            operation_sender,
            result_receiver,
        }
    }
}

#[pyclass]
pub struct PyMemory {
    operation_sender: Sender<Operation>,
    result_receiver: Receiver<OperationResult>,
}

#[pymethods]
impl PyMemory {
    pub fn __getitem__(&self, key: &PyRelocatable, py: Python) -> PyResult<PyObject> {
        self.operation_sender
            .send(Operation::ReadMemory(PyRelocatable::new((
                key.index, key.offset,
            ))))
            .unwrap();
        if let OperationResult::Reading(result) = self.result_receiver.recv().unwrap() {
            return Ok(result.to_object(py));
        }
        todo!()
    }

    pub fn __setitem__(&self, key: &PyRelocatable, value: PyMaybeRelocatable) -> PyResult<()> {
        self.operation_sender
            .send(Operation::WriteMemory(
                PyRelocatable::new((key.index, key.offset)),
                value,
            ))
            .unwrap();
        self.result_receiver.recv().unwrap();
        Ok(())
    }
}

impl PyMemory {
    pub fn new(
        operation_sender: Sender<Operation>,
        result_receiver: Receiver<OperationResult>,
    ) -> PyMemory {
        PyMemory {
            operation_sender,
            result_receiver,
        }
    }
}

#[pyclass]
pub struct PyIds {
    operation_sender: Sender<Operation>,
    result_receiver: Receiver<OperationResult>,
}

#[pymethods]
impl PyIds {
    pub fn __getattr__(&self, name: &str, py: Python) -> PyResult<PyObject> {
        self.operation_sender
            .send(Operation::ReadIds(name.to_string()))
            .unwrap();
        if let OperationResult::Reading(result) = self.result_receiver.recv().unwrap() {
            return Ok(result.to_object(py));
        }
        todo!()
    }

    pub fn __setattr__(&self, name: &str, value: PyMaybeRelocatable) -> PyResult<()> {
        self.operation_sender
            .send(Operation::WriteIds(name.to_string(), value))
            .unwrap();
        if let OperationResult::Success = self.result_receiver.recv().unwrap() {
            return Ok(());
        }
        todo!()
    }
}

impl PyIds {
    pub fn new(
        operation_sender: Sender<Operation>,
        result_receiver: Receiver<OperationResult>,
    ) -> PyIds {
        PyIds {
            operation_sender,
            result_receiver,
        }
    }
}

fn handle_memory_messages(
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
    operation_receiver: Receiver<Operation>,
    result_sender: Sender<OperationResult>,
    vm: &mut VirtualMachine,
) {
    loop {
        match operation_receiver.recv().unwrap() {
            Operation::End => break,
            Operation::ReadMemory(address) => {
                if let Some(value) = vm.memory.get(&address.to_relocatable()).unwrap() {
                    result_sender
                        .send(OperationResult::Reading(Into::<PyMaybeRelocatable>::into(
                            value,
                        )))
                        .unwrap();
                };
            }
            Operation::WriteMemory(key, value) => {
                vm.memory
                    .insert(
                        &key.to_relocatable(),
                        &(Into::<MaybeRelocatable>::into(value)),
                    )
                    .unwrap();
                result_sender.send(OperationResult::Success).unwrap();
            }
            Operation::AddSegment => {
                let result = vm.segments.add(&mut vm.memory);
                result_sender
                    .send(OperationResult::Segment(PyRelocatable::new((
                        result.segment_index,
                        result.offset,
                    ))))
                    .unwrap()
            }
            Operation::ReadIds(name) => {
                let hint_ref = ids_data.get(&name).unwrap();
                let value = get_value_from_reference(vm, hint_ref, ap_tracking)
                    .unwrap()
                    .unwrap();
                result_sender
                    .send(OperationResult::Reading(value.into()))
                    .unwrap();
            }
            Operation::WriteIds(name, value) => {
                let hint_ref = ids_data.get(&name).unwrap();
                let addr =
                    compute_addr_from_reference(hint_ref, &vm.run_context, &vm.memory, ap_tracking)
                        .unwrap();
                vm.memory
                    .insert(&addr, &(Into::<MaybeRelocatable>::into(value)))
                    .unwrap();
                result_sender.send(OperationResult::Success).unwrap()
            }
            Operation::WriteVecArg(ptr, arg) => {
                write_py_vec_args(&mut vm.memory, &ptr, &arg, &vm.prime).unwrap();
                result_sender.send(OperationResult::Success).unwrap()
            }
        }
    }
}

pub struct PythonExecutor {}

impl PythonExecutor {
    pub fn execute_hint(
        vm: &mut VirtualMachine,
        hint_data: &Box<dyn Any>,
    ) -> Result<(), VirtualMachineError> {
        let hint_data = hint_data
            .downcast_ref::<HintProcessorData>()
            .ok_or(VirtualMachineError::WrongHintData)?;
        let code = hint_data.code.clone();

        let (operation_sender, operation_receiver) = unbounded();
        let (result_sender, result_receiver) = unbounded();
        let ap = vm.run_context.ap;
        let fp = vm.run_context.fp;
        pyo3::prepare_freethreaded_python();
        let gil = Python::acquire_gil();
        let py = gil.python();
        py.allow_threads(move || {
            thread::spawn(move || {
                println!(" -- Starting python hint execution -- ");
                let gil = Python::acquire_gil();
                let py = gil.python();
                let memory = PyCell::new(
                    py,
                    PyMemory::new(operation_sender.clone(), result_receiver.clone()),
                )
                .unwrap();
                let segments = PyCell::new(
                    py,
                    PySegmentManager::new(operation_sender.clone(), result_receiver.clone()),
                )
                .unwrap();
                let ids =
                    PyCell::new(py, PyIds::new(operation_sender.clone(), result_receiver)).unwrap();
                let ap = PyCell::new(py, PyRelocatable::new((1, ap))).unwrap();
                let fp = PyCell::new(py, PyRelocatable::new((1, fp))).unwrap();
                let locals = PyDict::new(py);
                locals.set_item("memory", memory).unwrap();
                locals.set_item("segments", segments).unwrap();
                locals.set_item("ap", ap).unwrap();
                locals.set_item("fp", fp).unwrap();
                locals.set_item("ids", ids).unwrap();
                py.run(&code, None, Some(locals)).unwrap();
                println!(" -- Ending python hint -- ");
                operation_sender.send(Operation::End).unwrap();
            });
            handle_memory_messages(
                &hint_data.ids_data,
                &hint_data.ap_tracking,
                operation_receiver,
                result_sender,
                vm,
            );
        });
        Ok(())
    }
}
