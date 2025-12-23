from __future__ import annotations

import argparse
import asyncio
import logging
import sys
from pathlib import Path

# Make generated/ importable
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "generated"))

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="[%(levelname)s] %(message)s",
)
logger = logging.getLogger(__name__)

from actr_sdk import actr, ActrSystem, Context, Dest, DataStream  # type: ignore

from generated import package_pb2, actr_pb2
from generated import data_stream_peer_pb2 as pb2


@actr.service("data_stream_peer.StreamServer")
class StreamServerService:
    """StreamServer service implementation"""
    
    def __init__(self) -> None:
        self.received_count = {"count": 0}  # Use dict to allow modification in nested function
        logger.info("StreamServerService initialized")

    @actr.rpc(route_key="data_stream_peer.StreamServer.PrepareStream")
    async def prepare_stream(self, req: pb2.PrepareServerStreamRequest, ctx) -> pb2.PrepareStreamResponse:
        """Handle PrepareStream RPC request"""
        # Convert Rust binding context to high-level Context wrapper if needed
        if not isinstance(ctx, Context):
            ctx = Context(ctx)
        
        caller = ctx.caller_id()
        if caller is None:
            raise RuntimeError("No caller_id in ctx")

        logger.info(
            f"prepare_stream: stream_id={req.stream_id}, expected_count={req.expected_count}, caller={caller}"
        )

        self.received_count["count"] = 0
        stream_id = req.stream_id
        expected_count = req.expected_count

        # Register stream callback to receive DataStream messages from client
        async def stream_callback(data_stream: package_pb2.DataStream, sender_id) -> None:
            self.received_count["count"] += 1
            text = data_stream.payload.decode("utf-8", errors="replace")
            logger.info(
                f"server: stream {data_stream.stream_id} received {self.received_count['count']}/{expected_count} "
                f"from {sender_id}: {text}"
            )

        await ctx.register_stream(stream_id, stream_callback)

        # Call client's PrepareClientStream
        prepare_client_req = pb2.PrepareClientStreamRequest(
            stream_id=stream_id,
            expected_count=expected_count,
        )

        try:
            response_bytes = await ctx.call(
                Dest.actor(caller),
                "data_stream_peer.StreamClient.PrepareClientStream",
                prepare_client_req,
            )
            prepare_client_resp = pb2.PrepareStreamResponse.FromString(response_bytes)
            
            if not prepare_client_resp.ready:
                return pb2.PrepareStreamResponse(
                    ready=False,
                    message=prepare_client_resp.message,
                )
        except Exception as e:
            logger.error(f"Failed to call PrepareClientStream: {e}")
            return pb2.PrepareStreamResponse(
                ready=False,
                message=f"Failed to prepare client stream: {e}",
            )

        # Start background task to send DataStream messages back to client
        async def _send_stream_messages() -> None:
            logger.info(f"sending data stream back to client: {caller}")
            for i in range(1, expected_count + 1):
                message = f"[server] message {i}"
                data_stream_pb = package_pb2.DataStream(
                    stream_id=stream_id,
                    sequence=i,
                    payload=message.encode("utf-8"),
                )
                data_stream_wrapper = DataStream(data_stream_pb)
                target = Dest.actor(caller)
                
                try:
                    await ctx.send_stream(target, data_stream_wrapper)
                    logger.info(f"server sending {i}/{expected_count}: {message}")
                except Exception as e:
                    logger.warn(f"server send_data_stream failed: {e}")
                    break
                
                if i < expected_count:
                    await asyncio.sleep(1.0)

        asyncio.create_task(_send_stream_messages())

        return pb2.PrepareStreamResponse(
            ready=True,
            message=f"registered stream {stream_id} for {expected_count} messages",
        )


async def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--actr-toml", required=True)
    args = ap.parse_args()
    logger.info(f"Starting server with args: {args}")
    logger.info("Loading ActrSystem from TOML...")
    
    # Use ActrSystem (high-level API)
    system = await ActrSystem.from_toml(args.actr_toml)
    logger.info("ActrSystem loaded, creating workload...")
    
    # Use decorator-generated create_workload() method
    workload = StreamServerService.create_workload()
    logger.info("Workload created, attaching to system...")
    
    node = system.attach(workload)
    logger.info("ActrNode attached, starting...")
    
    # Use ActrNode.start() - automatically handles exceptions
    ref = await node.start()
    logger.info(f"✅ Python Server started! Actor ID: {ref.actor_id()}")
    
    await ref.wait_for_ctrl_c_and_shutdown()
    logger.info("Server shutting down...")
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))

