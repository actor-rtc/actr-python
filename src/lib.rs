#![allow(unsafe_op_in_unsafe_fn)]

use std::any::Any;
use std::sync::Arc;
use std::sync::OnceLock;

use actr_config::ObservabilityConfig;
use actr_framework::{Bytes, Context as FrameworkContext, Dest, Workload};
use actr_protocol::prost::Message as ProstMessage;
use actr_protocol::{
    ActorResult, ActrError, ActrId, ActrIdExt, PayloadType as RpPayloadType, ProtocolError,
    RpcEnvelope,
};
use actr_runtime::context::RuntimeContext;
use actr_runtime::init_observability;
use actr_runtime::prelude::{ActrNode, ActrRef, ActrSystem};

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyRuntimeError, PyStopAsyncIteration, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};
use tokio::sync::mpsc;

create_exception!(actr_raw, ActrRuntimeError, PyException);
create_exception!(actr_raw, ActrTransportError, ActrRuntimeError);
create_exception!(actr_raw, ActrDecodeError, ActrRuntimeError);
create_exception!(actr_raw, ActrUnknownRoute, ActrRuntimeError);
create_exception!(actr_raw, ActrGateNotInitialized, ActrRuntimeError);

// Global guard to keep observability initialized
static OBSERVABILITY_GUARD: OnceLock<actr_runtime::ObservabilityGuard> = OnceLock::new();

fn ensure_observability_initialized() {
    OBSERVABILITY_GUARD.get_or_init(|| {
        let filter_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
        init_observability(&ObservabilityConfig {
            filter_level: filter_level.clone(),
            tracing_enabled: false,
            tracing_endpoint: String::new(),
            tracing_service_name: "actr-runtime-py".to_string(),
        })
        .unwrap_or_else(|e| {
            eprintln!("[warn] Failed to initialize observability: {}", e);
            actr_runtime::ObservabilityGuard::default()
        })
    });
}

fn map_protocol_error(err: ProtocolError) -> PyErr {
    match err {
        ProtocolError::TransportError(msg) => ActrTransportError::new_err(msg),
        ProtocolError::DecodeError(msg)
        | ProtocolError::DeserializationError(msg)
        | ProtocolError::EncodeError(msg) => ActrDecodeError::new_err(msg),
        ProtocolError::UnknownRoute(msg) => ActrUnknownRoute::new_err(msg),
        ProtocolError::Actr(ActrError::UnknownRoute { route_key }) => {
            ActrUnknownRoute::new_err(route_key)
        }
        ProtocolError::Actr(ActrError::GateNotInitialized { message }) => {
            ActrGateNotInitialized::new_err(message)
        }
        other => ActrRuntimeError::new_err(other.to_string()),
    }
}

/// Convert Python ActrId protobuf object to Rust ActrId
///
/// # Note
/// We use `Py<PyAny>` instead of a specific type because protobuf-generated Python classes
/// are not PyO3 `#[pyclass]` types. PyO3 can only recognize types defined with `#[pyclass]`
/// or basic Python types. We validate the type at runtime by calling `SerializeToString()`.
fn python_actr_id_to_rust(_py: Python, py_actr_id: Py<PyAny>) -> PyResult<ActrId> {
    Python::attach(|py| -> PyResult<ActrId> {
        let py_any = py_actr_id.bind(py);
        // Use SerializeToString() method from Python protobuf object, then decode
        let bytes = py_any
            .call_method0("SerializeToString")?
            .extract::<Vec<u8>>()
            .map_err(|e| PyValueError::new_err(format!("Failed to serialize ActrId: {e}")))?;

        ActrId::decode(&bytes[..])
            .map_err(|e| PyValueError::new_err(format!("Failed to decode ActrId: {e}")))
    })
}

/// Convert Rust ActrId to Python ActrId protobuf object
fn rust_actr_id_to_python(py: Python, actr_id: &ActrId) -> PyResult<Py<PyAny>> {
    // Serialize Rust ActrId to bytes, then use Python's FromString to create object
    let bytes = actr_id.encode_to_vec();

    // Import the Python protobuf module
    let actr_module = py.import("generated.actr_pb2")?;
    let actr_id_class = actr_module.getattr("ActrId")?;

    // Use FromString method to create Python ActrId object from bytes
    let py_bytes = PyBytes::new(py, &bytes);
    let py_actr_id = actr_id_class
        .call_method1("FromString", (py_bytes,))?
        .extract::<Py<PyAny>>()?;

    Ok(py_actr_id)
}

/// Convert Python ActrType protobuf object to Rust ActrType
fn python_actr_type_to_rust(
    _py: Python,
    py_actr_type: Py<PyAny>,
) -> PyResult<actr_protocol::ActrType> {
    Python::attach(|py| -> PyResult<actr_protocol::ActrType> {
        let py_any = py_actr_type.bind(py);
        let bytes = py_any
            .call_method0("SerializeToString")?
            .extract::<Vec<u8>>()
            .map_err(|e| PyValueError::new_err(format!("Failed to serialize ActrType: {e}")))?;

        actr_protocol::ActrType::decode(&bytes[..])
            .map_err(|e| PyValueError::new_err(format!("Failed to decode ActrType: {e}")))
    })
}

/// Convert Rust ActrType to Python ActrType protobuf object
#[allow(dead_code)] // May be used in future
fn rust_actr_type_to_python(
    py: Python,
    actr_type: &actr_protocol::ActrType,
) -> PyResult<Py<PyAny>> {
    let bytes = actr_type.encode_to_vec();
    let actr_module = py.import("generated.actr_pb2")?;
    let actr_type_class = actr_module.getattr("ActrType")?;
    let py_bytes = PyBytes::new(py, &bytes);
    let py_actr_type = actr_type_class
        .call_method1("FromString", (py_bytes,))?
        .extract::<Py<PyAny>>()?;
    Ok(py_actr_type)
}

/// Convert Python DataStream protobuf object to Rust DataStream
///
/// # Note
/// We use `Py<PyAny>` instead of a specific type because protobuf-generated Python classes
/// are not PyO3 `#[pyclass]` types. PyO3 can only recognize types defined with `#[pyclass]`
/// or basic Python types. We validate the type at runtime by calling `SerializeToString()`.
fn python_datastream_to_rust(_py: Python, py_ds: Py<PyAny>) -> PyResult<actr_protocol::DataStream> {
    Python::attach(|py| -> PyResult<actr_protocol::DataStream> {
        let py_any = py_ds.bind(py);
        // Use SerializeToString() method from Python protobuf object, then decode
        // This is the most reliable way to convert Python protobuf to Rust protobuf
        let bytes = py_any
            .call_method0("SerializeToString")?
            .extract::<Vec<u8>>()
            .map_err(|e| PyValueError::new_err(format!("Failed to serialize DataStream: {e}")))?;

        actr_protocol::DataStream::decode(&bytes[..])
            .map_err(|e| PyValueError::new_err(format!("Failed to decode DataStream: {e}")))
    })
}

/// Convert Rust DataStream to Python DataStream protobuf object
fn rust_datastream_to_python(py: Python, ds: &actr_protocol::DataStream) -> PyResult<Py<PyAny>> {
    // Serialize Rust DataStream to bytes, then use Python's FromString to create object
    let bytes = ds.encode_to_vec();

    // Import the Python protobuf module
    let package_module = py.import("generated.package_pb2")?;
    let datastream_class = package_module.getattr("DataStream")?;

    // Use FromString method to create Python DataStream object from bytes
    let py_bytes = PyBytes::new(py, &bytes);
    let py_ds = datastream_class
        .call_method1("FromString", (py_bytes,))?
        .extract::<Py<PyAny>>()?;

    Ok(py_ds)
}

/// Python wrapper for ActrId protobuf message
#[pyclass(name = "ActrId")]
#[derive(Clone)]
pub struct ActrIdPy {
    inner: ActrId,
}

#[pymethods]
impl ActrIdPy {
    /// Create ActrId from Python protobuf object
    #[new]
    fn new(py_actr_id: Py<PyAny>) -> PyResult<Self> {
        Python::attach(|py| -> PyResult<Self> {
            let inner = python_actr_id_to_rust(py, py_actr_id)?;
            Ok(ActrIdPy { inner })
        })
    }

    /// Create ActrId from bytes (protobuf serialized)
    #[staticmethod]
    fn from_bytes(bytes: &[u8]) -> PyResult<Self> {
        let inner = ActrId::decode(bytes)
            .map_err(|e| PyValueError::new_err(format!("Failed to decode ActrId: {e}")))?;
        Ok(ActrIdPy { inner })
    }

    /// Convert to Python protobuf object
    fn to_protobuf<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        rust_actr_id_to_python(py, &self.inner)
    }

    /// Serialize to bytes
    fn to_bytes(&self) -> Vec<u8> {
        self.inner.encode_to_vec()
    }

    /// Get string representation
    fn to_string_repr(&self) -> String {
        self.inner.to_string_repr()
    }
}

impl ActrIdPy {
    pub fn inner(&self) -> &ActrId {
        &self.inner
    }

    pub fn from_rust(actr_id: ActrId) -> Self {
        ActrIdPy { inner: actr_id }
    }
}

/// Python wrapper for ActrType protobuf message
#[pyclass(name = "ActrType")]
#[derive(Clone)]
pub struct ActrTypePy {
    inner: actr_protocol::ActrType,
}

#[pymethods]
impl ActrTypePy {
    /// Create ActrType from Python protobuf object
    #[new]
    fn new(py_actr_type: Py<PyAny>) -> PyResult<Self> {
        Python::attach(|py| -> PyResult<Self> {
            let inner = python_actr_type_to_rust(py, py_actr_type)?;
            Ok(ActrTypePy { inner })
        })
    }

    /// Create ActrType from bytes (protobuf serialized)
    #[staticmethod]
    fn from_bytes(bytes: &[u8]) -> PyResult<Self> {
        let inner = actr_protocol::ActrType::decode(bytes)
            .map_err(|e| PyValueError::new_err(format!("Failed to decode ActrType: {e}")))?;
        Ok(ActrTypePy { inner })
    }

    /// Convert to Python protobuf object
    fn to_protobuf<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        rust_actr_type_to_python(py, &self.inner)
    }

    /// Serialize to bytes
    fn to_bytes(&self) -> Vec<u8> {
        self.inner.encode_to_vec()
    }

    /// Get manufacturer
    fn manufacturer(&self) -> String {
        self.inner.manufacturer.clone()
    }

    /// Get name
    fn name(&self) -> String {
        self.inner.name.clone()
    }
}

impl ActrTypePy {
    pub fn inner(&self) -> &actr_protocol::ActrType {
        &self.inner
    }

    pub fn from_rust(actr_type: actr_protocol::ActrType) -> Self {
        ActrTypePy { inner: actr_type }
    }
}

/// Python wrapper for Dest (destination identifier)
#[pyclass(name = "Dest")]
#[derive(Clone)]
pub struct DestPy {
    inner: Dest,
}

#[pymethods]
impl DestPy {
    /// Create Shell destination (Workload → App)
    #[staticmethod]
    fn shell() -> Self {
        DestPy { inner: Dest::Shell }
    }

    /// Create Local destination (call local Workload)
    #[staticmethod]
    fn local() -> Self {
        DestPy { inner: Dest::Local }
    }

    /// Create Actor destination (call remote Actor)
    ///
    /// # Parameters
    /// - `actr_id`: Python `ActrId` protobuf object (from `generated.actr_pb2.ActrId`)
    #[staticmethod]
    fn actor<'py>(py: Python<'py>, actr_id: Py<PyAny>) -> PyResult<Self> {
        let rust_actr_id = python_actr_id_to_rust(py, actr_id)?;
        Ok(DestPy {
            inner: Dest::Actor(rust_actr_id),
        })
    }

    /// Check if this is a Shell destination
    fn is_shell(&self) -> bool {
        self.inner.is_shell()
    }

    /// Check if this is a Local destination
    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    /// Check if this is an Actor destination
    fn is_actor(&self) -> bool {
        self.inner.is_actor()
    }

    /// Get ActrId if this is an Actor destination, returns None otherwise
    ///
    /// # Returns
    /// Python `ActrId` protobuf object (from `generated.actr_pb2.ActrId`) or None
    fn as_actor_id<'py>(&self, py: Python<'py>) -> PyResult<Option<Py<PyAny>>> {
        if let Some(id) = self.inner.as_actor_id() {
            Ok(Some(rust_actr_id_to_python(py, id)?))
        } else {
            Ok(None)
        }
    }
}

impl DestPy {
    pub fn inner(&self) -> &Dest {
        &self.inner
    }

    pub fn from_rust(dest: Dest) -> Self {
        DestPy { inner: dest }
    }
}

/// Python wrapper for DataStream protobuf message
#[pyclass(name = "DataStream")]
#[derive(Clone)]
pub struct DataStreamPy {
    inner: actr_protocol::DataStream,
}

#[pymethods]
impl DataStreamPy {
    /// Create DataStream from Python protobuf object
    #[new]
    fn new(py_ds: Py<PyAny>) -> PyResult<Self> {
        Python::attach(|py| -> PyResult<Self> {
            let inner = python_datastream_to_rust(py, py_ds)?;
            Ok(DataStreamPy { inner })
        })
    }

    /// Create DataStream from bytes (protobuf serialized)
    #[staticmethod]
    fn from_bytes(bytes: &[u8]) -> PyResult<Self> {
        let inner = actr_protocol::DataStream::decode(bytes)
            .map_err(|e| PyValueError::new_err(format!("Failed to decode DataStream: {e}")))?;
        Ok(DataStreamPy { inner })
    }

    /// Convert to Python protobuf object
    fn to_protobuf<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        rust_datastream_to_python(py, &self.inner)
    }

    /// Serialize to bytes
    fn to_bytes(&self) -> Vec<u8> {
        self.inner.encode_to_vec()
    }

    /// Get stream_id
    fn stream_id(&self) -> String {
        self.inner.stream_id.clone()
    }

    /// Get sequence
    fn sequence(&self) -> u64 {
        self.inner.sequence
    }

    /// Get payload
    fn payload(&self) -> Vec<u8> {
        self.inner.payload.to_vec()
    }

    /// Get timestamp_ms
    fn timestamp_ms(&self) -> Option<i64> {
        self.inner.timestamp_ms
    }
}

impl DataStreamPy {
    pub fn inner(&self) -> &actr_protocol::DataStream {
        &self.inner
    }

    pub fn from_rust(ds: actr_protocol::DataStream) -> Self {
        DataStreamPy { inner: ds }
    }
}

#[pyclass(frozen)]
#[derive(Clone, Copy, Debug)]
pub enum PayloadType {
    RpcReliable,
    RpcSignal,
    StreamReliable,
    StreamLatencyFirst,
}

impl PayloadType {
    fn to_rust(self) -> RpPayloadType {
        match self {
            PayloadType::RpcReliable => RpPayloadType::RpcReliable,
            PayloadType::RpcSignal => RpPayloadType::RpcSignal,
            PayloadType::StreamReliable => RpPayloadType::StreamReliable,
            PayloadType::StreamLatencyFirst => RpPayloadType::StreamLatencyFirst,
        }
    }
}

/// Python wrapper for ActorResult

#[pyclass(name = "ActorResult")]
pub struct ActorResultPy {
    #[pyo3(get)]
    ok: bool,
    value: Option<Py<PyAny>>,
    error: Option<Py<PyAny>>,
}

#[pymethods]
impl ActorResultPy {
    #[getter]
    fn value(&self, py: Python) -> Option<Py<PyAny>> {
        self.value.as_ref().map(|v| v.clone_ref(py))
    }

    #[getter]
    fn error(&self, py: Python) -> Option<Py<PyAny>> {
        self.error.as_ref().map(|e| e.clone_ref(py))
    }

    fn is_ok(&self) -> bool {
        self.ok
    }

    fn unwrap(&self, py: Python) -> PyResult<Py<PyAny>> {
        if self.ok {
            Ok(self.value.as_ref().unwrap().clone_ref(py))
        } else {
            let err = self.error.as_ref().unwrap().clone_ref(py).into_bound(py);
            Err(PyErr::from_value(err))
        }
    }

    fn unwrap_err(&self, py: Python) -> PyResult<Py<PyAny>> {
        if !self.ok {
            Ok(self.error.as_ref().unwrap().clone_ref(py))
        } else {
            Err(PyValueError::new_err("ActorResult.ok == True"))
        }
    }
}

type WrappedWorkload = PyWorkloadWrapper;
type WrappedNode = ActrNode<WrappedWorkload>;
type WrappedRef = ActrRef<WrappedWorkload>;

#[pyclass(name = "ActrSystem")]
pub struct ActrSystemPy {
    inner: Option<ActrSystem>,
}

#[pymethods]
impl ActrSystemPy {
    #[staticmethod]
    fn from_toml<'py>(py: Python<'py>, path: String) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            eprintln!("[py] Loading config from: {}", path);
            let config = actr_config::ConfigParser::from_file(&path)
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            eprintln!("[py] Config loaded, creating ActrSystem...");
            let system = ActrSystem::new(config).await.map_err(map_protocol_error)?;
            eprintln!("[py] ActrSystem created successfully");
            Python::attach(|py| {
                Py::new(
                    py,
                    ActrSystemPy {
                        inner: Some(system),
                    },
                )
                .map(Py::into_any)
            })
        })
    }

    fn attach(&mut self, workload: Py<PyAny>) -> PyResult<ActrNodePy> {
        eprintln!("[py] attach: taking system...");
        let system = self.inner.take().ok_or_else(|| {
            PyRuntimeError::new_err("ActrSystem already consumed (attach called twice)")
        })?;

        // 获取当前线程的事件循环句柄
        let event_loop = Python::attach(|py| -> PyResult<Py<PyAny>> {
            let asyncio = py.import("asyncio")?;
            let get_event_loop = asyncio.getattr("get_event_loop")?;
            let loop_obj = get_event_loop.call0()?;
            Ok(loop_obj.extract::<Py<PyAny>>()?)
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to get event loop: {e}")))?;

        eprintln!("[py] attach: creating PyWorkloadWrapper...");
        let mut py_workload = PyWorkloadWrapper::new(workload)?;
        py_workload.set_event_loop(event_loop);
        eprintln!("[py] attach: calling system.attach...");
        let node = system.attach(py_workload);
        eprintln!("[py] attach: system.attach completed");
        Ok(ActrNodePy { inner: Some(node) })
    }
}

#[pyclass(name = "ActrNode")]
pub struct ActrNodePy {
    inner: Option<WrappedNode>,
}

#[pymethods]
impl ActrNodePy {
    fn start<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let node = self.inner.take().ok_or_else(|| {
            PyRuntimeError::new_err("ActrNode already consumed (start called twice)")
        })?;

        // future_into_py 将 Rust async 函数转换为 Python awaitable
        //
        // 执行上下文说明：
        // 1. 调用时（当前）：在 Python GIL 中，返回 awaitable（不执行 async 代码）
        // 2. Python await 时：在 Python Event Loop 中调度
        // 3. Rust async 执行时：在 Tokio Runtime 中执行（可能在 Tokio worker 线程）
        // 4. 返回结果时：在 Tokio 线程中，通过 GIL 返回给 Python Event Loop
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            eprintln!("[py] ActrNode.start: calling node.start()...");
            // 此时在 Tokio Runtime 中执行
            let actr_ref = node.start().await.map_err(map_protocol_error)?;
            eprintln!("[py] ActrNode.start: node.start() completed");
            // 在 Tokio 线程中，通过 GIL 访问 Python 对象
            Python::attach(|py| {
                Py::new(
                    py,
                    ActrRefPy {
                        inner: Some(actr_ref),
                    },
                )
                .map(Py::into_any)
            })
        })
    }

    fn try_start<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let node = self.inner.take().ok_or_else(|| {
            PyRuntimeError::new_err("ActrNode already consumed (try_start called twice)")
        })?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let actr_ref = node.start().await.map_err(map_protocol_error)?;
            Python::attach(|py| {
                Py::new(
                    py,
                    ActrRefPy {
                        inner: Some(actr_ref),
                    },
                )
                .map(Py::into_any)
            })
        })
    }
}

#[pyclass(name = "ActrRef")]
pub struct ActrRefPy {
    inner: Option<WrappedRef>,
}

#[pymethods]
impl ActrRefPy {
    /// Get the Actor ID
    ///
    /// # Returns
    /// `ActrId` wrapper object
    fn actor_id<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("ActrRef has been consumed"))?;
        // Convert Rust ActrId to Python protobuf object
        rust_actr_id_to_python(py, inner.actor_id())
    }

    fn shutdown(&self) -> PyResult<()> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("ActrRef has been consumed"))?;
        inner.shutdown();
        Ok(())
    }

    fn wait_for_shutdown<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("ActrRef has been consumed"))?
            .clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner.wait_for_shutdown().await;
            Ok(Python::attach(|py| py.None()))
        })
    }

    fn wait_for_ctrl_c_and_shutdown<'py>(
        &mut self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self
            .inner
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("ActrRef already consumed"))?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .wait_for_ctrl_c_and_shutdown()
                .await
                .map_err(map_protocol_error)?;
            Ok(Python::attach(|py| py.None()))
        })
    }

    /// Call Actor method (Shell → Workload RPC)
    ///
    /// This method sends a request to the Actor's Workload and waits for a response.
    /// The target is automatically set to this Actor (in-process communication).
    ///
    /// # Parameters
    /// - `route_key`: Route key string (e.g., "package.Service.Method")
    /// - `request`: Request protobuf bytes
    /// - `timeout_ms`: Timeout in milliseconds (default: 30000)
    /// - `payload_type`: Payload transmission type (default: RpcReliable)
    /// - `response_type`: Optional Python protobuf class for automatic deserialization (default: None)
    ///
    /// # Returns
    /// `bytes | ResponseObject` - If `response_type` is provided, returns deserialized protobuf object; otherwise returns bytes
    ///
    /// # Note
    /// This is for Shell → Workload RPC calls. For Workload → other Actor calls, use `Context.call_raw()`.
    /// Raises exceptions on error.
    ///
    /// # Example
    /// ```python
    /// # Without response_type (returns bytes)
    /// response_bytes = await actr_ref.call("package.Service.Method", req_bytes)
    /// response = MyResponse.FromString(response_bytes)
    ///
    /// # With response_type (returns deserialized object)
    /// response = await actr_ref.call("package.Service.Method", req_bytes, response_type=MyResponse)
    /// ```
    #[pyo3(signature = (route_key, request, timeout_ms=30000, payload_type=PayloadType::RpcReliable, response_type=None))]
    fn call<'py>(
        &self,
        py: Python<'py>,
        route_key: String,
        request: &[u8],
        timeout_ms: i64,
        payload_type: PayloadType,
        response_type: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("ActrRef has been consumed"))?
            .clone();
        let request_bytes = Bytes::from(request.to_vec());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response_bytes = inner
                .call_raw(route_key, request_bytes, timeout_ms, payload_type.to_rust())
                .await
                .map_err(map_protocol_error)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                // If response_type is provided, automatically deserialize
                if let Some(response_type_obj) = response_type {
                    let response_class = response_type_obj.bind(py);
                    // Call FromString() method on the protobuf class
                    let response_obj = response_class
                        .call_method1("FromString", (PyBytes::new(py, &response_bytes),))
                        .map_err(|e| {
                            PyValueError::new_err(format!("Failed to deserialize response: {e}"))
                        })?;
                    Ok(response_obj.into_any().into())
                } else {
                    // No response_type provided, return bytes
                    Ok(PyBytes::new(py, &response_bytes).into_any().into())
                }
            })
            .map(Py::into_any)
        })
    }

    /// Send one-way message to Actor (Shell → Workload, fire-and-forget)
    ///
    /// This method sends a message to the Actor's Workload without waiting for a response.
    /// The target is automatically set to this Actor (in-process communication).
    ///
    /// # Parameters
    /// - `route_key`: Route key string (e.g., "package.Service.Method")
    /// - `message`: Message protobuf bytes
    /// - `payload_type`: Payload transmission type (default: RpcReliable)
    ///
    /// # Note
    /// This is for Shell → Workload fire-and-forget messages. For Workload → other Actor messages, use `Context.tell_raw()`.
    /// Raises exceptions on error.
    #[pyo3(signature = (route_key, message, payload_type=PayloadType::RpcReliable))]
    fn tell<'py>(
        &self,
        py: Python<'py>,
        route_key: String,
        message: &[u8],
        payload_type: PayloadType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("ActrRef has been consumed"))?
            .clone();
        let message_bytes = Bytes::from(message.to_vec());

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .tell_raw(route_key, message_bytes, payload_type.to_rust())
                .await
                .map_err(map_protocol_error)?;

            Ok(Python::attach(|py| py.None()))
        })
    }
}

#[pyclass(name = "Context")]
pub struct ContextPy {
    inner: RuntimeContext,
}

#[pymethods]
impl ContextPy {
    fn self_id<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        // Convert Rust ActrId to Python protobuf object
        rust_actr_id_to_python(py, self.inner.self_id())
    }

    fn caller_id<'py>(&self, py: Python<'py>) -> PyResult<Option<Py<PyAny>>> {
        // Convert Rust ActrId to Python protobuf object if present
        if let Some(id) = self.inner.caller_id() {
            Ok(Some(rust_actr_id_to_python(py, id)?))
        } else {
            Ok(None)
        }
    }

    fn request_id(&self) -> String {
        self.inner.request_id().to_string()
    }

    /// Discover route candidate using ActrType
    ///
    /// # Parameters
    /// - `actr_type`: Python `ActrType` protobuf object (from `generated.actr_pb2.ActrType`)
    ///
    /// # Returns
    /// Python `ActrId` protobuf object (from `generated.actr_pb2.ActrId`)
    fn discover_route_candidate<'py>(
        &self,
        py: Python<'py>,
        actr_type: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ctx = self.inner.clone();
        let target_type = python_actr_type_to_rust(py, actr_type)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let id = ctx
                .discover_route_candidate(&target_type)
                .await
                .map_err(map_protocol_error)?;
            // Convert Rust ActrId to Python protobuf object
            Python::attach(|py| rust_actr_id_to_python(py, &id).map(Py::into_any))
        })
    }

    /// Try discover route candidate using ActrType (raises exception on error)
    ///
    /// # Parameters
    /// - `actr_type`: Python `ActrType` protobuf object (from `generated.actr_pb2.ActrType`)
    ///
    /// # Returns
    /// Python `ActrId` protobuf object
    fn try_discover_route_candidate<'py>(
        &self,
        py: Python<'py>,
        actr_type: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ctx = self.inner.clone();
        let target_type = python_actr_type_to_rust(py, actr_type)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let id = ctx
                .discover_route_candidate(&target_type)
                .await
                .map_err(map_protocol_error)?;
            // Convert Rust ActrId to Python protobuf object
            Python::attach(|py| rust_actr_id_to_python(py, &id).map(Py::into_any))
        })
    }

    /// Execute a Request/Response RPC call
    ///
    /// # Parameters
    /// - `target`: Target destination (`Dest` wrapper object)
    ///   - Use `Dest.shell()` for Workload → App calls
    ///   - Use `Dest.local()` for calling local Workload
    ///   - Use `Dest.actor(actr_id)` for calling remote Actor
    /// - `route_key`: Route key string (e.g., "package.Service.Method")
    /// - `request`: Request protobuf bytes
    /// - `timeout_ms`: Timeout in milliseconds (default: 30000)
    /// - `payload_type`: Payload transmission type (default: RpcReliable)
    /// - `response_type`: Optional Python protobuf class for automatic deserialization (e.g., `my_service_pb2.EchoResponse`)
    ///
    /// # Returns
    /// `bytes | ResponseObject` - If `response_type` is provided, returns deserialized protobuf object; otherwise returns bytes
    ///
    /// # Note
    /// This method is for request/response RPC calls. For fire-and-forget messages, use `tell_raw()`.
    /// Raises exceptions on error.
    ///
    /// # Example
    /// ```python
    /// from actr_python_sdk import Dest
    ///
    /// # Call remote Actor
    /// target = Dest.actor(server_id)
    /// response = await ctx.call_raw(target, "package.Service.Method", req_bytes, response_type=MyResponse)
    ///
    /// # Call Shell (from Workload)
    /// target = Dest.shell()
    /// response_bytes = await ctx.call_raw(target, "package.Service.Method", req_bytes)
    /// ```
    #[pyo3(signature = (target, route_key, request, timeout_ms=30000, payload_type=PayloadType::RpcReliable, response_type=None))]
    fn call_raw<'py>(
        &self,
        py: Python<'py>,
        target: &DestPy,
        route_key: String,
        request: &[u8],
        timeout_ms: i64,
        payload_type: PayloadType,
        response_type: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ctx = self.inner.clone();
        let target_dest = target.inner().clone();
        let request_bytes = Bytes::from(request.to_vec());
        let payload_type_rust = payload_type.to_rust();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bytes = ctx
                .call_raw(
                    &target_dest,
                    route_key,
                    payload_type_rust,
                    request_bytes,
                    timeout_ms,
                )
                .await
                .map_err(map_protocol_error)?;
            Python::attach(|py| -> PyResult<Py<PyAny>> {
                // If response_type is provided, automatically deserialize
                if let Some(response_type_obj) = response_type {
                    let response_class = response_type_obj.bind(py);
                    // Call FromString() method on the protobuf class
                    let response_obj = response_class
                        .call_method1("FromString", (PyBytes::new(py, &bytes),))
                        .map_err(|e| {
                            PyValueError::new_err(format!("Failed to deserialize response: {e}"))
                        })?;
                    Ok(response_obj.into_any().into())
                } else {
                    // No response_type provided, return bytes
                    Ok(PyBytes::new(py, &bytes).into_any().into())
                }
            })
            .map(Py::into_any)
        })
    }

    /// Execute a fire-and-forget Message RPC call
    ///
    /// # Parameters
    /// - `target`: Target destination (`Dest` wrapper object)
    ///   - Use `Dest.shell()` for Workload → App calls
    ///   - Use `Dest.local()` for calling local Workload
    ///   - Use `Dest.actor(actr_id)` for calling remote Actor
    /// - `route_key`: Route key string (e.g., "package.Service.Method")
    /// - `message`: Message protobuf bytes
    /// - `payload_type`: Payload transmission type (default: RpcReliable)
    ///
    /// # Note
    /// This method is for fire-and-forget messages. For request/response calls, use `call_raw()`.
    /// Raises exceptions on error.
    #[pyo3(signature = (target, route_key, message, payload_type=PayloadType::RpcReliable))]
    fn tell_raw<'py>(
        &self,
        py: Python<'py>,
        target: &DestPy,
        route_key: String,
        message: &[u8],
        payload_type: PayloadType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ctx = self.inner.clone();
        let target_dest = target.inner().clone();
        let message_bytes = Bytes::from(message.to_vec());
        let payload_type_rust = payload_type.to_rust();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            ctx.tell_raw(&target_dest, route_key, payload_type_rust, message_bytes)
                .await
                .map_err(map_protocol_error)?;
            Ok(Python::attach(|py| py.None()))
        })
    }

    /// Register a DataStream callback function.
    ///
    /// The callback function will be called for each DataStream chunk received.
    /// Callback signature: `async def callback(data_stream: DataStream, sender_id: ActrId) -> None`
    ///
    /// # Parameters
    /// - `stream_id`: Stream identifier string
    /// - `callback`: Callback function that receives:
    ///   - `data_stream`: Python `DataStream` protobuf object (from `generated.package_pb2.DataStream`)
    ///   - `sender_id`: Python `ActrId` protobuf object (from `generated.actr_pb2.ActrId`)
    ///
    /// Usage:
    ///   async def my_callback(data_stream: package_pb2.DataStream, sender_id: actr_pb2.ActrId):
    ///       print(f"Received from {sender_id}: seq={data_stream.sequence}")
    ///   await ctx.register_stream("stream-1", my_callback)
    fn register_stream<'py>(
        &self,
        py: Python<'py>,
        stream_id: String,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ctx = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let callback_py = Python::attach(|py| callback.clone_ref(py));
            ctx.register_stream(stream_id.clone(), move |chunk, sender_id| {
                let callback_clone = Python::attach(|py| callback_py.clone_ref(py));
                let stream_id_debug = stream_id.clone();
                Box::pin(async move {
                    tracing::info!(
                        "[py] Stream callback invoked: stream_id={}, seq={}, size={} bytes, sender={:?}",
                        stream_id_debug,
                        chunk.sequence,
                        chunk.payload.len(),
                        sender_id
                    );

                    // Convert Rust DataStream and ActrId to Python protobuf objects
                    let (py_ds, py_sender_id) = Python::attach(|py| -> PyResult<(Py<PyAny>, Py<PyAny>)> {
                        let ds = rust_datastream_to_python(py, &chunk)?;
                        let sender_id_rust = sender_id.clone();
                        let sid = rust_actr_id_to_python(py, &sender_id_rust)?;
                        Ok((ds, sid))
                    })
                    .map_err(|e| ProtocolError::TransportError(format!("Failed to convert to Python objects: {e}")))?;

                    // Call Python callback in a blocking task
                    let result = tokio::task::spawn_blocking(move || {
                        Python::attach(|py| -> PyResult<Py<PyAny>> {
                            let callback_obj = callback_clone.bind(py);
                            let py_ds_obj = py_ds.bind(py);
                            let py_sender_id_obj = py_sender_id.bind(py);
                            let coro = callback_obj.call1((py_ds_obj, py_sender_id_obj))?;
                            // Use asyncio.run to execute the coroutine in a blocking context
                            let asyncio = py.import("asyncio")?;
                            let run_func = asyncio.getattr("run")?;
                            let result = run_func.call1((coro,))?;
                            Ok(result.into_any().into())
                        })
                    })
                    .await
                    .map_err(|e| ProtocolError::TransportError(format!("Task join failed: {e}")))?;

                    match result {
                        Ok(_) => {
                            tracing::debug!(
                                "[py] Stream callback completed successfully: stream_id={}",
                                stream_id_debug
                            );
                            Ok(())
                        }
                        Err(e) => {
                            tracing::error!(
                                "[py] Stream callback error: stream_id={}, error={:?}",
                                stream_id_debug,
                                e
                            );
                            Err(ProtocolError::TransportError(format!(
                                "Python callback error: {e}"
                            )))
                        }
                    }
                })
            })
            .await
            .map_err(map_protocol_error)?;

            Ok(Python::attach(|py| py.None()))
        })
    }

    fn unregister_stream<'py>(
        &self,
        py: Python<'py>,
        stream_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ctx = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            ctx.unregister_stream(&stream_id)
                .await
                .map_err(map_protocol_error)?;
            Ok(Python::attach(|py| py.None()))
        })
    }

    /// Send DataStream
    ///
    /// # Parameters
    /// - `target`: Target destination (`Dest` wrapper object)
    ///   - Use `Dest.shell()` for Workload → App calls
    ///   - Use `Dest.local()` for calling local Workload
    ///   - Use `Dest.actor(actr_id)` for calling remote Actor
    /// - `data_stream`: `DataStream` wrapper object
    ///
    /// # Note
    /// Raises exceptions on error.
    /// Uses `StreamReliable` payload type by default.
    #[pyo3(signature = (target, data_stream))]
    fn send_data_stream<'py>(
        &self,
        py: Python<'py>,
        target: &DestPy,
        data_stream: &DataStreamPy,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ctx = self.inner.clone();
        let target_dest = target.inner().clone();
        let chunk = data_stream.inner().clone();
        let stream_id = chunk.stream_id.clone();
        let sequence = chunk.sequence;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            tracing::info!(
                "[py] send_data_stream: target={:?}, stream_id={}, sequence={}, payload_size={} bytes",
                target_dest,
                stream_id,
                sequence,
                chunk.payload.len()
            );

            // Send with StreamReliable payload type (hardcoded)
            ctx.send_data_stream_with_type(&target_dest, RpPayloadType::StreamReliable, chunk)
                .await
                .map_err(|e| {
                    tracing::error!(
                        "[py] send_data_stream FAILED: target={:?}, stream_id={}, sequence={}, error={:?}",
                        target_dest,
                        stream_id,
                        sequence,
                        e
                    );
                    e
                })
                .map_err(map_protocol_error)?;

            tracing::info!(
                "[py] send_data_stream SUCCESS: target={:?}, stream_id={}, sequence={}",
                target_dest,
                stream_id,
                sequence
            );

            Ok(Python::attach(|py| py.None()))
        })
    }
}

#[pyclass(name = "StreamIter")]
pub struct StreamIterPy {
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Py<PyAny>>>>,
}

#[pymethods]
impl StreamIterPy {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let rx = Arc::clone(&self.rx);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = rx.lock().await;
            match guard.recv().await {
                Some(item) => Ok(item),
                None => Err(PyStopAsyncIteration::new_err(())),
            }
        })
    }
}

/// Workload wrapper that forwards lifecycle and dispatch to a Python object.
pub struct PyWorkloadWrapper {
    py_obj: Py<PyAny>,
    /// Python 事件循环句柄，用于 run_coroutine_threadsafe
    event_loop: Option<Py<PyAny>>,
}

impl PyWorkloadWrapper {
    pub fn new(obj: Py<PyAny>) -> PyResult<Self> {
        Ok(Self {
            py_obj: obj,
            event_loop: None,
        })
    }

    /// 设置事件循环句柄（在 attach() 时调用）
    pub fn set_event_loop(&mut self, loop_obj: Py<PyAny>) {
        self.event_loop = Some(loop_obj);
    }

    fn make_context_py<'py>(
        &self,
        py: Python<'py>,
        ctx: &RuntimeContext,
    ) -> PyResult<Py<ContextPy>> {
        Py::new(py, ContextPy { inner: ctx.clone() })
    }

    /// Get Dispatcher object from Python Workload (if implemented)
    ///
    /// Returns Some(dispatcher_obj) if Workload implements get_dispatcher() method,
    /// None otherwise (for backward compatibility).
    fn get_dispatcher(&self) -> Option<Py<PyAny>> {
        Python::attach(|py| -> PyResult<Option<Py<PyAny>>> {
            let obj = self.py_obj.bind(py);
            if obj.hasattr("get_dispatcher")? {
                if let Ok(dispatcher) = obj.call_method0("get_dispatcher") {
                    if let Ok(dispatcher_obj) = dispatcher.extract::<Py<PyAny>>() {
                        return Ok(Some(dispatcher_obj));
                    }
                }
            }
            Ok(None)
        })
        .ok()
        .flatten()
    }
}

pub struct PyDispatcher;

impl PyDispatcher {
    /// Dispatch using a separate Dispatcher object (provided by Workload.get_dispatcher())
    async fn dispatch_with_dispatcher(
        workload: &PyWorkloadWrapper,
        dispatcher_obj: Py<PyAny>,
        runtime_ctx: &RuntimeContext,
        route_key: String,
        payload: Vec<u8>,
    ) -> ActorResult<Bytes> {
        let runtime_ctx_clone = runtime_ctx.clone();
        let route_key_clone = route_key.clone();
        let payload_clone = payload.clone();
        let workload_obj = Python::attach(|py| workload.py_obj.clone_ref(py));

        // Helper function to create ContextPy
        fn make_ctx_py(py: Python, ctx: &RuntimeContext) -> PyResult<Py<ContextPy>> {
            Py::new(py, ContextPy { inner: ctx.clone() })
        }

        // 使用保存的事件循环句柄，通过 run_coroutine_threadsafe 投递到 Python 事件循环
        let event_loop = workload.event_loop.as_ref().ok_or_else(|| {
            ProtocolError::TransportError(
                "Event loop not set. Make sure to call attach() from within an async context."
                    .to_string(),
            )
        })?;

        // 直接在 Python::attach 中等待结果
        // 注意：concurrent.futures.Future.result() 是阻塞调用，但由于我们在异步上下文中，
        // 并且 run_coroutine_threadsafe 已经在 Python 事件循环的线程中执行协程，
        // 所以 result() 的阻塞不会影响 Python 事件循环的执行
        let result_obj = Python::attach(|py| -> PyResult<Py<PyAny>> {
            // 创建协程
            let ctx_py = make_ctx_py(py, &runtime_ctx_clone).map_err(|e| {
                PyErr::from(PyValueError::new_err(format!(
                    "Failed to create ContextPy: {e}"
                )))
            })?;

            let dispatcher = dispatcher_obj.bind(py);
            let workload_py = workload_obj.bind(py);
            let ctx_obj = ctx_py.clone_ref(py);
            let route = PyString::new(py, &route_key_clone);
            let pay = PyBytes::new(py, &payload_clone);

            let coro = dispatcher.call_method1("dispatch", (workload_py, route, pay, ctx_obj))?;

            // 使用 run_coroutine_threadsafe 将协程投递到 Python 事件循环
            let asyncio = py.import("asyncio")?;
            let run_coroutine_threadsafe = asyncio.getattr("run_coroutine_threadsafe")?;
            let concurrent_future = run_coroutine_threadsafe.call1((coro, event_loop.bind(py)))?;

            // 直接在 Python::attach 中等待结果
            // concurrent.futures.Future.result() 会阻塞当前线程直到完成
            // 这是安全的，因为：
            // 1. run_coroutine_threadsafe 已经在 Python 事件循环的线程中调度了协程
            // 2. result() 的阻塞只是等待 Python 事件循环完成执行，不会阻塞 Python 事件循环本身
            let result_method = concurrent_future.getattr("result")?;
            let result = result_method.call0()?;
            Ok(result.into_any().into())
        })
        .map_err(|e| {
            ProtocolError::TransportError(format!("Python dispatcher.dispatch call failed: {e}"))
        })?;

        // Convert return value to bytes
        Python::attach(|py| {
            let bound = result_obj.bind(py);
            if let Ok(b) = bound.cast::<PyBytes>() {
                Ok(Bytes::from(b.as_bytes().to_vec()))
            } else if let Ok(v) = bound.extract::<Vec<u8>>() {
                Ok(Bytes::from(v))
            } else {
                Err(ProtocolError::EncodeError(
                    "Python dispatcher.dispatch must return bytes".to_string(),
                ))
            }
        })
    }
}

#[async_trait::async_trait]
impl actr_framework::MessageDispatcher for PyDispatcher {
    type Workload = PyWorkloadWrapper;

    async fn dispatch<C: FrameworkContext>(
        workload: &Self::Workload,
        envelope: RpcEnvelope,
        ctx: &C,
    ) -> ActorResult<Bytes> {
        // Downcast the generic Context to RuntimeContext (requires Context: Any)
        let any = ctx as &dyn Any;
        let runtime_ctx = any.downcast_ref::<RuntimeContext>().ok_or_else(|| {
            ProtocolError::TransportError("Context is not RuntimeContext".to_string())
        })?;

        let payload = envelope
            .payload
            .as_ref()
            .map(|b| b.to_vec())
            .unwrap_or_default();
        let route_key = envelope.route_key.clone();

        // Check if Workload provides a Dispatcher via get_dispatcher() method
        let dispatcher_obj = workload.get_dispatcher().ok_or_else(|| {
            ProtocolError::TransportError(
                format!(
                    "Workload does not provide a dispatcher. Please implement get_dispatcher() method. route_key: {}",
                    route_key
                )
            )
        })?;

        // Use the provided Dispatcher object
        PyDispatcher::dispatch_with_dispatcher(
            workload,
            dispatcher_obj,
            runtime_ctx,
            route_key,
            payload,
        )
        .await
    }
}

#[async_trait::async_trait]
impl Workload for PyWorkloadWrapper {
    type Dispatcher = PyDispatcher;

    async fn on_start<C: FrameworkContext>(&self, ctx: &C) -> ActorResult<()> {
        let any = ctx as &dyn Any;
        let runtime_ctx = any.downcast_ref::<RuntimeContext>().ok_or_else(|| {
            ProtocolError::TransportError("Context is not RuntimeContext".to_string())
        })?;

        let ctx_py = Python::attach(|py| self.make_context_py(py, runtime_ctx)).map_err(|e| {
            ProtocolError::TransportError(format!("Failed to create ContextPy: {e}"))
        })?;

        // If on_start is not implemented, treat as no-op.
        let maybe = Python::attach(|py| {
            let obj = self.py_obj.bind(py);
            Ok(obj.hasattr("on_start")?)
        })
        .map_err(|e: PyErr| ProtocolError::TransportError(format!("Python hasattr failed: {e}")))?;

        if !maybe {
            return Ok(());
        }

        let fut = Python::attach(|py| -> PyResult<_> {
            let obj = self.py_obj.bind(py);
            let ctx_obj = ctx_py.clone_ref(py);
            let coro = obj.call_method1("on_start", (ctx_obj,))?;
            pyo3_async_runtimes::tokio::into_future(coro)
        })
        .map_err(|e| ProtocolError::TransportError(format!("Python on_start call failed: {e}")))?;

        fut.await.map_err(|e| {
            ProtocolError::TransportError(format!("Python on_start await failed: {e}"))
        })?;
        Ok(())
    }

    async fn on_stop<C: FrameworkContext>(&self, ctx: &C) -> ActorResult<()> {
        let any = ctx as &dyn Any;
        let runtime_ctx = any.downcast_ref::<RuntimeContext>().ok_or_else(|| {
            ProtocolError::TransportError("Context is not RuntimeContext".to_string())
        })?;

        let ctx_py = Python::attach(|py| self.make_context_py(py, runtime_ctx)).map_err(|e| {
            ProtocolError::TransportError(format!("Failed to create ContextPy: {e}"))
        })?;

        let maybe = Python::attach(|py| {
            let obj = self.py_obj.bind(py);
            Ok(obj.hasattr("on_stop")?)
        })
        .map_err(|e: PyErr| ProtocolError::TransportError(format!("Python hasattr failed: {e}")))?;

        if !maybe {
            return Ok(());
        }

        let fut = Python::attach(|py| -> PyResult<_> {
            let obj = self.py_obj.bind(py);
            let ctx_obj = ctx_py.clone_ref(py);
            let coro = obj.call_method1("on_stop", (ctx_obj,))?;
            pyo3_async_runtimes::tokio::into_future(coro)
        })
        .map_err(|e| ProtocolError::TransportError(format!("Python on_stop call failed: {e}")))?;

        fut.await.map_err(|e| {
            ProtocolError::TransportError(format!("Python on_stop await failed: {e}"))
        })?;
        Ok(())
    }
}

#[pymodule]
fn actr_raw(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // pyo3-async-runtimes 0.27 uses a lazily-initialized Tokio runtime by default.

    // Initialize Rust logging/tracing (only once)
    ensure_observability_initialized();

    m.add("ActrRuntimeError", _py.get_type::<ActrRuntimeError>())?;
    m.add("ActrTransportError", _py.get_type::<ActrTransportError>())?;
    m.add("ActrDecodeError", _py.get_type::<ActrDecodeError>())?;
    m.add("ActrUnknownRoute", _py.get_type::<ActrUnknownRoute>())?;
    m.add(
        "ActrGateNotInitialized",
        _py.get_type::<ActrGateNotInitialized>(),
    )?;
    m.add_class::<PayloadType>()?;
    m.add_class::<DestPy>()?;
    m.add_class::<DataStreamPy>()?;
    m.add_class::<ActrSystemPy>()?;
    m.add_class::<ActrNodePy>()?;
    m.add_class::<ActrRefPy>()?;
    m.add_class::<ContextPy>()?;
    m.add_class::<StreamIterPy>()?;

    Ok(())
}
