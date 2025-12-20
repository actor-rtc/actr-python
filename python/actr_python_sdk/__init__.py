"""
Actr Python SDK

This module provides Python bindings for actr-runtime, including:
- High-level Pythonic API: Python-friendly wrappers (root package)
- Rust Binding: Direct Rust bindings (in .binding submodule, imports from actr_raw)
- Decorators: @actr.service and @actr.rpc (in .decorators submodule)

Recommended usage:
    from actr_python_sdk import actr, System
    
    @actr.service("my_service.EchoService")
    class MyService:
        @actr.rpc
        async def echo(self, req: EchoRequest, ctx) -> EchoResponse:
            return EchoResponse(message=req.message)
    
    system = await System.from_toml("Actr.toml")
    workload = MyService.create_workload()
    node = system.attach(workload)
    ref = await node.start()
"""

from importlib import metadata as _metadata
from typing import Optional, Union, TypeVar, Any

try:
    __version__ = _metadata.version("actr-python-sdk")
except _metadata.PackageNotFoundError:
    __version__ = "0.0.0"
from .binding import (
    ActrSystem as RustActrSystem,
    ActrNode as RustActrNode,
    ActrRef as RustActrRef,
    Context as RustContext,
    Dest as RustDest,
    PayloadType as RustPayloadType,
    # ActrId and ActrType are protobuf types, imported separately when needed
    DataStream as RustDataStream,
    ActrRuntimeError,
    ActrTransportError,
    ActrDecodeError,
    ActrUnknownRoute,
    ActrGateNotInitialized,
  
)

# Type variables for generic methods
T = TypeVar('T')
R = TypeVar('R')


class System:
    """
    High-level ActrSystem wrapper
    
    Provides a Pythonic interface for creating and managing Actor systems.
    """
    
    def __init__(self, rust_system):
        """Initialize with Rust ActrSystem"""
        self._rust = rust_system
    
    @staticmethod
    async def from_toml(path: str):
        """
        Create ActrSystem from TOML configuration file
        
        Args:
            path: Path to TOML configuration file
        
        Returns:
            System instance
        
        Raises:
            RuntimeError: If Rust extension is not available
            ActrRuntimeError: If system creation fails
        """
        if RustActrSystem is None:
            raise RuntimeError("Rust ActrSystem not available. Make sure Rust extension is built.")
        rust_system = await RustActrSystem.from_toml(path)
        return System(rust_system)
    
    def attach(self, workload):
        """
        Attach workload to system
        
        Args:
            workload: Workload instance
        
        Returns:
            Node instance
        """
        rust_node = self._rust.attach(workload)
        return Node(rust_node)


class Node:
    """
    High-level ActrNode wrapper
    
    Provides Pythonic interface for Actor nodes.
    """
    
    def __init__(self, rust_node):
        """Initialize with Rust ActrNode"""
        self._rust = rust_node
    
    async def start(self):
        """
        Start the node
        
        Returns:
            Ref instance
        
        Raises:
            ActrRuntimeError: If node start fails
        """
        rust_ref = await self._rust.start()
        return Ref(rust_ref)
    
    async def try_start(self):
        """
        Try to start the node
        
        Returns:
            Ref instance
        
        Raises:
            ActrRuntimeError: If node start fails
        """
        rust_ref = await self._rust.try_start()
        return Ref(rust_ref)


class Ref:
    """
    High-level ActrRef wrapper
    
    Provides Pythonic methods for interacting with running Actors.
    """
    
    def __init__(self, rust_ref):
        """Initialize with Rust ActrRef"""
        self._rust = rust_ref
    
    def actor_id(self):
        """Get Actor ID"""
        return self._rust.actor_id()
    
    async def call(
        self,
        route_key: str,
        request,
        timeout_ms: int = 30000,
        payload_type: RustPayloadType = None
    ):
        """
        Call Actor method (Shell → Workload RPC)
        
        Args:
            route_key: Route key string
            request: Request protobuf object (not bytes)
            timeout_ms: Timeout in milliseconds
            payload_type: Payload transmission type
        
        Returns:
            Response protobuf object
        
        Raises:
            ActrRuntimeError: If RPC call fails
            ValueError: If request is not a protobuf message
        """
        if payload_type is None:
            payload_type = RustPayloadType.RpcReliable
        
        # Serialize request
        if not hasattr(request, 'SerializeToString'):
            raise ValueError(f"Request must be a protobuf message, got {type(request)}")
        request_bytes = request.SerializeToString()
        
        # Call Rust method (now raises exception directly on error)
        return await self._rust.call(
            route_key,
            request_bytes,
            timeout_ms,
            payload_type
        )
    
    async def tell(
        self,
        route_key: str,
        message,
        payload_type: RustPayloadType = None
    ):
        """
        Send one-way message (Shell → Workload, fire-and-forget)
        
        Args:
            route_key: Route key string
            message: Message protobuf object (not bytes)
            payload_type: Payload transmission type
        
        Raises:
            ActrRuntimeError: If message send fails
            ValueError: If message is not a protobuf message
        """
        if payload_type is None:
            payload_type = RustPayloadType.RpcReliable
        
        # Serialize message
        if not hasattr(message, 'SerializeToString'):
            raise ValueError(f"Message must be a protobuf message, got {type(message)}")
        message_bytes = message.SerializeToString()
        
        # Call Rust method (now raises exception directly on error)
        await self._rust.tell(
            route_key,
            message_bytes,
            payload_type
        )
    
    def shutdown(self):
        """Trigger Actor shutdown"""
        self._rust.shutdown()
    
    async def wait_for_shutdown(self):
        """Wait for Actor to fully shutdown"""
        await self._rust.wait_for_shutdown()
    
    async def wait_for_ctrl_c_and_shutdown(self):
        """Wait for Ctrl+C signal, then shutdown"""
        await self._rust.wait_for_ctrl_c_and_shutdown()


class Ctx:
    """
    High-level Context wrapper
    
    Provides Pythonic methods for Actor context operations.
    """
    
    def __init__(self, rust_ctx):
        """Initialize with Rust Context"""
        self._rust = rust_ctx
    
    def self_id(self):
        """Get self Actor ID"""
        return self._rust.self_id()
    
    def caller_id(self):
        """Get caller Actor ID"""
        return self._rust.caller_id()
    
    def request_id(self) -> str:
        """Get current request ID"""
        return self._rust.request_id()
    
    async def discover(self, actr_type: Any):  # ActrType protobuf object
        """
        Discover route candidate
        
        Args:
            actr_type: ActrType protobuf object
        
        Returns:
            ActrId protobuf object
        
        Raises:
            ActrRuntimeError: If discovery fails
        """
        return await self._rust.discover_route_candidate(actr_type)
    
    async def call(
        self,
        target: Union[RustDest, Any],  # ActrId protobuf object or Dest
        route_key: str,
        request,
        timeout_ms: int = 30000,
        payload_type: RustPayloadType = None,
        response_type = None
    ):
        """
        Execute request/response RPC call
        
        Args:
            target: Dest wrapper object or ActrId protobuf object
            route_key: Route key string
            request: Request protobuf object (not bytes)
            timeout_ms: Timeout in milliseconds
            payload_type: Payload transmission type
            response_type: Optional Python protobuf class for automatic deserialization
        
        Returns:
            Response protobuf object (if response_type provided) or bytes
        
        Raises:
            ActrRuntimeError: If RPC call fails
            ValueError: If request is not a protobuf message
        """
        if payload_type is None:
            payload_type = RustPayloadType.RpcReliable
        
        # Convert ActrId to Dest if needed
        if not isinstance(target, RustDest):
            target = RustDest.actor(target)
        
        # Serialize request
        if not hasattr(request, 'SerializeToString'):
            raise ValueError(f"Request must be a protobuf message, got {type(request)}")
        request_bytes = request.SerializeToString()
        
        # Call Rust method (now raises exception directly on error)
        return await self._rust.call_raw(
            target,
            route_key,
            request_bytes,
            timeout_ms,
            payload_type,
            response_type
        )
    
    async def tell(
        self,
        target: Union[RustDest, Any],  # ActrId protobuf object or Dest
        route_key: str,
        message,
        payload_type: RustPayloadType = None
    ):
        """
        Execute fire-and-forget message RPC call
        
        Args:
            target: Dest wrapper object or ActrId protobuf object
            route_key: Route key string
            message: Message protobuf object (not bytes)
            payload_type: Payload transmission type
        
        Raises:
            ActrRuntimeError: If message send fails
            ValueError: If message is not a protobuf message
        """
        if payload_type is None:
            payload_type = RustPayloadType.RpcReliable
        
        # Convert ActrId to Dest if needed
        if not isinstance(target, RustDest):
            target = RustDest.actor(target)
        
        # Serialize message
        if not hasattr(message, 'SerializeToString'):
            raise ValueError(f"Message must be a protobuf message, got {type(message)}")
        message_bytes = message.SerializeToString()
        
        # Call Rust method (now raises exception directly on error)
        await self._rust.tell_raw(
            target,
            route_key,
            message_bytes,
            payload_type
        )
    
    async def register_stream(self, stream_id: str, callback):
        """Register DataStream callback"""
        await self._rust.register_stream(stream_id, callback)
    
    async def unregister_stream(self, stream_id: str):
        """Unregister DataStream"""
        await self._rust.unregister_stream(stream_id)
    
    async def send_stream(
        self,
        target: Union[RustDest, Any],  # ActrId protobuf object or Dest
        data_stream,
        payload_type: RustPayloadType = None
    ):
        """
        Send DataStream
        
        Args:
            target: Dest wrapper object or ActrId protobuf object
            data_stream: DataStream protobuf object or wrapper
            payload_type: Payload transmission type
        
        Raises:
            ActrRuntimeError: If send fails
        """
        if payload_type is None:
            payload_type = RustPayloadType.StreamReliable
        
        # Convert ActrId to Dest if needed
        if not isinstance(target, RustDest):
            target = RustDest.actor(target)
        
        # Convert protobuf to wrapper if needed
        if not isinstance(data_stream, RustDataStream):
            data_stream = RustDataStream(data_stream)
        
        # Call Rust method (now raises exception directly on error)
        await self._rust.send_data_stream(target, data_stream)


# Re-export types for convenience
Dest = RustDest
PayloadType = RustPayloadType
DataStream = RustDataStream
# ActrId and ActrType are protobuf types, should be imported from generated protobuf code
# Example: from generated.actr_pb2 import ActrId, ActrType

# Import decorators
from .decorators import service, rpc, ActrDecorators

# Create decorator namespace
actr = ActrDecorators()

# Re-export Rust binding module for advanced users who need direct access
# Usage: from actr_python_sdk.binding import ActrSystem, ActrRef, etc.
from . import binding

# Convenience: expose Rust binding types directly for backward compatibility
# but recommend using high-level API
ActrSystem = binding.ActrSystem
ActrNode = binding.ActrNode
ActrRef = binding.ActrRef
Context = binding.Context

__all__ = [
    "__version__",
    # High-level Pythonic API (recommended, root package)
    "System",
    "Node",
    "Ref",
    "Ctx",
    "Dest",
    "PayloadType",
    "DataStream",
    "ActrId",
    "ActrType",
    # Decorators (recommended)
    "actr",
    "service",
    "rpc",
    # Exceptions
    "ActrRuntimeError",
    "ActrTransportError",
    "ActrDecodeError",
    "ActrUnknownRoute",
    "ActrGateNotInitialized",
    # Rust bindings (for backward compatibility and advanced use)
    "ActrSystem",
    "ActrNode",
    "ActrRef",
    "Context",
    # Submodules (for direct access)
    "binding",
    "decorators",
]
