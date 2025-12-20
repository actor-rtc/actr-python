use actr_framework::Dest;
use actr_protocol::PayloadType as RpPayloadType;
use actr_protocol::prost::Message as ProstMessage;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::conversions::{
    python_actr_id_to_rust, python_datastream_to_rust, rust_actr_id_to_python,
    rust_datastream_to_python,
};

/// Python wrapper for Dest (destination identifier)
#[pyclass(name = "Dest")]
#[derive(Clone)]
pub struct DestPy {
    inner: Dest,
}

#[pymethods]
impl DestPy {
    #[staticmethod]
    fn shell() -> Self {
        DestPy { inner: Dest::Shell }
    }

    #[staticmethod]
    fn local() -> Self {
        DestPy { inner: Dest::Local }
    }

    #[staticmethod]
    fn actor<'py>(py: Python<'py>, actr_id: Py<PyAny>) -> PyResult<Self> {
        let rust_actr_id = python_actr_id_to_rust(py, actr_id)?;
        Ok(DestPy {
            inner: Dest::Actor(rust_actr_id),
        })
    }

    fn is_shell(&self) -> bool {
        self.inner.is_shell()
    }

    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    fn is_actor(&self) -> bool {
        self.inner.is_actor()
    }

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
    #[new]
    fn new(py_ds: Py<PyAny>) -> PyResult<Self> {
        Python::attach(|py| -> PyResult<Self> {
            let inner = python_datastream_to_rust(py, py_ds)?;
            Ok(DataStreamPy { inner })
        })
    }

    #[staticmethod]
    fn from_bytes(bytes: &[u8]) -> PyResult<Self> {
        let inner = actr_protocol::DataStream::decode(bytes)
            .map_err(|e| PyValueError::new_err(format!("Failed to decode DataStream: {e}")))?;
        Ok(DataStreamPy { inner })
    }

    fn to_protobuf<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        rust_datastream_to_python(py, &self.inner)
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.inner.encode_to_vec()
    }

    fn stream_id(&self) -> String {
        self.inner.stream_id.clone()
    }

    fn sequence(&self) -> u64 {
        self.inner.sequence
    }

    fn payload(&self) -> Vec<u8> {
        self.inner.payload.to_vec()
    }

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
    pub fn to_rust(self) -> RpPayloadType {
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
    pub ok: bool,
    pub value: Option<Py<PyAny>>,
    pub error: Option<Py<PyAny>>,
}
