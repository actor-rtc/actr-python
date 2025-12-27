from __future__ import annotations

import argparse
import asyncio
import logging
import sys
from pathlib import Path

from actr import ActrSystem, Dest, WorkloadBase

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "generated"))
sys.path.insert(0, str(ROOT))

logging.basicConfig(
    level=logging.INFO,
    format="[%(levelname)s] %(message)s",
)
logger = logging.getLogger(__name__)

from generated import data_stream_peer_pb2 as pb2
from generated import stream_server_actor as server_actor
from actr import DataStream as StreamData, Context 


class StreamServerService(server_actor.StreamServerHandler):
    """StreamServer service implementation (custom workload)"""

    def __init__(self) -> None:
        self.received_count = {"count": 0}
        logger.info("StreamServerService initialized")

    async def prepare_stream(
        self, req: pb2.PrepareServerStreamRequest, ctx:Context
    ) -> pb2.PrepareStreamResponse:
        caller = ctx.caller_id()
        if caller is None:
            raise RuntimeError("No caller_id in ctx")

        logger.info(
            "prepare_stream: stream_id=%s, expected_count=%s, caller=%s",
            req.stream_id,
            req.expected_count,
            caller,
        )

        self.received_count["count"] = 0
        stream_id = req.stream_id
        expected_count = req.expected_count

        async def stream_callback(stream: StreamData, sender_id) -> None:
            self.received_count["count"] += 1
            text = stream.payload().decode("utf-8", errors="replace")
            logger.info(
                "server: stream %s received %s/%s from %s: %s",
                stream.stream_id(),
                self.received_count["count"],
                expected_count,
                sender_id,
                text,
            )

        await ctx.register_stream(stream_id, stream_callback)

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
            logger.error("Failed to call PrepareClientStream: %s", e)
            return pb2.PrepareStreamResponse(
                ready=False,
                message=f"Failed to prepare client stream: {e}",
            )

        async def _send_stream_messages() -> None:
            logger.info("sending data stream back to client: %s", caller)
            for i in range(1, expected_count + 1):
                message = f"[server] message {i}"
                data_stream = StreamData(
                    stream_id=stream_id,
                    sequence=i,
                    payload=message.encode("utf-8"),
                )
                target = Dest.actor(caller)

                try:
                    await ctx.send_stream(target, data_stream)
                    logger.info("server sending %s/%s: %s", i, expected_count, message)
                except Exception as e:
                    logger.warning("server send_data_stream failed: %s", e)
                    break

                if i < expected_count:
                    await asyncio.sleep(1.0)

        asyncio.create_task(_send_stream_messages())

        return pb2.PrepareStreamResponse(
            ready=True,
            message=f"registered stream {stream_id} for {expected_count} messages",
        )


class StreamServerWorkload(WorkloadBase):
    def __init__(self, handler: StreamServerService):
        self.handler = handler
        super().__init__(server_actor.StreamServerDispatcher())

    async def on_start(self, ctx:Context) -> None:
        logger.info("StreamServerWorkload on_start")

    async def on_stop(self, ctx:Context) -> None:
        logger.info("StreamServerWorkload on_stop")


async def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--actr-toml", required=True)
    args = ap.parse_args()
    logger.info("Starting server with args: %s", args)
    # Load ActrSystem from TOML
    system = await ActrSystem.from_toml(args.actr_toml)
    # Create StreamServerWorkload
    workload = StreamServerWorkload(StreamServerService())
    # Attach workload to ActrSystem
    node = system.attach(workload)
    # Start ActrNode
    ref = await node.start()
    logger.info("Python Server started! Actor ID: %s", ref.actor_id())

    await ref.wait_for_ctrl_c_and_shutdown()
    logger.info("Server shutting down...")
    return 0

if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
