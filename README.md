# Actr Python SDK (`actr_python_sdk` + `actr_raw`)

Actr's Python SDK is a mixed Rust/Python package that ships a Pyo3-powered extension module (`actr_raw`) together with a Pythonic wrapper layer (`actr_python_sdk`). The Rust layer exposes the raw bindings, while the SDK layer provides decorators for defining services, a high-level `ActrSystem` API, and low-level bindings for advanced use cases.

## Layout
- `pyproject.toml` – Python packaging metadata for maturin/wheel builds
- `Cargo.toml` / `src/` – Rust extension implemented with Pyo3 (exports `actr_raw`)
- `python/` – Pure-Python helpers, decorators, and examples (packaged as `actr_python_sdk`)
- `python/README.md` – Decorator-focused guide (kept with the Python sources)

## Quick start
Requirements: Python 3.9+, Rust toolchain, `pip install maturin>=1.8`.

```bash
cd actr-python
maturin develop --release  # build and install into the current virtualenv
```

Then use the SDK:

```python
from actr_python_sdk import ActrSystem, actr
from generated import my_service_pb2

@actr.service("my_service.EchoService")
class MyService:
    @actr.rpc
    async def echo(self, req: my_service_pb2.EchoRequest, ctx):
        return my_service_pb2.EchoResponse(message=f"Echo: {req.message}")

system = await ActrSystem.from_toml("Actr.toml")
node = system.attach(MyService.create_workload())
ref = await node.start()
await ref.wait_for_ctrl_c_and_shutdown()
```

## Building wheels
```bash
# Build a release wheel and sdist into ./target/wheels
maturin build --release
```

Artifacts produced by `maturin build` include the Rust extension (`actr_raw`) and the Python sources from `python/`, ready for publishing to an internal index or PyPI.

**Note:** 
- When using `pip install` directly on the source directory
- For development, use `maturin develop --release` which will install both `actr_raw` and `actr_python_sdk` packages (via editable install).

## Development notes
- The crate depends on the local Rust workspace crates under `../actr/crates/*`; keep those paths in sync when moving the SDK.
- Set `RUST_LOG=debug` while developing to surface runtime diagnostics from the binding layer.
- Python typing support is shipped via `python/actr_python_sdk/py.typed`.

---

# Architecture Documentation

This section provides detailed information about the `actr_python_sdk` Python package architecture, package structure, and implementation details.

## Package Structure

```
actr_python_sdk/
├── __init__.py          # Root package entry, exports all public APIs
├── binding/             # Rust binding layer (low-level API, imports actr_raw)
└── decorators/          # Decorator implementation (@actr.service, @actr.rpc)
```

### Module Overview

#### `binding/` - Rust Binding Layer

**Responsibility**: Directly import and export all types and methods from the Rust extension module.

**Contents**:
- Import all types from the Rust extension module (`actr_raw`)
- Provide a unified import interface

**Exports**:
- `ActrSystem`, `ActrNode`, `ActrRef`, `Context`
- `Dest`, `PayloadType`
- `DataStream`, `ActrId`, `ActrType`
- All exception types: `ActrRuntimeError`, `ActrTransportError`, etc.

**Use Cases**:
- Advanced users who need direct access to Rust APIs
- Scenarios requiring fine-grained control

#### `__init__.py` - High-level Pythonic API

**Responsibility**: Provide Python-friendly high-level APIs that wrap the Rust bindings.

**Main Classes**:

1. **`ActrSystem`**: High-level system wrapper
   - `from_toml(path)` - Create system from TOML file
   - `attach(workload)` - Attach Workload

2. **`ActrNode`**: High-level node wrapper
   - `start()` - Start node (directly raises exceptions)

3. **`ActrRef`**: High-level ref wrapper
   - `call(route_key, request, ...)` - Call RPC (auto serialize)
   - `tell(route_key, message, ...)` - Send message (auto serialize)
   - `shutdown()`, `wait_for_shutdown()`, `wait_for_ctrl_c_and_shutdown()`

4. **`Context`**: High-level Context wrapper
   - `call(target, route_key, request, ...)` - RPC call
   - `tell(target, route_key, message, ...)` - Send message
   - `discover(actr_type)` - Service discovery
   - `register_stream()`, `unregister_stream()`, `send_stream()`

**Features**:
- **Auto serialization**: Accept protobuf request objects and automatically serialize to bytes; RPC responses are returned as `bytes` and should be deserialized manually via `ResponseType.FromString(response_bytes)`
- **Direct exception raising**: All methods directly raise exceptions, following Python conventions
- **Auto type conversion**: Automatically convert `ActrId` to `Dest` (e.g., `Dest.actor(actr_id)`)
- **Pythonic design**: Concise method names, reasonable parameters, following Python conventions

#### `decorators/` - Decorator Implementation

**Responsibility**: Provide decorator APIs to simplify service definitions.

**Main Decorators**:

1. **`@actr.service(service_name)`**
   - Mark class as Actr service
   - Automatically collect all `@actr.rpc` methods
   - Automatically generate Dispatcher and Workload
   - Add convenience method `create_workload()`

2. **`@actr.rpc` or `@actr.rpc(route_key="...")`**
   - Mark method as RPC handler function
   - Automatically generate route_key (default: `{service_name}.{method_name}`)
   - Support custom route_key

**Features**:
- Automatic Dispatcher generation (message routing)
- Automatic Workload generation (lifecycle management)
- Type inference (infer request/response types from type annotations)
- Service registry (global service metadata management)

**Exports from `__init__.py`**:
- High-level APIs: `ActrSystem`, `ActrNode`, `ActrRef`, `Context`
- Types: `Dest`, `PayloadType`, `DataStream`, `ActrId`, `ActrType`
- Exceptions: `ActrRuntimeError`, `ActrTransportError`, etc.
- Decorators: `actr`, `service`, `rpc`
- Rust binding: `binding` module (for advanced usage)

---

## Design Philosophy

### 1. Layered Architecture

```
┌─────────────────────────────────────┐
│  Decorator API (@actr.service, @actr.rpc) │  ← Simplest, recommended
├─────────────────────────────────────┤
│  High-level Pythonic API (ActrSystem, ActrRef, Context) │  ← Python-friendly, recommended
├─────────────────────────────────────┤
│  Rust Binding (binding/)            │  ← Low-level, direct Rust mapping
└─────────────────────────────────────┘
```

### 2. Pythonic Design Principles

- **Direct exception raising**: All high-level API methods directly raise exceptions, following Python conventions
  - Use standard `try/except` for error handling
  - Exception types: `ActrRuntimeError`, `ActrTransportError`, `ActrDecodeError`, etc.
- **Auto serialization**: Accept protobuf request objects and automatically serialize to bytes (RPC responses are returned as `bytes`)
- **Type inference**: Automatically infer types from type annotations
- **Concise API**: Concise method names, reasonable parameters

### 3. Backward Compatibility

- Preserve direct access to Rust bindings (`binding` module)

---

## API Layers

### Layer 1: Decorator API (Most Recommended)

**Features**: Simplest, automatically generates all code

```python
from actr_python_sdk import actr, ActrSystem

@actr.service("my_service.EchoService")
class MyService:
    @actr.rpc
    async def echo(self, req: EchoRequest, ctx) -> EchoResponse:
        return EchoResponse(message=req.message)

# Usage
system = await ActrSystem.from_toml("Actr.toml")
workload = MyService.create_workload()
node = system.attach(workload)
ref = await node.start()
```

### Layer 2: High-level Pythonic API

**Features**: Python-friendly, automatic serialization handling

```python
from actr_python_sdk import ActrSystem, ActrRef, Context

# Use high-level API
system = await ActrSystem.from_toml("Actr.toml")
workload = MyWorkload()
node = system.attach(workload)
ref = await node.start()

# Call RPC (auto serialize, directly raises exceptions)
try:
    response_bytes = await ref.call("route.key", request_obj)
    response_obj = ResponseType.FromString(response_bytes)
except ActrRuntimeError as e:
    # Handle error
    pass
```

### Layer 3: Rust Binding (Advanced Users)

**Features**: Direct access to Rust APIs, maximum control

```python
from actr_python_sdk.binding import ActrSystem, ActrRef

# Use Rust binding
system = await ActrSystem.from_toml("Actr.toml")
# Need manual serialization/deserialization of protobuf messages
```

---

## Implementation Details

### 1. Rust Binding Import Strategy

`binding/__init__.py` uses multiple strategies to import the Rust extension module:

1. Check if module already exists in `sys.modules`
2. Try `import actr_raw`
3. If all fail, provide stub and warning

### 2. High-level API Wrapping

Classes in `__init__.py` wrap Rust objects:

- Store Rust object references (`self._rust`)
- Provide Pythonic methods
- Automatically handle type conversion and serialization
- Unified exception handling

### 3. Decorator Implementation

`decorators/__init__.py` implements the decorator system:

- **Service registry**: Global `_service_registry` stores service metadata
- **Method collection**: Use `inspect` to collect all `@actr.rpc` methods
- **Type inference**: Use `get_type_hints` to infer request/response types
- **Auto generation**: Dynamically generate Dispatcher and Workload classes

### 4. Error Handling

**High-level API (Recommended)**:
- **Direct exception raising**: All methods directly raise exceptions
- Use standard Python exception handling: `try/except`
- Exception types: `ActrRuntimeError`, `ActrTransportError`, `ActrDecodeError`, etc.

**Example**:
```python
from actr_python_sdk import ActrSystem, Context, ActrRuntimeError

try:
    response = await ctx.call(server_id, "route.key", request)
    # Directly use response, no need to check is_ok()
except ActrRuntimeError as e:
    # Handle error
    log("error", f"RPC call failed: {e}")
```

**Rust Binding (Low-level)**:
- **Direct exception raising**: All methods directly raise exceptions
- Use standard Python exception handling: `try/except`
- Exception types: `ActrRuntimeError`, `ActrTransportError`, `ActrDecodeError`, etc.

**Decorators**:
- Automatically handle exception propagation
- Exceptions in handler methods automatically propagate to callers

---

## Usage Guide

### Quick Start (Recommended)

```python
from actr_python_sdk import actr, ActrSystem

@actr.service("my_service.EchoService")
class MyService:
    @actr.rpc
    async def echo(self, req: EchoRequest, ctx) -> EchoResponse:
        return EchoResponse(message=req.message)

async def main():
    system = await ActrSystem.from_toml("Actr.toml")
    workload = MyService.create_workload()
    node = system.attach(workload)
    ref = await node.start()
    await ref.wait_for_ctrl_c_and_shutdown()
```

### Using High-level API (Without Decorators)

```python
from actr_python_sdk import ActrSystem, ActrRef, Context

class MyWorkload:
    def __init__(self, handler):
        self.handler = handler
        self._dispatcher = MyDispatcher()
    
    def get_dispatcher(self):
        return self._dispatcher
    
    async def on_start(self, ctx: Context):
        pass

async def main():
    system = await ActrSystem.from_toml("Actr.toml")
    workload = MyWorkload(MyHandler())
    node = system.attach(workload)
    ref = await node.start()
```

### Using Rust Binding (Advanced Usage)

**Note**: Rust Binding now also directly raises exceptions, consistent with high-level API behavior.

```python
from actr_python_sdk.binding import ActrSystem, ActrRef, ActrRuntimeError

async def main():
    try:
        system = await ActrSystem.from_toml("Actr.toml")
        workload = MyWorkload()
        node = system.attach(workload)
        ref = await node.start()  # Directly raises exception, no need to check is_ok()
    except ActrRuntimeError as e:
        # Handle error
        pass
```

**Note**: Rust Binding and high-level API now use the same exception handling mechanism. All methods directly raise exceptions, following Python conventions.

---

## Summary

### Main Changes

1. **Separated Rust Binding**: Created `binding/` module, clearly distinguishing Rust binding and Python implementation
2. **High-level API Wrapping**: Created high-level API in `__init__.py`, providing Pythonic high-level APIs
3. **Decorator Support**: Implemented `@actr.service` and `@actr.rpc` decorators
4. **Unified Exception Handling**: All API layers (Rust Binding and high-level API) directly raise exceptions
   - Follows Python conventions: use `try/except` for error handling
   - Type-safe: Exception types are clear (`ActrRuntimeError`, `ActrTransportError`, `ActrDecodeError`, etc.)
   - Consistent: All API layers use the same error handling mechanism
5. **Auto Serialization**: High-level API automatically serializes protobuf requests; RPC responses are returned as `bytes` and should be deserialized manually via `ResponseType.FromString(response_bytes)`

### Recommended Usage

- **Daily Development**: Use decorator API (`@actr.service`, `@actr.rpc`)
- **Need Control**: Use high-level API (`ActrSystem`, `ActrRef`, `Context`)
- **Advanced Scenarios**: Use Rust Binding (`binding` module)

### Backward Compatibility

- All Rust binding types can still be imported
- Recommend using high-level API or decorators
