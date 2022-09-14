use std::{
    any::Any,
    collections::HashMap,
    thread::{self, JoinHandle},
};

use crate::{
    any_box, bigint,
    hint_processor::{
        builtin_hint_processor::python_executor_helpers::get_value_from_reference,
        hint_processor_definition::HintReference, hint_processor_utils::bigint_to_usize,
    },
    pycell,
    serde::deserialize_program::ApTracking,
    types::{
        exec_scope::ExecutionScopes,
        relocatable::{MaybeRelocatable, Relocatable},
    },
    vm::{errors::vm_errors::VirtualMachineError, vm_core::VirtualMachine},
};

use super::{
    builtin_hint_processor_definition::HintProcessorData,
    python_executor_helpers::{compute_addr_from_reference, write_py_vec_args},
};
use crossbeam_channel::{unbounded, Receiver, Sender};
use num_bigint::BigInt;
use pyo3::{exceptions::PyTypeError, prelude::*, types::PyDict};

const CHANNEL_ERROR_MSG: &str = "Failed to communicate between channels";

#[derive(FromPyObject, Debug)]
pub enum PyMaybeRelocatable {
    Int(BigInt),
    RelocatableValue(PyRelocatable),
}

impl From<MaybeRelocatable> for PyMaybeRelocatable {
    fn from(val: MaybeRelocatable) -> Self {
        match val {
            MaybeRelocatable::RelocatableValue(rel) => PyMaybeRelocatable::RelocatableValue(
                PyRelocatable::new((rel.segment_index, rel.offset)),
            ),
            MaybeRelocatable::Int(num) => PyMaybeRelocatable::Int(num),
        }
    }
}

impl From<&MaybeRelocatable> for PyMaybeRelocatable {
    fn from(val: &MaybeRelocatable) -> Self {
        match val {
            MaybeRelocatable::RelocatableValue(rel) => PyMaybeRelocatable::RelocatableValue(
                PyRelocatable::new((rel.segment_index, rel.offset)),
            ),
            MaybeRelocatable::Int(num) => PyMaybeRelocatable::Int(num.clone()),
        }
    }
}

impl From<PyMaybeRelocatable> for MaybeRelocatable {
    fn from(val: PyMaybeRelocatable) -> Self {
        match val {
            PyMaybeRelocatable::RelocatableValue(rel) => {
                MaybeRelocatable::RelocatableValue(Relocatable::from((rel.index, rel.offset)))
            }
            PyMaybeRelocatable::Int(num) => MaybeRelocatable::Int(num),
        }
    }
}

impl From<&PyMaybeRelocatable> for MaybeRelocatable {
    fn from(val: &PyMaybeRelocatable) -> Self {
        match val {
            PyMaybeRelocatable::RelocatableValue(rel) => {
                MaybeRelocatable::RelocatableValue(Relocatable::from((rel.index, rel.offset)))
            }
            PyMaybeRelocatable::Int(num) => MaybeRelocatable::Int(num.clone()),
        }
    }
}

impl From<Relocatable> for PyRelocatable {
    fn from(val: Relocatable) -> Self {
        PyRelocatable::new((val.segment_index, val.offset))
    }
}

impl From<Relocatable> for PyMaybeRelocatable {
    fn from(val: Relocatable) -> Self {
        PyMaybeRelocatable::RelocatableValue(val.into())
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
                let result = bigint_to_usize(&value);
                if let Ok(value) = result {
                    if value <= self.offset {
                        return Ok(PyMaybeRelocatable::RelocatableValue(PyRelocatable {
                            index: self.index,
                            offset: self.offset - value,
                        })
                        .to_object(py));
                    };
                }
                Err(PyTypeError::new_err(
                    "MaybeRelocatable substraction failure: Offset exceeded",
                ))
            }
            PyMaybeRelocatable::RelocatableValue(address) => {
                if self.index == address.index && self.offset >= address.offset {
                    return Ok(
                        PyMaybeRelocatable::Int(bigint!(self.offset - address.offset))
                            .to_object(py),
                    );
                }
                Err(PyTypeError::new_err(
                    "Cant sub two Relocatables of different segments",
                ))
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
            PyMaybeRelocatable::Int(value) => value.clone().into_py(py),
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
    ReadValue(PyMaybeRelocatable),
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
        send_operation(&self.operation_sender, Operation::AddSegment)?;
        if let OperationResult::Segment(result) = self
            .result_receiver
            .recv()
            .map_err(|_| PyTypeError::new_err(CHANNEL_ERROR_MSG))?
        {
            return Ok(result);
        }
        Err(PyTypeError::new_err("segments.add() failure"))
    }
    pub fn write_arg(&self, ptr: PyRelocatable, arg: Vec<PyMaybeRelocatable>) -> PyResult<()> {
        send_operation(&self.operation_sender, Operation::WriteVecArg(ptr, arg))?;
        check_operation_success(&self.result_receiver, "segments.write_arg()")
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
        send_operation(
            &self.operation_sender,
            Operation::ReadMemory(PyRelocatable::new((key.index, key.offset))),
        )?;
        get_read_value_result(&self.result_receiver, "memory.__getitem__()", &py)
    }

    pub fn __setitem__(&self, key: &PyRelocatable, value: PyMaybeRelocatable) -> PyResult<()> {
        send_operation(
            &self.operation_sender,
            Operation::WriteMemory(PyRelocatable::new((key.index, key.offset)), value),
        )?;
        check_operation_success(&self.result_receiver, "memory.__setitem__()")
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
        send_operation(&self.operation_sender, Operation::ReadIds(name.to_string()))?;
        get_read_value_result(&self.result_receiver, "ids.__getattr__()", &py)
    }

    pub fn __setattr__(&self, name: &str, value: PyMaybeRelocatable) -> PyResult<()> {
        send_operation(
            &self.operation_sender,
            Operation::WriteIds(name.to_string(), value),
        )?;
        check_operation_success(&self.result_receiver, "ids.__setattr__()")
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

fn handle_messages(
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
    operation_receiver: Receiver<Operation>,
    result_sender: Sender<OperationResult>,
    vm: &mut VirtualMachine,
) -> Result<(), VirtualMachineError> {
    loop {
        match operation_receiver
            .recv()
            .map_err(|_| VirtualMachineError::PythonExecutorChannel)?
        {
            Operation::End => break,
            Operation::ReadMemory(address) => {
                if let Some(value) = vm.memory.get(&address.to_relocatable())? {
                    send_result(&result_sender, OperationResult::ReadValue(value.into()))?;
                };
            }
            Operation::WriteMemory(key, value) => {
                vm.memory.insert(
                    &key.to_relocatable(),
                    &(Into::<MaybeRelocatable>::into(value)),
                )?;
                send_result(&result_sender, OperationResult::Success)?;
            }
            Operation::AddSegment => {
                let result = vm.segments.add(&mut vm.memory);
                send_result(&result_sender, OperationResult::Segment(result.into()))?;
            }
            Operation::ReadIds(name) => {
                let hint_ref = ids_data
                    .get(&name)
                    .ok_or(VirtualMachineError::FailedToGetIds)?;
                let value = get_value_from_reference(vm, hint_ref, ap_tracking)?;
                send_result(&result_sender, OperationResult::ReadValue(value.into()))?;
            }
            Operation::WriteIds(name, value) => {
                let hint_ref = ids_data
                    .get(&name)
                    .ok_or(VirtualMachineError::FailedToGetIds)?;
                let addr = compute_addr_from_reference(
                    hint_ref,
                    &vm.run_context,
                    &vm.memory,
                    ap_tracking,
                )?;
                vm.memory
                    .insert(&addr, &(Into::<MaybeRelocatable>::into(value)))?;
                send_result(&result_sender, OperationResult::Success)?;
            }
            Operation::WriteVecArg(ptr, arg) => {
                write_py_vec_args(&mut vm.memory, &ptr, &arg, &vm.prime)?;
                send_result(&result_sender, OperationResult::Success)?;
            }
        }
    }
    Ok(())
}

pub enum ScopeOperation {
    NoOperation,
    Exit,
    Enter(Vec<PyObject>),
    AddValues(Vec<PyObject>),
}

pub struct PythonExecutor {}

impl PythonExecutor {
    pub fn execute_hint(
        vm: &mut VirtualMachine,
        hint_data: &Box<dyn Any>,
        exec_scopes: &mut ExecutionScopes,
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
        let new_vars = py.allow_threads(move || -> JoinHandle<Result<HashMap<std::string::String, Py<PyAny>>, VirtualMachineError>>{
            let new_vars = thread::spawn(move || -> Result<HashMap::<String, PyObject>, VirtualMachineError> {
                println!(" -- Starting python hint execution -- ");
                let gil = Python::acquire_gil();
                let py = gil.python();
                let memory = pycell!(
                    py,
                    PyMemory::new(operation_sender.clone(), result_receiver.clone())
                );
                let segments = pycell!(
                    py,
                    PySegmentManager::new(operation_sender.clone(), result_receiver.clone())
                );
                let ids = pycell!(py, PyIds::new(operation_sender.clone(), result_receiver));
                let ap = pycell!(py, PyRelocatable::new((1, ap)));
                let fp = pycell!(py, PyRelocatable::new((1, fp)));
                let globals = PyDict::new(py);
                let locals = PyDict::new(py);
                globals.set_item("memory", memory).unwrap();
                globals.set_item("ids", ids).unwrap();
                globals.set_item("segments", segments).unwrap();
                globals.set_item("ap", ap).unwrap();
                globals.set_item("fp", fp).unwrap();
                py.run(&code, Some(globals), Some(locals)).unwrap();
                println!(" -- Ending python hint -- ");
                operation_sender
                    .send(Operation::End)
                    .map_err(|_| VirtualMachineError::PythonExecutorChannel)?;
                Ok(get_scope_variables(locals, py))
            });
            handle_messages(
                &hint_data.ids_data,
                &hint_data.ap_tracking,
                operation_receiver,
                result_sender,
                vm,
            ).unwrap();
            new_vars
        }).join().unwrap().unwrap();
        update_scope(exec_scopes, &new_vars);
        Ok(())
    }
}

fn update_scope(
    exec_scopes: &mut ExecutionScopes,
    new_scope_variables: &HashMap<String, PyObject>,
) {
    for (name, pyobj) in new_scope_variables {
        exec_scopes.assign_or_update_variable(name, any_box!(pyobj.clone()))
    }
}

fn get_scope_variables(locals: &PyDict, py: Python) -> HashMap<String, PyObject> {
    let mut new_locals = HashMap::<String, PyObject>::new();
    for (name, pyvalue) in locals.iter() {
        new_locals.insert(name.to_string(), pyvalue.to_object(py));
    }
    new_locals
}

fn send_result(
    sender: &Sender<OperationResult>,
    result: OperationResult,
) -> Result<(), VirtualMachineError> {
    sender
        .send(result)
        .map_err(|_| VirtualMachineError::PythonExecutorChannel)
}

fn send_operation(sender: &Sender<Operation>, operation: Operation) -> Result<(), PyErr> {
    sender
        .send(operation)
        .map_err(|_| PyTypeError::new_err(CHANNEL_ERROR_MSG))
}

fn check_operation_success(
    receiver: &Receiver<OperationResult>,
    method_name: &str,
) -> Result<(), PyErr> {
    if let OperationResult::Success = receiver
        .recv()
        .map_err(|_| PyTypeError::new_err(CHANNEL_ERROR_MSG))?
    {
        return Ok(());
    }
    let string = format!("{} failure", method_name);
    Err(PyTypeError::new_err(string))
}

fn get_read_value_result(
    receiver: &Receiver<OperationResult>,
    method_name: &str,
    py: &Python,
) -> PyResult<PyObject> {
    if let OperationResult::ReadValue(result) = receiver
        .recv()
        .map_err(|_| PyTypeError::new_err(CHANNEL_ERROR_MSG))?
    {
        return Ok(result.to_object(*py));
    }
    let string = format!("{} failure", method_name);
    Err(PyTypeError::new_err(string))
}
