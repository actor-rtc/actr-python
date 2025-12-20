use actr_protocol::ActrId;
use actr_protocol::prost::Message as ProstMessage;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

pub fn python_actr_id_to_rust(_py: Python, py_actr_id: Py<PyAny>) -> PyResult<ActrId> {
    Python::attach(|py| -> PyResult<ActrId> {
        let py_any = py_actr_id.bind(py);
        let bytes = py_any
            .call_method0("SerializeToString")?
            .extract::<Vec<u8>>()
            .map_err(|e| PyValueError::new_err(format!("Failed to serialize ActrId: {e}")))?;

        ActrId::decode(&bytes[..])
            .map_err(|e| PyValueError::new_err(format!("Failed to decode ActrId: {e}")))
    })
}

pub fn rust_actr_id_to_python(py: Python, actr_id: &ActrId) -> PyResult<Py<PyAny>> {
    let bytes = actr_id.encode_to_vec();
    let actr_module = py.import("generated.actr_pb2")?;
    let actr_id_class = actr_module.getattr("ActrId")?;
    let py_bytes = PyBytes::new(py, &bytes);
    let py_actr_id = actr_id_class
        .call_method1("FromString", (py_bytes,))?
        .extract::<Py<PyAny>>()?;
    Ok(py_actr_id)
}

pub fn python_actr_type_to_rust(
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

#[allow(dead_code)]
pub fn rust_actr_type_to_python(
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

pub fn python_datastream_to_rust(
    _py: Python,
    py_ds: Py<PyAny>,
) -> PyResult<actr_protocol::DataStream> {
    Python::attach(|py| -> PyResult<actr_protocol::DataStream> {
        let py_any = py_ds.bind(py);
        let bytes = py_any
            .call_method0("SerializeToString")?
            .extract::<Vec<u8>>()
            .map_err(|e| PyValueError::new_err(format!("Failed to serialize DataStream: {e}")))?;

        actr_protocol::DataStream::decode(&bytes[..])
            .map_err(|e| PyValueError::new_err(format!("Failed to decode DataStream: {e}")))
    })
}

pub fn rust_datastream_to_python(
    py: Python,
    ds: &actr_protocol::DataStream,
) -> PyResult<Py<PyAny>> {
    let bytes = ds.encode_to_vec();
    let package_module = py.import("generated.package_pb2")?;
    let datastream_class = package_module.getattr("DataStream")?;
    let py_bytes = PyBytes::new(py, &bytes);
    let py_ds = datastream_class
        .call_method1("FromString", (py_bytes,))?
        .extract::<Py<PyAny>>()?;
    Ok(py_ds)
}
