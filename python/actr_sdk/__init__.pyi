"""
Type stubs for actr_sdk package.

This file provides type hints for the actr_sdk Python SDK.
"""

from typing import Any, Callable, Coroutine, Optional, TypeVar
from enum import Enum

# Re-export from actr_raw submodule
from .actr_raw import (
    ActrRuntimeError as ActrRuntimeError,
    ActrTransportError as ActrTransportError,
    ActrDecodeError as ActrDecodeError,
    ActrUnknownRoute as ActrUnknownRoute,
    ActrGateNotInitialized as ActrGateNotInitialized,
    PayloadType as PayloadType,
    Dest as Dest,
    DataStream as DataStream,
)

__version__: str

T = TypeVar('T')
R = TypeVar('R')

# High-level wrapper classes

class ActrSystem:
    """High-level ActrSystem wrapper."""
    
    @staticmethod
    async def from_toml(path: str) -> "ActrSystem":
        """
        Create ActrSystem from TOML configuration file.
        
        Args:
            path: Path to TOML configuration file
        
        Returns:
            ActrSystem instance
        """
        ...
    
    def attach(self, workload: Any) -> "ActrNode":
        """
        Attach workload to system.
        
        Args:
            workload: Workload instance
        
        Returns:
            ActrNode instance
        """
        ...

class ActrNode:
    """High-level ActrNode wrapper."""
    
    async def start(self) -> "ActrRef":
        """
        Start the node.
        
        Returns:
            ActrRef instance
        """
        ...
    
    async def try_start(self) -> "ActrRef":
        """
        Try to start the node.
        
        Returns:
            ActrRef instance
        """
        ...

class ActrRef:
    """High-level ActrRef wrapper."""
    
    def actor_id(self) -> Any:
        """Get Actor ID."""
        ...
    
    async def call(
        self,
        route_key: str,
        request: Any,
        timeout_ms: int = 30000,
        payload_type: Optional[PayloadType] = None,
    ) -> bytes:
        """
        Call Actor method (Shell → Workload RPC).
        
        Args:
            route_key: Route key string
            request: Request protobuf object (not bytes)
            timeout_ms: Timeout in milliseconds
            payload_type: Payload transmission type
        
        Returns:
            Response bytes
        """
        ...
    
    async def tell(
        self,
        route_key: str,
        message: Any,
        payload_type: Optional[PayloadType] = None,
    ) -> None:
        """
        Send one-way message (Shell → Workload, fire-and-forget).
        
        Args:
            route_key: Route key string
            message: Message protobuf object (not bytes)
            payload_type: Payload transmission type
        """
        ...
    
    def shutdown(self) -> None:
        """Trigger Actor shutdown."""
        ...
    
    async def wait_for_shutdown(self) -> None:
        """Wait for Actor to fully shutdown."""
        ...
    
    async def wait_for_ctrl_c_and_shutdown(self) -> None:
        """Wait for Ctrl+C signal, then shutdown."""
        ...

class Context:
    """High-level Context wrapper."""
    
    def self_id(self) -> Any:
        """Get self Actor ID."""
        ...
    
    def caller_id(self) -> Optional[Any]:
        """Get caller Actor ID."""
        ...
    
    def request_id(self) -> str:
        """Get current request ID."""
        ...
    
    async def discover(self, actr_type: Any) -> Any:
        """
        Discover route candidate.
        
        Args:
            actr_type: protobuf type object
        
        Returns:
            Actor id protobuf object
        """
        ...
    
    async def call(
        self,
        target: Dest,
        route_key: str,
        request: Any,
        timeout_ms: int = 30000,
        payload_type: Optional[PayloadType] = None,
    ) -> bytes:
        """
        Execute request/response RPC call.
        
        Args:
            target: Dest wrapper object
            route_key: Route key string
            request: Request protobuf object (not bytes)
            timeout_ms: Timeout in milliseconds
            payload_type: Payload transmission type
        
        Returns:
            Response bytes
        """
        ...
    
    async def tell(
        self,
        target: Dest,
        route_key: str,
        message: Any,
        payload_type: Optional[PayloadType] = None,
    ) -> None:
        """
        Execute fire-and-forget message RPC call.
        
        Args:
            target: Dest wrapper object
            route_key: Route key string
            message: Message protobuf object (not bytes)
            payload_type: Payload transmission type
        """
        ...
    
    async def register_stream(
        self,
        stream_id: str,
        callback: Callable[[Any, Any], Coroutine[Any, Any, None]],
    ) -> None:
        """
        Register DataStream callback.
        
        Args:
            stream_id: Stream identifier
            callback: Async callback function(data_stream, sender_id)
        """
        ...
    
    async def unregister_stream(self, stream_id: str) -> None:
        """
        Unregister DataStream.
        
        Args:
            stream_id: Stream identifier
        """
        ...
    
    async def send_stream(
        self,
        target: Dest,
        data_stream: Any,
        payload_type: Optional[PayloadType] = None,
    ) -> None:
        """
        Send DataStream.
        
        Args:
            target: Dest wrapper object
            data_stream: DataStream protobuf object or wrapper
            payload_type: Payload transmission type
        """
        ...

# Decorator namespace class
class ActrDecorators:
    """Decorator namespace for @actr.service and @actr.rpc."""
    
    def service(self, service_name: str) -> Callable[[type], type]:
        """
        Decorator to mark a class as an Actr service.
        
        Args:
            service_name: Full service name (e.g., "my_package.MyService")
        
        Returns:
            Class decorator
        """
        ...
    
    def rpc(
        self, 
        func: Optional[Callable[..., Any]] = None,
        *,
        route_key: Optional[str] = None,
    ) -> Callable[..., Any]:
        """
        Decorator to mark a method as an RPC handler.
        
        Args:
            func: The function to decorate (when used without parentheses)
            route_key: Optional custom route key (default: "{service_name}.{method_name}")
        
        Returns:
            Method decorator
        """
        ...

# Decorator instance
actr: ActrDecorators

# Decorator functions (direct access)
def service(service_name: str) -> Callable[[type], type]:
    """Decorator to mark a class as an Actr service."""
    ...

def rpc(
    func: Optional[Callable[..., Any]] = None,
    *,
    route_key: Optional[str] = None,
) -> Callable[..., Any]:
    """Decorator to mark a method as an RPC handler."""
    ...

# Submodule
from . import actr_raw as actr_raw
from . import decorators as decorators

__all__ = [
    "__version__",
    # High-level Pythonic API
    "ActrSystem",
    "ActrNode",
    "ActrRef",
    "Context",
    "Dest",
    "PayloadType",
    "DataStream",
    # Decorators
    "actr",
    "service",
    "rpc",
    # Exceptions
    "ActrRuntimeError",
    "ActrTransportError",
    "ActrDecodeError",
    "ActrUnknownRoute",
    "ActrGateNotInitialized",
    # Submodules
    "actr_raw",
    "decorators",
]
