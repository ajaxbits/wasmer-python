use crate::{
    errors::to_py_err,
    externals::{Function, Global, Memory, Table},
    wasmer_inner::wasmer,
};
use pyo3::{
    exceptions::PyTypeError,
    prelude::*,
    types::{PyDict, PyString},
};

/// An `ImportObject` represents all of the import data used when
/// instantiating a WebAssembly module.
///
/// # Important
///
/// This object is deprecated in favor of dictionaries in Python.
/// You can now type:
///
/// ```py
/// from wasmer import Store, Function
///
/// def sum(x: int, y: int) -> int:
///     return x + y
///
/// store = Store()
/// import_object = {}
/// import_object["math"] = {
///     "sum": Function(store, sum)
/// }
/// ```
///
/// ## Example
///
/// Importing a function, `math.sum`, and call it through the exported
/// `add_one` function:
///
/// ```py
/// from wasmer import Store, Module, Instance, ImportObject, Function
/// def sum(x: int, y: int) -> int:
///     return x + y
///
/// store = Store()
/// module = Module(
///     store,
///     """
///     (module
///       (import "math" "sum" (func $sum (param i32 i32) (result i32)))
///       (func (export "add_one") (param i32) (result i32)
///         local.get 0
///         i32.const 1
///         call $sum))
///     """
/// )
///
/// import_object = ImportObject()
/// import_object.register(
///     "math",
///     {
///         "sum": Function(store, sum)
///     }
/// )
///
/// instance = Instance(module, import_object)
///
/// assert instance.exports.add_one(1) == 2
/// ```
///
/// Importing a memory:
///
/// ```py
/// from wasmer import Store, Module, Instance, Memory, MemoryType, ImportObject
///
/// store = Store()
/// module = Module(
///     store,
///     """
///     (module
///       (import "env" "memory" (memory $memory 1))
///       (func (export "increment")
///         i32.const 0
///         i32.const 0
///         i32.load    ;; load 0
///         i32.const 1
///         i32.add     ;; add 1
///         i32.store   ;; store at 0
///         ))
///     """
/// )
///
/// memory = Memory(store, MemoryType(minimum=1))
/// view = memory.uint8_view(offset=0)
///
/// import_object = ImportObject()
/// import_object.register(
///     "env",
///     {
///         "memory": memory
///     }
/// )
///
/// instance = Instance(module, import_object)
///
/// assert view[0] == 0
/// instance.exports.increment()
/// assert view[0] == 1
/// instance.exports.increment()
/// assert view[0] == 2
/// ```
///
/// Importing a global:
///
/// ```py
/// from wasmer import Store, Module, Instance, ImportObject, Global, Value
///
/// store = Store()
/// module = Module(
///     store,
///     """
///     (module
///       (import "env" "global" (global $global (mut i32)))
///       (func (export "read_g") (result i32)
///         global.get $global)
///       (func (export "write_g") (param i32)
///         local.get 0
///         global.set $global))
///     """
/// )
///
/// global_ = Global(store, Value.i32(7), mutable=True)
///
/// import_object = ImportObject()
/// import_object.register(
///     "env",
///     {
///         "global": global_
///     }
/// )
///
/// instance = Instance(module, import_object)
///
/// assert instance.exports.read_g() == 7
/// global_.value = 153
/// assert instance.exports.read_g() == 153
/// instance.exports.write_g(11)
/// assert global_.value == 11
/// ```
///
/// etc.
#[pyclass(unsendable)]
#[pyo3(text_signature = "()")]
pub struct ImportObject {
    inner: wasmer::ImportObject,
}

impl ImportObject {
    pub(crate) fn raw_new(inner: wasmer::ImportObject) -> Self {
        Self { inner }
    }

    pub(crate) fn inner(&self) -> &wasmer::ImportObject {
        &self.inner
    }
}

impl ImportObject {
    /// Gets an `ImportObject` from a Python dictionary.
    pub(crate) fn from_pydict(dict: &PyDict) -> Result<Self, PyErr> {
        let mut io = ImportObject::new();
        for (namespace_name, namespace_dict) in dict.into_iter() {
            let namespace_name = namespace_name.to_string();
            let namespace_dict = namespace_dict
                .downcast::<PyDict>()?;
            io.register(&namespace_name, namespace_dict)?;
        }
        Ok(io)
    }
}

#[pymethods]
impl ImportObject {
    #[new]
    pub(crate) fn new() -> Self {
        ImportObject::raw_new(Default::default())
    }

    /// Checks whether the import object contains a specific namespace.
    ///
    /// ## Example
    ///
    /// ```py
    /// from wasmer import ImportObject
    ///
    /// import_object = ImportObject()
    ///
    /// assert import_object.contains_namespace("foo") == False
    /// ```
    #[pyo3(text_signature = "($self, namespace_name)")]
    fn contains_namespace(&self, namespace_name: &str) -> bool {
        self.inner.contains_namespace(namespace_name)
    }

    /// Gets an `ImportObject` from a Python dictionary.
    #[pyo3(text_signature = "($self)")]
    pub(crate) fn to_dict<'py>(&'py self) -> Result<PyObject, PyErr> {
        let gil_guard = Python::acquire_gil();
        let py = gil_guard.python();

        let dict = PyDict::new(py);
        for (namespace, name, export) in self.inner.externs_vec() {
            let elem = match export {
                wasmer::Extern::Function(function) => {
                    Py::new(py, Function::raw_new(function.clone()))?.to_object(py)
                }
                wasmer::Extern::Global(global) => {
                    Py::new(py, Global::raw_new(global.clone()))?.to_object(py)
                }
                wasmer::Extern::Memory(memory) => {
                    Py::new(py, Memory::raw_new(memory.clone()))?.to_object(py)
                }
                wasmer::Extern::Table(table) => {
                    Py::new(py, Table::raw_new(table.clone()))?.to_object(py)
                }
            };
            let namespace_dict = match dict.get_item(&namespace) {
                Some(namespace_dict) => {
                    namespace_dict
                },
                None => {
                    let namespace_dict = PyDict::new(py);
                    dict.set_item(&namespace, namespace_dict)?;
                    namespace_dict
                }
            };
            namespace_dict.set_item(&name, elem)?;
        }
        Ok(dict.to_object(py))
    }

    /// Registers a set of `Function`, `Memory`, `Global` or `Table`
    /// to a particular namespace.
    ///
    /// ## Example
    ///
    /// ```py
    /// from wasmer import Store, ImportObject, Function, Memory, MemoryType
    ///
    /// store = Store()
    ///
    /// def sum(x: int, y: int) -> int:
    ///     return x + y
    ///
    /// import_object = ImportObject()
    /// import_object.register(
    ///     "env",
    ///     {
    ///         "sum": Function(store, sum),
    ///         "memory": Memory(store, MemoryType(minimum=1))
    ///     }
    /// )
    /// ```
    #[pyo3(text_signature = "($self, namespace_name, namespace)")]
    pub(crate) fn register(&mut self, namespace_name: &str, namespace: &PyDict) -> PyResult<()> {
        let mut wasmer_namespace = wasmer::Exports::new();

        for (name, item) in namespace.into_iter() {
            let name = String::from(name.downcast::<PyString>().map_err(PyErr::from)?.to_str()?);

            if let Ok(function) = item.downcast::<PyCell<Function>>() {
                let function = function.borrow();

                wasmer_namespace.insert(name, function.inner().clone());
            } else if let Ok(memory) = item.downcast::<PyCell<Memory>>() {
                let memory = memory.borrow();

                wasmer_namespace.insert(name, memory.inner().clone());
            } else if let Ok(global) = item.downcast::<PyCell<Global>>() {
                let global = global.borrow();

                wasmer_namespace.insert(name, global.inner().clone());
            } else if let Ok(table) = item.downcast::<PyCell<Table>>() {
                let table = table.borrow();

                wasmer_namespace.insert(name, table.inner().clone());
            } else {
                return Err(to_py_err::<PyTypeError, _>(format!(
                    "`ImportObject` cannot register the given type `{:?}`",
                    item.get_type().name()
                )));
            }
        }

        self.inner.register(namespace_name, wasmer_namespace);

        Ok(())
    }
}
