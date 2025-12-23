from __future__ import annotations

import argparse
import asyncio
import logging
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "generated"))

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="[%(levelname)s] %(message)s",
)
logger = logging.getLogger(__name__)

from actr_sdk import actr, ActrSystem, ActrRef, Context, Dest, DataStream, ActrRuntimeError  # type: ignore

from generated import package_pb2, actr_pb2
from generated import data_stream_peer_pb2 as pb2


@actr.service("data_stream_peer.StreamClient")
class StreamClientService:
    """StreamClient service implementation"""
    
    def __init__(self) -> None:
        self.server_type = actr_pb2.ActrType(
            manufacturer="acme",
            name="DataStreamPeerConcurrentServer",
        )
        logger.info("StreamClientService initialized")

    @actr.rpc(route_key="data_stream_peer.StreamClient.PrepareClientStream")
    async def prepare_client_stream(self, req: pb2.PrepareClientStreamRequest, ctx) -> pb2.PrepareStreamResponse:
        """Handle PrepareClientStream RPC request"""
        # Convert Rust binding context to high-level Context wrapper if needed
        if not isinstance(ctx, Context):
            ctx = Context(ctx)
        
        logger.info(
            f"prepare_client_stream: stream_id={req.stream_id}, expected_count={req.expected_count}"
        )

        stream_id = req.stream_id
        expected_count = req.expected_count

        # Register stream callback to receive DataStream messages from server
        async def stream_callback(data_stream: package_pb2.DataStream, sender_id) -> None:
            text = data_stream.payload.decode("utf-8", errors="replace")
            logger.info(
                f"client received {data_stream.sequence}/{expected_count} from {sender_id}: {text}"
            )

        await ctx.register_stream(stream_id, stream_callback)

        return pb2.PrepareStreamResponse(
            ready=True,
            message=f"client ready to receive {expected_count} messages on {stream_id}",
        )

    @actr.rpc(route_key="data_stream_peer.StreamClient.StartStream")
    async def start_stream(self, req: pb2.StartStreamRequest, ctx) -> pb2.StartStreamResponse:
        """Handle StartStream RPC request"""
        # Convert Rust binding context to high-level Context wrapper if needed
        if not isinstance(ctx, Context):
            ctx = Context(ctx)
        
        logger.info(
            f"start_stream: client_id={req.client_id}, stream_id={req.stream_id}, message_count={req.message_count}"
        )

        logger.info(f"discovering server type: {self.server_type}")
        try:
            server_id = await ctx.discover(self.server_type)
            logger.info(f"discovered server: {server_id}")
        except ActrRuntimeError as e:
            logger.error(f"Failed to discover server: {e}")
            return pb2.StartStreamResponse(
                accepted=False,
                message=f"Failed to discover server: {e}",
            )

        prepare_req = pb2.PrepareServerStreamRequest(
            stream_id=req.stream_id,
            expected_count=req.message_count,
        )

        try:
            response_bytes = await ctx.call(
                Dest.actor(server_id),
                "data_stream_peer.StreamServer.PrepareStream",
                prepare_req,
            )
            prepare_resp = pb2.PrepareStreamResponse.FromString(response_bytes)
            
            if not prepare_resp.ready:
                return pb2.StartStreamResponse(
                    accepted=False,
                    message=prepare_resp.message,
                )
        except Exception as e:
            logger.error(f"Failed to call PrepareStream: {e}")
            return pb2.StartStreamResponse(
                accepted=False,
                message=f"Failed to prepare stream: {e}",
            )

        # Send DataStream messages to server in background task
        # This prevents the RPC from timing out or being cancelled
        async def _send_stream_messages() -> None:
            logger.info(f"Starting to send {req.message_count} DataStream messages to server")
            for i in range(1, req.message_count + 1):
                message = f"[client {req.client_id}] message {i}"
                data_stream_pb = package_pb2.DataStream(
                    stream_id=req.stream_id,
                    sequence=i,
                    payload=message.encode("utf-8"),
                )
                data_stream_wrapper = DataStream(data_stream_pb)
                target = Dest.actor(server_id)
                
                logger.info(f"client sending {i}/{req.message_count}: {message}")
                try:
                    await ctx.send_stream(target, data_stream_wrapper)
                except Exception as e:
                    logger.error(f"client send_data_stream failed: {e}")
                    return
                
                if i < req.message_count:
                    await asyncio.sleep(1.0)
            logger.info(f"Completed sending {req.message_count} DataStream messages to server")

        # Start background task to send messages
        asyncio.create_task(_send_stream_messages())

        return pb2.StartStreamResponse(
            accepted=True,
            message=f"accepted, will send {req.message_count} messages to {server_id}",
        )


async def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--actr-toml", required=True)
    ap.add_argument("client_id", nargs="?", default="client-1")
    ap.add_argument("message_count", type=int, nargs="?", default=5)
    args = ap.parse_args()

    logger.info(f"Starting client: client_id={args.client_id}, message_count={args.message_count}")
    logger.info("Loading ActrSystem from TOML...")
    
    # Use ActrSystem (high-level API)
    system = await ActrSystem.from_toml(args.actr_toml)
    logger.info("ActrSystem loaded, creating workload...")
    
    # Use decorator-generated create_workload() method
    workload = StreamClientService.create_workload()
    logger.info("Workload created, attaching to system...")
    
    node = system.attach(workload)
    logger.info("ActrNode attached, starting...")
    
    # Use ActrNode.start() - automatically handles exceptions
    ref = await node.start()
    logger.info(f"✅ Client started! Actor ID: {ref.actor_id()}")
    
    # Call StartStream RPC on the local workload
    stream_id = f"{args.client_id}-stream"
    start_req = pb2.StartStreamRequest(
        client_id=args.client_id,
        stream_id=stream_id,
        message_count=args.message_count,
    )
    
    logger.info(f"Sending StartStreamRequest: client_id={args.client_id}, stream_id={stream_id}, message_count={args.message_count}")
    try:
        response_bytes = await ref.call(
            "data_stream_peer.StreamClient.StartStream",
            start_req,
        )
        response = pb2.StartStreamResponse.FromString(response_bytes)
        logger.info(f"StartStream response: accepted={response.accepted}, message={response.message}")
        
        if not response.accepted:
            logger.error(f"StartStream was not accepted: {response.message}")
            return 1
    except ActrRuntimeError as e:
        logger.error(f"Failed to call StartStream: {e}")
        return 1
    
    # Wait a bit for stream messages to be received
    logger.info("Waiting for stream messages...")
    await asyncio.sleep(args.message_count + 2)
    
    logger.info("Client completed, shutting down...")
    ref.shutdown()
    await ref.wait_for_shutdown()
    
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))

