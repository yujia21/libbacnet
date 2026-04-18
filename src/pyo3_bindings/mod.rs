// PyO3 bindings
//
// Exposes the sans-IO `Stack` and all associated input/output types to Python.
// The Python API is:
//
//   from libbacnet import (
//       Stack, StackConfig,
//       InputReceived, InputTick, InputSend,
//       OutputTransmit, OutputEvent, OutputDeadline,
//       EventResponse, EventTimeout, EventAbort, EventError,
//       EventUnconfirmedReceived,
//       UnconfirmedIAm, UnconfirmedIAmRouterToNetwork,
//       BacnetAddr,
//       ServiceReadProperty, ServiceReadPropertyMultiple, ServiceWriteProperty,
//       ReadAccessSpec, PropertyReference,
//       PropertyValueNull, PropertyValueBoolean, PropertyValueUnsigned,
//       PropertyValueSigned, PropertyValueReal, PropertyValueDouble,
//       PropertyValueOctetString, PropertyValueCharacterString,
//       PropertyValueBitString, PropertyValueEnumerated,
//       PropertyValueDate, PropertyValueTime,
//       PropertyValueAny,
//       ObjectIdentifier,
//       BacnetError, BacnetTimeoutError, InvokeIdExhaustedError,
//   )

// PyO3 0.20 emits `non_local_definitions` warnings for `#[pymethods]` blocks.
// This is a known issue with the macro; suppress it for the entire module.
#![allow(non_local_definitions)]

use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::codec::types::{
    BitString, CharacterEncoding, CharacterString, Date, ObjectIdentifier, ObjectType,
    PropertyValue, Time, Weekday,
};
use crate::services::read_property_multiple::{PropertyReference, ReadAccessSpec};
use crate::services::{read_property, read_property_multiple};
use crate::stack::addr::BacnetAddr;
use crate::stack::types::{
    BacnetEvent, BacnetService, Input, Output, StackConfig, UnconfirmedMessage,
};
use crate::stack::Stack;

// ─────────────────────────────────────────────────────────────────────────────
// Custom Python exceptions
// ─────────────────────────────────────────────────────────────────────────────

pyo3::create_exception!(
    libbacnet,
    BacnetError,
    PyException,
    "BACnet protocol error."
);
pyo3::create_exception!(
    libbacnet,
    BacnetTimeoutError,
    BacnetError,
    "No response received within the retry budget."
);
pyo3::create_exception!(
    libbacnet,
    InvokeIdExhaustedError,
    BacnetError,
    "All 256 invoke IDs are in use for this destination."
);

// ─────────────────────────────────────────────────────────────────────────────
// Helper: ObjectIdentifier Python class
// ─────────────────────────────────────────────────────────────────────────────

/// BACnet ObjectIdentifier (type, instance).
#[pyclass(name = "ObjectIdentifier", module = "libbacnet")]
#[derive(Clone, Debug)]
pub struct PyObjectIdentifier {
    pub object_type: u16,
    #[pyo3(get, set)]
    pub instance: u32,
}

#[pymethods]
impl PyObjectIdentifier {
    #[new]
    fn new(object_type: u16, instance: u32) -> Self {
        Self {
            object_type,
            instance,
        }
    }
    #[getter]
    fn get_object_type(&self, py: Python<'_>) -> PyObject {
        object_type_to_py(py, self.object_type)
    }
    #[setter]
    fn set_object_type(&mut self, value: u16) {
        self.object_type = value;
    }
    fn __repr__(&self) -> String {
        format!(
            "ObjectIdentifier(type={}, instance={})",
            self.object_type, self.instance
        )
    }
}

fn oid_to_rust(py_oid: &PyObjectIdentifier) -> ObjectIdentifier {
    ObjectIdentifier {
        object_type: ObjectType::from_u16(py_oid.object_type),
        instance: py_oid.instance,
    }
}

fn oid_from_rust(oid: &ObjectIdentifier) -> PyObjectIdentifier {
    PyObjectIdentifier {
        object_type: oid.object_type.to_u16(),
        instance: oid.instance,
    }
}

/// Wrap a raw u16 object-type value into a Python `ObjectType` IntEnum instance.
/// Falls back to the raw integer if the import fails.
fn object_type_to_py(py: Python<'_>, value: u16) -> PyObject {
    if let Ok(m) = py.import("libbacnet._enums") {
        if let Ok(cls) = m.getattr("ObjectType") {
            if let Ok(v) = cls.call1((value as u32,)) {
                return v.into();
            }
        }
    }
    value.into_py(py)
}

/// Wrap a raw u32 property-id value into a Python `PropertyIdentifier` IntEnum instance.
/// Falls back to the raw integer if the import fails.
fn property_id_to_py(py: Python<'_>, value: u32) -> PyObject {
    if let Ok(m) = py.import("libbacnet._enums") {
        if let Ok(cls) = m.getattr("PropertyIdentifier") {
            if let Ok(v) = cls.call1((value,)) {
                return v.into();
            }
        }
    }
    value.into_py(py)
}

/// Wrap a raw u32 error-class value into a Python `ErrorClass` IntEnum instance.
fn error_class_to_py(py: Python<'_>, value: u32) -> PyObject {
    if let Ok(m) = py.import("libbacnet._enums") {
        if let Ok(cls) = m.getattr("ErrorClass") {
            if let Ok(v) = cls.call1((value,)) {
                return v.into();
            }
        }
    }
    value.into_py(py)
}

/// Wrap a raw u32 error-code value into a Python `ErrorCode` IntEnum instance.
fn error_code_to_py(py: Python<'_>, value: u32) -> PyObject {
    if let Ok(m) = py.import("libbacnet._enums") {
        if let Ok(cls) = m.getattr("ErrorCode") {
            if let Ok(v) = cls.call1((value,)) {
                return v.into();
            }
        }
    }
    value.into_py(py)
}

// ─────────────────────────────────────────────────────────────────────────────
// BacnetAddr Python class
// ─────────────────────────────────────────────────────────────────────────────

/// IPv4 BACnet address (addr, port).
#[pyclass(name = "BacnetAddr", module = "libbacnet")]
#[derive(Clone, Debug)]
pub struct PyBacnetAddr {
    #[pyo3(get, set)]
    pub addr: String,
    #[pyo3(get, set)]
    pub port: u16,
}

#[pymethods]
impl PyBacnetAddr {
    #[new]
    fn new(addr: String, port: u16) -> Self {
        Self { addr, port }
    }
    fn __repr__(&self) -> String {
        format!("BacnetAddr('{}', {})", self.addr, self.port)
    }
}

fn addr_to_rust(py_addr: &PyBacnetAddr) -> PyResult<BacnetAddr> {
    let ipv4: std::net::Ipv4Addr = py_addr
        .addr
        .parse()
        .map_err(|_| PyValueError::new_err(format!("Invalid IPv4 address: {}", py_addr.addr)))?;
    Ok(BacnetAddr {
        addr: ipv4,
        port: py_addr.port,
    })
}

fn addr_from_rust(addr: &BacnetAddr) -> PyBacnetAddr {
    PyBacnetAddr {
        addr: addr.addr.to_string(),
        port: addr.port,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PropertyValue Python types
// ─────────────────────────────────────────────────────────────────────────────

#[pyclass(name = "PropertyValueNull", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueNull;
#[pymethods]
impl PyPropertyValueNull {
    #[new]
    fn new() -> Self {
        Self
    }
    fn __repr__(&self) -> &'static str {
        "PropertyValueNull()"
    }
}

#[pyclass(name = "PropertyValueBoolean", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueBoolean {
    #[pyo3(get, set)]
    pub value: bool,
}
#[pymethods]
impl PyPropertyValueBoolean {
    #[new]
    fn new(value: bool) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueBoolean({})", self.value)
    }
}

#[pyclass(name = "PropertyValueUnsigned", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueUnsigned {
    #[pyo3(get, set)]
    pub value: u32,
}
#[pymethods]
impl PyPropertyValueUnsigned {
    #[new]
    fn new(value: u32) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueUnsigned({})", self.value)
    }
}

#[pyclass(name = "PropertyValueSigned", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueSigned {
    #[pyo3(get, set)]
    pub value: i32,
}
#[pymethods]
impl PyPropertyValueSigned {
    #[new]
    fn new(value: i32) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueSigned({})", self.value)
    }
}

#[pyclass(name = "PropertyValueReal", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueReal {
    #[pyo3(get, set)]
    pub value: f32,
}
#[pymethods]
impl PyPropertyValueReal {
    #[new]
    fn new(value: f32) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueReal({})", self.value)
    }
}

#[pyclass(name = "PropertyValueDouble", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueDouble {
    #[pyo3(get, set)]
    pub value: f64,
}
#[pymethods]
impl PyPropertyValueDouble {
    #[new]
    fn new(value: f64) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueDouble({})", self.value)
    }
}

#[pyclass(name = "PropertyValueOctetString", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueOctetString {
    #[pyo3(get, set)]
    pub value: Vec<u8>,
}
#[pymethods]
impl PyPropertyValueOctetString {
    #[new]
    fn new(value: Vec<u8>) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueOctetString({:?})", self.value)
    }
}

#[pyclass(name = "PropertyValueCharacterString", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueCharacterString {
    #[pyo3(get, set)]
    pub value: String,
}
#[pymethods]
impl PyPropertyValueCharacterString {
    #[new]
    fn new(value: String) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueCharacterString({:?})", self.value)
    }
}

#[pyclass(name = "PropertyValueBitString", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueBitString {
    #[pyo3(get, set)]
    pub used_bits: u8,
    #[pyo3(get, set)]
    pub bits: Vec<u8>,
}
#[pymethods]
impl PyPropertyValueBitString {
    #[new]
    fn new(used_bits: u8, bits: Vec<u8>) -> Self {
        Self { used_bits, bits }
    }
    fn __repr__(&self) -> String {
        format!(
            "PropertyValueBitString(used={}, bits={:?})",
            self.used_bits, self.bits
        )
    }
}

#[pyclass(name = "PropertyValueEnumerated", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueEnumerated {
    #[pyo3(get, set)]
    pub value: u32,
}
#[pymethods]
impl PyPropertyValueEnumerated {
    #[new]
    fn new(value: u32) -> Self {
        Self { value }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueEnumerated({})", self.value)
    }
}

#[pyclass(name = "PropertyValueDate", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueDate {
    #[pyo3(get, set)]
    pub year: u16,
    #[pyo3(get, set)]
    pub month: u8,
    #[pyo3(get, set)]
    pub day: u8,
    /// weekday 0=Mon..6=Sun, 7=Mon-Fri, None=wildcard
    #[pyo3(get, set)]
    pub weekday: Option<u8>,
}
#[pymethods]
impl PyPropertyValueDate {
    #[new]
    fn new(year: u16, month: u8, day: u8, weekday: Option<u8>) -> Self {
        Self {
            year,
            month,
            day,
            weekday,
        }
    }
    fn __repr__(&self) -> String {
        format!(
            "PropertyValueDate({}-{:02}-{:02})",
            self.year, self.month, self.day
        )
    }
}

#[pyclass(name = "PropertyValueTime", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueTime {
    #[pyo3(get, set)]
    pub hour: u8,
    #[pyo3(get, set)]
    pub minute: u8,
    #[pyo3(get, set)]
    pub second: u8,
    #[pyo3(get, set)]
    pub hundredths: u8,
}
#[pymethods]
impl PyPropertyValueTime {
    #[new]
    fn new(hour: u8, minute: u8, second: u8, hundredths: u8) -> Self {
        Self {
            hour,
            minute,
            second,
            hundredths,
        }
    }
    fn __repr__(&self) -> String {
        format!(
            "PropertyValueTime({:02}:{:02}:{:02}.{:02})",
            self.hour, self.minute, self.second, self.hundredths
        )
    }
}

#[pyclass(name = "PropertyValueAny", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueAny {
    #[pyo3(get, set)]
    pub data: Vec<u8>,
}
#[pymethods]
impl PyPropertyValueAny {
    #[new]
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
    fn __repr__(&self) -> String {
        format!("PropertyValueAny({:?})", self.data)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Response result types
// ─────────────────────────────────────────────────────────────────────────────

/// Multiple decoded values from a ReadProperty array/list response.
#[pyclass(name = "PropertyValueArray", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyValueArray {
    #[pyo3(get)]
    pub values: Vec<PyObject>,
}
#[pymethods]
impl PyPropertyValueArray {
    fn __repr__(&self) -> String {
        format!("PropertyValueArray([{} item(s)])", self.values.len())
    }
}

#[pyclass(name = "BacnetPropertyError", module = "libbacnet")]
#[derive(Clone)]
pub struct PyBacnetPropertyError {
    pub error_class: u32,
    pub error_code: u32,
    pub error_class_py: PyObject,
    pub error_code_py: PyObject,
}
#[pymethods]
impl PyBacnetPropertyError {
    #[new]
    fn new(py: Python<'_>, error_class: u32, error_code: u32) -> Self {
        Self {
            error_class,
            error_code,
            error_class_py: error_class_to_py(py, error_class),
            error_code_py: error_code_to_py(py, error_code),
        }
    }
    #[getter]
    fn get_error_class(&self, py: Python<'_>) -> PyObject {
        self.error_class_py.clone_ref(py)
    }
    #[getter]
    fn get_error_code(&self, py: Python<'_>) -> PyObject {
        self.error_code_py.clone_ref(py)
    }
    fn __repr__(&self) -> String {
        format!(
            "BacnetPropertyError(error_class={}, error_code={})",
            self.error_class, self.error_code
        )
    }
}

/// Result for a single property within ReadPropertyMultiple.
#[pyclass(name = "PropertyResult", module = "libbacnet")]
#[derive(Clone)]
pub struct PyPropertyResult {
    #[pyo3(get)]
    pub property_id: PyObject,
    #[pyo3(get)]
    pub array_index: Option<u32>,
    /// Either a PropertyValue* instance or a BacnetPropertyError.
    #[pyo3(get)]
    pub value: PyObject,
}
#[pymethods]
impl PyPropertyResult {
    fn __repr__(&self, py: Python<'_>) -> String {
        format!(
            "PropertyResult(property_id={}, array_index={:?}, value={})",
            self.property_id
                .as_ref(py)
                .str()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            self.array_index,
            self.value
                .as_ref(py)
                .repr()
                .map(|s| s.to_string())
                .unwrap_or_default()
        )
    }
}

/// Result for one object in a ReadPropertyMultiple response.
#[pyclass(name = "ObjectResult", module = "libbacnet")]
#[derive(Clone)]
pub struct PyObjectResult {
    #[pyo3(get)]
    pub object_id: Py<PyObjectIdentifier>,
    #[pyo3(get)]
    pub properties: Vec<Py<PyPropertyResult>>,
}
#[pymethods]
impl PyObjectResult {
    fn __repr__(&self, py: Python<'_>) -> String {
        let oid = self.object_id.as_ref(py).borrow();
        format!(
            "ObjectResult(object_id={}, properties=[...])",
            oid.__repr__()
        )
    }
}

/// Decoded result of a ReadProperty response.
#[pyclass(name = "ReadPropertyResult", module = "libbacnet")]
#[derive(Clone)]
pub struct PyReadPropertyResult {
    #[pyo3(get)]
    pub object_id: Py<PyObjectIdentifier>,
    #[pyo3(get)]
    pub property_id: PyObject,
    #[pyo3(get)]
    pub array_index: Option<u32>,
    /// A PropertyValue* instance.
    #[pyo3(get)]
    pub value: PyObject,
}
#[pymethods]
impl PyReadPropertyResult {
    fn __repr__(&self, py: Python<'_>) -> String {
        let oid = self.object_id.as_ref(py).borrow();
        format!(
            "ReadPropertyResult(object_id={}, property_id={}, array_index={:?}, value={})",
            oid.__repr__(),
            self.property_id
                .as_ref(py)
                .str()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            self.array_index,
            self.value
                .as_ref(py)
                .repr()
                .map(|s| s.to_string())
                .unwrap_or_default()
        )
    }
}

/// Decoded result of a ReadPropertyMultiple response.
#[pyclass(name = "ReadPropertyMultipleResult", module = "libbacnet")]
#[derive(Clone)]
pub struct PyReadPropertyMultipleResult {
    #[pyo3(get)]
    pub objects: Vec<Py<PyObjectResult>>,
}
#[pymethods]
impl PyReadPropertyMultipleResult {
    fn __repr__(&self) -> String {
        format!(
            "ReadPropertyMultipleResult(objects=[{} object(s)])",
            self.objects.len()
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Decode functions exposed to Python
// ─────────────────────────────────────────────────────────────────────────────

/// Decode a ReadProperty ComplexACK payload into a ReadPropertyResult.
#[pyfunction]
fn decode_read_property(py: Python<'_>, data: Vec<u8>) -> PyResult<PyReadPropertyResult> {
    let result = read_property::decode_response(&data)
        .map_err(|e| BacnetError::new_err(format!("Decode error: {:?}", e)))?;
    let oid_py = Py::new(py, oid_from_rust(&result.object_id))?;
    let value_py = property_value_to_py(py, result.value);
    Ok(PyReadPropertyResult {
        object_id: oid_py,
        property_id: property_id_to_py(py, u32::from(result.property_id)),
        array_index: result.array_index,
        value: value_py,
    })
}

/// Decode a ReadPropertyMultiple ComplexACK payload into a ReadPropertyMultipleResult.
#[pyfunction]
fn decode_read_property_multiple(
    py: Python<'_>,
    data: Vec<u8>,
) -> PyResult<PyReadPropertyMultipleResult> {
    let result = read_property_multiple::decode_response(&data)
        .map_err(|e| BacnetError::new_err(format!("Decode error: {:?}", e)))?;

    let mut objects = Vec::new();
    for obj in result.objects {
        let oid_py = Py::new(py, oid_from_rust(&obj.object_id))?;
        let mut properties = Vec::new();
        for prop in obj.properties {
            let value_py: PyObject = match prop.value {
                Ok(pv) => property_value_to_py(py, pv),
                Err(e) => PyBacnetPropertyError {
                    error_class: e.error_class,
                    error_code: e.error_code,
                    error_class_py: error_class_to_py(py, e.error_class),
                    error_code_py: error_code_to_py(py, e.error_code),
                }
                .into_py(py),
            };
            let prop_py = Py::new(
                py,
                PyPropertyResult {
                    property_id: property_id_to_py(py, u32::from(prop.property_id)),
                    array_index: prop.array_index,
                    value: value_py,
                },
            )?;
            properties.push(prop_py);
        }
        let obj_py = Py::new(
            py,
            PyObjectResult {
                object_id: oid_py,
                properties,
            },
        )?;
        objects.push(obj_py);
    }
    Ok(PyReadPropertyMultipleResult { objects })
}

/// Convert a Rust `PropertyValue` → Python object.
fn property_value_to_py(py: Python<'_>, pv: PropertyValue) -> PyObject {
    match pv {
        PropertyValue::Null => PyPropertyValueNull.into_py(py),
        PropertyValue::Boolean(v) => PyPropertyValueBoolean { value: v }.into_py(py),
        PropertyValue::Unsigned(v) => PyPropertyValueUnsigned { value: v }.into_py(py),
        PropertyValue::Signed(v) => PyPropertyValueSigned { value: v }.into_py(py),
        PropertyValue::Real(v) => PyPropertyValueReal { value: v }.into_py(py),
        PropertyValue::Double(v) => PyPropertyValueDouble { value: v }.into_py(py),
        PropertyValue::OctetString(v) => PyPropertyValueOctetString { value: v }.into_py(py),
        PropertyValue::CharacterString(cs) => {
            PyPropertyValueCharacterString { value: cs.value }.into_py(py)
        }
        PropertyValue::BitString(bs) => PyPropertyValueBitString {
            used_bits: bs.used_bits,
            bits: bs.bits,
        }
        .into_py(py),
        PropertyValue::Enumerated(v) => PyPropertyValueEnumerated { value: v }.into_py(py),
        PropertyValue::Date(d) => {
            let weekday = d.weekday.map(|w| w as u8);
            PyPropertyValueDate {
                year: d.year,
                month: d.month,
                day: d.day,
                weekday,
            }
            .into_py(py)
        }
        PropertyValue::Time(t) => PyPropertyValueTime {
            hour: t.hour,
            minute: t.minute,
            second: t.second,
            hundredths: t.hundredths,
        }
        .into_py(py),
        PropertyValue::ObjectIdentifier(oid) => oid_from_rust(&oid).into_py(py),
        PropertyValue::Any(data) => PyPropertyValueAny { data }.into_py(py),
        PropertyValue::Array(items) => {
            let py_values: Vec<PyObject> = items
                .into_iter()
                .map(|pv| property_value_to_py(py, pv))
                .collect();
            PyPropertyValueArray { values: py_values }.into_py(py)
        }
    }
}

/// Convert a Python PropertyValue object → Rust `PropertyValue`.
fn property_value_from_py(py: Python<'_>, obj: &PyObject) -> PyResult<PropertyValue> {
    let obj_ref = obj.as_ref(py);
    if obj_ref.is_instance_of::<PyPropertyValueNull>() {
        Ok(PropertyValue::Null)
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueBoolean>>() {
        Ok(PropertyValue::Boolean(v.value))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueUnsigned>>() {
        Ok(PropertyValue::Unsigned(v.value))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueSigned>>() {
        Ok(PropertyValue::Signed(v.value))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueReal>>() {
        Ok(PropertyValue::Real(v.value))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueDouble>>() {
        Ok(PropertyValue::Double(v.value))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueOctetString>>() {
        Ok(PropertyValue::OctetString(v.value.clone()))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueCharacterString>>() {
        Ok(PropertyValue::CharacterString(CharacterString {
            encoding: CharacterEncoding::Utf8,
            value: v.value.clone(),
        }))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueBitString>>() {
        Ok(PropertyValue::BitString(BitString {
            used_bits: v.used_bits,
            bits: v.bits.clone(),
        }))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueEnumerated>>() {
        Ok(PropertyValue::Enumerated(v.value))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueDate>>() {
        let weekday = v.weekday.and_then(|w| match w {
            0 => Some(Weekday::Monday),
            1 => Some(Weekday::Tuesday),
            2 => Some(Weekday::Wednesday),
            3 => Some(Weekday::Thursday),
            4 => Some(Weekday::Friday),
            5 => Some(Weekday::Saturday),
            6 => Some(Weekday::Sunday),
            7 => Some(Weekday::MondayToFriday),
            _ => None,
        });
        Ok(PropertyValue::Date(Date {
            year: v.year,
            month: v.month,
            day: v.day,
            weekday,
        }))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueTime>>() {
        Ok(PropertyValue::Time(Time {
            hour: v.hour,
            minute: v.minute,
            second: v.second,
            hundredths: v.hundredths,
        }))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyObjectIdentifier>>() {
        Ok(PropertyValue::ObjectIdentifier(oid_to_rust(&v)))
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyPropertyValueAny>>() {
        Ok(PropertyValue::Any(v.data.clone()))
    } else {
        Err(PyValueError::new_err("Unknown PropertyValue type"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Input Python types
// ─────────────────────────────────────────────────────────────────────────────

/// Input: a UDP datagram was received.
#[pyclass(name = "InputReceived", module = "libbacnet")]
pub struct PyInputReceived {
    #[pyo3(get, set)]
    pub data: Vec<u8>,
    #[pyo3(get, set)]
    pub src: Py<PyBacnetAddr>,
}
#[pymethods]
impl PyInputReceived {
    #[new]
    fn new(data: Vec<u8>, src: Py<PyBacnetAddr>) -> Self {
        Self { data, src }
    }
}

/// Input: timer tick — inject current time (seconds since arbitrary epoch).
#[pyclass(name = "InputTick", module = "libbacnet")]
pub struct PyInputTick {
    #[pyo3(get, set)]
    pub now: f64,
}
#[pymethods]
impl PyInputTick {
    #[new]
    fn new(now: f64) -> Self {
        Self { now }
    }
}

/// ReadAccessSpec for ReadPropertyMultiple.
#[pyclass(name = "ReadAccessSpec", module = "libbacnet")]
#[derive(Clone)]
pub struct PyReadAccessSpec {
    #[pyo3(get, set)]
    pub object_id: Py<PyObjectIdentifier>,
    #[pyo3(get, set)]
    pub properties: Vec<(u32, Option<u32>)>,
}
#[pymethods]
impl PyReadAccessSpec {
    /// properties: list of (property_id, optional_array_index)
    #[new]
    fn new(object_id: Py<PyObjectIdentifier>, properties: Vec<(u32, Option<u32>)>) -> Self {
        Self {
            object_id,
            properties,
        }
    }
}

/// Input: send a confirmed service request.
#[pyclass(name = "InputSend", module = "libbacnet")]
pub struct PyInputSend {
    #[pyo3(get, set)]
    pub service: PyObject,
    #[pyo3(get, set)]
    pub dest: Py<PyBacnetAddr>,
}
#[pymethods]
impl PyInputSend {
    #[new]
    fn new(service: PyObject, dest: Py<PyBacnetAddr>) -> Self {
        Self { service, dest }
    }
}

/// Service: ReadProperty request.
#[pyclass(name = "ServiceReadProperty", module = "libbacnet")]
pub struct PyServiceReadProperty {
    #[pyo3(get, set)]
    pub object_id: Py<PyObjectIdentifier>,
    #[pyo3(get, set)]
    pub property_id: u32,
    #[pyo3(get, set)]
    pub array_index: Option<u32>,
}
#[pymethods]
impl PyServiceReadProperty {
    #[new]
    fn new(object_id: Py<PyObjectIdentifier>, property_id: u32, array_index: Option<u32>) -> Self {
        Self {
            object_id,
            property_id,
            array_index,
        }
    }
}

/// Service: ReadPropertyMultiple request.
#[pyclass(name = "ServiceReadPropertyMultiple", module = "libbacnet")]
pub struct PyServiceReadPropertyMultiple {
    #[pyo3(get, set)]
    pub specs: Vec<Py<PyReadAccessSpec>>,
}
#[pymethods]
impl PyServiceReadPropertyMultiple {
    #[new]
    fn new(specs: Vec<Py<PyReadAccessSpec>>) -> Self {
        Self { specs }
    }
}

/// Service: WriteProperty request.
#[pyclass(name = "ServiceWriteProperty", module = "libbacnet")]
pub struct PyServiceWriteProperty {
    #[pyo3(get, set)]
    pub object_id: Py<PyObjectIdentifier>,
    #[pyo3(get, set)]
    pub property_id: u32,
    #[pyo3(get, set)]
    pub value: PyObject,
    #[pyo3(get, set)]
    pub array_index: Option<u32>,
    #[pyo3(get, set)]
    pub priority: Option<u8>,
}
#[pymethods]
impl PyServiceWriteProperty {
    #[new]
    fn new(
        object_id: Py<PyObjectIdentifier>,
        property_id: u32,
        value: PyObject,
        array_index: Option<u32>,
        priority: Option<u8>,
    ) -> Self {
        Self {
            object_id,
            property_id,
            value,
            array_index,
            priority,
        }
    }
}

fn service_from_py(py: Python<'_>, obj: &PyObject) -> PyResult<BacnetService> {
    let obj_ref = obj.as_ref(py);
    if let Ok(v) = obj_ref.extract::<PyRef<PyServiceReadProperty>>() {
        let oid = v.object_id.as_ref(py).borrow();
        Ok(BacnetService::ReadProperty {
            object_id: oid_to_rust(&oid),
            property_id: crate::enums::PropertyIdentifier::from(v.property_id),
            array_index: v.array_index,
        })
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyServiceReadPropertyMultiple>>() {
        let mut specs = Vec::new();
        for spec_py in &v.specs {
            let spec = spec_py.as_ref(py).borrow();
            let oid = spec.object_id.as_ref(py).borrow();
            let properties = spec
                .properties
                .iter()
                .map(|(pid, ai)| PropertyReference {
                    property_id: crate::enums::PropertyIdentifier::from(*pid),
                    array_index: *ai,
                })
                .collect();
            specs.push(ReadAccessSpec {
                object_id: oid_to_rust(&oid),
                properties,
            });
        }
        Ok(BacnetService::ReadPropertyMultiple { specs })
    } else if let Ok(v) = obj_ref.extract::<PyRef<PyServiceWriteProperty>>() {
        let oid = v.object_id.as_ref(py).borrow();
        let value = property_value_from_py(py, &v.value)?;
        Ok(BacnetService::WriteProperty {
            object_id: oid_to_rust(&oid),
            property_id: crate::enums::PropertyIdentifier::from(v.property_id),
            value,
            array_index: v.array_index,
            priority: v.priority,
        })
    } else {
        Err(PyValueError::new_err("Unknown service type"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Output and Event Python types
// ─────────────────────────────────────────────────────────────────────────────

/// Output: transmit bytes to a destination.
#[pyclass(name = "OutputTransmit", module = "libbacnet")]
pub struct PyOutputTransmit {
    #[pyo3(get)]
    pub data: Vec<u8>,
    #[pyo3(get)]
    pub dest: Py<PyBacnetAddr>,
}

/// Output: a BACnet event occurred.
#[pyclass(name = "OutputEvent", module = "libbacnet")]
pub struct PyOutputEvent {
    #[pyo3(get)]
    pub event: PyObject,
}

/// Output: call Tick no later than this time.
#[pyclass(name = "OutputDeadline", module = "libbacnet")]
pub struct PyOutputDeadline {
    #[pyo3(get)]
    pub deadline: f64,
}

/// Event: confirmed response received.
#[pyclass(name = "EventResponse", module = "libbacnet")]
pub struct PyEventResponse {
    #[pyo3(get)]
    pub invoke_id: u8,
    #[pyo3(get)]
    pub payload: Vec<u8>,
}

/// Event: request timed out.
#[pyclass(name = "EventTimeout", module = "libbacnet")]
pub struct PyEventTimeout {
    #[pyo3(get)]
    pub invoke_id: u8,
}

/// Event: server sent Abort PDU.
#[pyclass(name = "EventAbort", module = "libbacnet")]
pub struct PyEventAbort {
    #[pyo3(get)]
    pub invoke_id: u8,
    #[pyo3(get)]
    pub reason: u8,
}

/// Event: BACnet error or local error.
#[pyclass(name = "EventError", module = "libbacnet")]
pub struct PyEventError {
    #[pyo3(get)]
    pub invoke_id: u8,
    #[pyo3(get)]
    pub message: String,
}

/// Event: unconfirmed message received.
#[pyclass(name = "EventUnconfirmedReceived", module = "libbacnet")]
pub struct PyEventUnconfirmedReceived {
    #[pyo3(get)]
    pub src: Py<PyBacnetAddr>,
    #[pyo3(get)]
    pub message: PyObject,
}

/// Unconfirmed I-Am message.
#[pyclass(name = "UnconfirmedIAm", module = "libbacnet")]
pub struct PyUnconfirmedIAm {
    #[pyo3(get)]
    pub device_id: Py<PyObjectIdentifier>,
    #[pyo3(get)]
    pub max_apdu: u32,
    #[pyo3(get)]
    pub segmentation: u8,
    #[pyo3(get)]
    pub vendor_id: u32,
}

/// Unconfirmed IAmRouterToNetwork message.
#[pyclass(name = "UnconfirmedIAmRouterToNetwork", module = "libbacnet")]
pub struct PyUnconfirmedIAmRouterToNetwork {
    #[pyo3(get)]
    pub networks: Vec<u16>,
}

fn output_to_py(py: Python<'_>, output: Output) -> PyResult<PyObject> {
    match output {
        Output::Transmit { data, dest } => {
            let dest_py = Py::new(py, addr_from_rust(&dest))?;
            Ok(PyOutputTransmit {
                data,
                dest: dest_py,
            }
            .into_py(py))
        }
        Output::Deadline(d) => Ok(PyOutputDeadline { deadline: d }.into_py(py)),
        Output::Event(event) => {
            let event_obj = bacnet_event_to_py(py, event)?;
            Ok(PyOutputEvent { event: event_obj }.into_py(py))
        }
    }
}

fn bacnet_event_to_py(py: Python<'_>, event: BacnetEvent) -> PyResult<PyObject> {
    match event {
        BacnetEvent::Response { invoke_id, payload } => {
            Ok(PyEventResponse { invoke_id, payload }.into_py(py))
        }
        BacnetEvent::Timeout { invoke_id } => Ok(PyEventTimeout { invoke_id }.into_py(py)),
        BacnetEvent::Abort { invoke_id, reason } => {
            Ok(PyEventAbort { invoke_id, reason }.into_py(py))
        }
        BacnetEvent::Error { invoke_id, message } => {
            Ok(PyEventError { invoke_id, message }.into_py(py))
        }
        BacnetEvent::UnconfirmedReceived { src, message } => {
            let src_py = Py::new(py, addr_from_rust(&src))?;
            let message_py = unconfirmed_to_py(py, message)?;
            Ok(PyEventUnconfirmedReceived {
                src: src_py,
                message: message_py,
            }
            .into_py(py))
        }
    }
}

fn unconfirmed_to_py(py: Python<'_>, msg: UnconfirmedMessage) -> PyResult<PyObject> {
    match msg {
        UnconfirmedMessage::IAm {
            device_id,
            max_apdu,
            segmentation,
            vendor_id,
        } => {
            let dev_py = Py::new(py, oid_from_rust(&device_id))?;
            Ok(PyUnconfirmedIAm {
                device_id: dev_py,
                max_apdu,
                segmentation,
                vendor_id,
            }
            .into_py(py))
        }
        UnconfirmedMessage::IAmRouterToNetwork { networks } => {
            Ok(PyUnconfirmedIAmRouterToNetwork { networks }.into_py(py))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stack Python class
// ─────────────────────────────────────────────────────────────────────────────

/// StackConfig — tuneable protocol parameters.
#[pyclass(name = "StackConfig", module = "libbacnet")]
pub struct PyStackConfig {
    #[pyo3(get, set)]
    pub apdu_timeout_secs: f64,
    #[pyo3(get, set)]
    pub apdu_retries: u8,
    #[pyo3(get, set)]
    pub max_apdu_length: usize,
    #[pyo3(get, set)]
    pub max_segment_buffer: usize,
}
#[pymethods]
impl PyStackConfig {
    #[new]
    #[pyo3(signature = (
        apdu_timeout_secs = 3.0,
        apdu_retries = 3,
        max_apdu_length = 1476,
        max_segment_buffer = 2097152,
    ))]
    fn new(
        apdu_timeout_secs: f64,
        apdu_retries: u8,
        max_apdu_length: usize,
        max_segment_buffer: usize,
    ) -> Self {
        Self {
            apdu_timeout_secs,
            apdu_retries,
            max_apdu_length,
            max_segment_buffer,
        }
    }
}

/// The sans-IO BACnet/IP stack.
///
/// Usage:
///
/// ```python
/// stack = Stack()
/// outputs = stack.process(InputTick(now=0.0))
/// ```
#[pyclass(name = "Stack", module = "libbacnet")]
pub struct PyStack {
    inner: Stack,
}

#[pymethods]
impl PyStack {
    /// Create a new stack with optional config.
    #[new]
    #[pyo3(signature = (config = None))]
    fn new(config: Option<&PyStackConfig>) -> Self {
        let cfg = config
            .map(|c| StackConfig {
                apdu_timeout_secs: c.apdu_timeout_secs,
                apdu_retries: c.apdu_retries,
                max_apdu_length: c.max_apdu_length,
                max_segment_buffer: c.max_segment_buffer,
            })
            .unwrap_or_default();
        Self {
            inner: Stack::new(cfg),
        }
    }

    /// Process one input event and return a list of output objects.
    ///
    /// Raises `InvokeIdExhaustedError` when all 256 invoke IDs for the
    /// destination are in use.
    fn process(&mut self, py: Python<'_>, input: &PyAny) -> PyResult<PyObject> {
        let rust_input = py_to_input(py, input)?;
        let outputs = self.inner.process(rust_input);
        let list = PyList::empty(py);
        for out in outputs {
            // Convert InvokeIdExhausted error events to a Python exception.
            if let Output::Event(BacnetEvent::Error { ref message, .. }) = out {
                if message.contains("pool exhausted") {
                    return Err(InvokeIdExhaustedError::new_err(message.clone()));
                }
            }
            list.append(output_to_py(py, out)?)?;
        }
        Ok(list.into())
    }
}

fn py_to_input(py: Python<'_>, obj: &PyAny) -> PyResult<Input> {
    if let Ok(v) = obj.extract::<PyRef<PyInputReceived>>() {
        let src = v.src.as_ref(py).borrow();
        let rust_src = addr_to_rust(&src)?;
        return Ok(Input::Received {
            data: v.data.clone(),
            src: rust_src,
        });
    }
    if let Ok(v) = obj.extract::<PyRef<PyInputTick>>() {
        return Ok(Input::Tick { now: v.now });
    }
    if let Ok(v) = obj.extract::<PyRef<PyInputSend>>() {
        let dest = v.dest.as_ref(py).borrow();
        let rust_dest = addr_to_rust(&dest)?;
        let service = service_from_py(py, &v.service)?;
        return Ok(Input::Send {
            service,
            dest: rust_dest,
        });
    }
    Err(PyValueError::new_err(
        "Expected InputReceived, InputTick, or InputSend",
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Module registration
// ─────────────────────────────────────────────────────────────────────────────

pub fn register(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    // Exceptions
    m.add("BacnetError", py.get_type::<BacnetError>())?;
    m.add("BacnetTimeoutError", py.get_type::<BacnetTimeoutError>())?;
    m.add(
        "InvokeIdExhaustedError",
        py.get_type::<InvokeIdExhaustedError>(),
    )?;

    // Core types
    m.add_class::<PyObjectIdentifier>()?;
    m.add_class::<PyBacnetAddr>()?;
    m.add_class::<PyStackConfig>()?;
    m.add_class::<PyStack>()?;

    // Input types
    m.add_class::<PyInputReceived>()?;
    m.add_class::<PyInputTick>()?;
    m.add_class::<PyInputSend>()?;
    m.add_class::<PyReadAccessSpec>()?;
    m.add_class::<PyServiceReadProperty>()?;
    m.add_class::<PyServiceReadPropertyMultiple>()?;
    m.add_class::<PyServiceWriteProperty>()?;

    // Output types
    m.add_class::<PyOutputTransmit>()?;
    m.add_class::<PyOutputEvent>()?;
    m.add_class::<PyOutputDeadline>()?;

    // Event types
    m.add_class::<PyEventResponse>()?;
    m.add_class::<PyEventTimeout>()?;
    m.add_class::<PyEventAbort>()?;
    m.add_class::<PyEventError>()?;
    m.add_class::<PyEventUnconfirmedReceived>()?;
    m.add_class::<PyUnconfirmedIAm>()?;
    m.add_class::<PyUnconfirmedIAmRouterToNetwork>()?;

    // PropertyValue types
    m.add_class::<PyPropertyValueNull>()?;
    m.add_class::<PyPropertyValueBoolean>()?;
    m.add_class::<PyPropertyValueUnsigned>()?;
    m.add_class::<PyPropertyValueSigned>()?;
    m.add_class::<PyPropertyValueReal>()?;
    m.add_class::<PyPropertyValueDouble>()?;
    m.add_class::<PyPropertyValueOctetString>()?;
    m.add_class::<PyPropertyValueCharacterString>()?;
    m.add_class::<PyPropertyValueBitString>()?;
    m.add_class::<PyPropertyValueEnumerated>()?;
    m.add_class::<PyPropertyValueDate>()?;
    m.add_class::<PyPropertyValueTime>()?;
    m.add_class::<PyPropertyValueAny>()?;
    m.add_class::<PyPropertyValueArray>()?;

    // Response result types
    m.add_class::<PyBacnetPropertyError>()?;
    m.add_class::<PyPropertyResult>()?;
    m.add_class::<PyObjectResult>()?;
    m.add_class::<PyReadPropertyResult>()?;
    m.add_class::<PyReadPropertyMultipleResult>()?;

    // Decode functions
    m.add_function(wrap_pyfunction!(decode_read_property, m)?)?;
    m.add_function(wrap_pyfunction!(decode_read_property_multiple, m)?)?;

    Ok(())
}
