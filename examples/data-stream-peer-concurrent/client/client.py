from __future__ import annotations

import argparse
import asyncio
import logging
import sys
from pathlib import Path

from actr import (
    ActrSystem,
    ActrRef,
    Dest,
    ActrRuntimeError,
    WorkloadBase,
    ActrType,
)

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "generated"))
sys.path.insert(0, str(ROOT))

logging.basicConfig(
    level=logging.INFO,
    format="[%(levelname)s] %(message)s",
)
logger = logging.getLogger(__name__)

from generated import data_stream_peer_pb2 as pb2
from generated import stream_client_actor as client_actor
from actr import DataStream,Context


class StreamClientService(client_actor.StreamClientHandler):
    """StreamClient service implementation (custom workload)"""

    def __init__(self) -> None:
        self.target_actr_type = ActrType(
            "acme",
            "DataStreamPeerConcurrentServer",
        )
        # Per-stream task spawner: 每个 stream_id 一个 Queue，保证同一 stream 串行，不同 stream 并发
        self.stream_queues: dict[str, asyncio.Queue] = {}
        logger.info("StreamClientService initialized")

    async def prepare_client_stream(
        self, req: pb2.PrepareClientStreamRequest, ctx
    ) -> pb2.PrepareStreamResponse:
        logger.info(
            "prepare_client_stream: stream_id=%s, expected_count=%s",
            req.stream_id,
            req.expected_count,
        )

        stream_id = req.stream_id
        expected_count = req.expected_count

        # 为这个 stream_id 创建 Queue 和专属处理任务（如果还没有）
        if stream_id not in self.stream_queues:
            queue = asyncio.Queue()
            self.stream_queues[stream_id] = queue

            # 启动专属任务来串行处理这个 stream 的消息
            async def stream_task():
                logger.info("🚀 Started dedicated task for stream: %s", stream_id)
                
                while True:
                    try:
                        stream, sender_id = await queue.get()
                        text = stream.payload().decode("utf-8", errors="replace")
                        logger.info(
                            "client received %s/%s from %s (stream=%s): %s",
                            stream.sequence(),
                            expected_count,
                            sender_id,
                            stream_id,
                            text,
                        )
                        queue.task_done()
                    except asyncio.CancelledError:
                        logger.info("🛑 Stream task cancelled: %s", stream_id)
                        break
                    except Exception as e:
                        logger.error("Error processing stream %s: %s", stream_id, e)

            asyncio.create_task(stream_task())

        # 注册 stream callback：快速返回，消息发送到 Queue
        async def stream_callback(stream: DataStream, sender_id) -> None:
            # 发送到该 stream 的专属 Queue（非阻塞）
            if stream_id in self.stream_queues:
                await self.stream_queues[stream_id].put((stream, sender_id))
            else:
                logger.warning("No queue found for stream: %s", stream_id)

        await ctx.register_stream(stream_id, stream_callback)

        return pb2.PrepareStreamResponse(
            ready=True,
            message=f"client ready to receive {expected_count} messages on {stream_id}",
        )

    async def start_stream(
        self, req: pb2.StartStreamRequest, ctx:Context
    ) -> pb2.StartStreamResponse:
        logger.info(
            "start_stream: client_id=%s, stream_id=%s, message_count=%s",
            req.client_id,
            req.stream_id,
            req.message_count,
        )

        logger.info("discovering server type: %s", self.target_actr_type)
        try:
            server_id = await ctx.discover(self.target_actr_type)
            logger.info("discovered server: %s", server_id)
        except ActrRuntimeError as e:
            logger.error("Failed to discover server: %s", e)
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
            logger.error("Failed to call PrepareStream: %s", e)
            return pb2.StartStreamResponse(
                accepted=False,
                message=f"Failed to prepare stream: {e}",
            )

        async def _send_stream_messages() -> None:
            logger.info(
                "Starting to send %s DataStream messages to server",
                req.message_count,
            )
            for i in range(1, req.message_count + 1):
                message = f"[client {req.client_id}] message {i}"
                data_stream = DataStream(
                    stream_id=req.stream_id,
                    sequence=i,
                    payload=message.encode("utf-8"),
                )
                target = Dest.actor(server_id)

                try:
                    await ctx.send_stream(target, data_stream)
                    logger.info("client sending %s/%s: %s", i, req.message_count, message)
                except Exception as e:
                    logger.warning("client send_data_stream failed: %s", e)
                    break

                if i < req.message_count:
                    await asyncio.sleep(1.0)

        asyncio.create_task(_send_stream_messages())

        return pb2.StartStreamResponse(
            accepted=True,
            message=f"client sending {req.message_count} messages",
        )


class StreamClientWorkload(WorkloadBase):
    def __init__(self, handler: StreamClientService):
        self.handler = handler
        super().__init__(client_actor.StreamClientDispatcher())

    async def on_start(self, ctx) -> None:
        logger.info("StreamClientWorkload on_start")

    async def on_stop(self, ctx) -> None:
        logger.info("StreamClientWorkload on_stop")


async def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--actr-toml", required=True)
    ap.add_argument("client_id")
    ap.add_argument("message_count", type=int)
    args = ap.parse_args()
    logger.info("Starting client with args: %s", args)
    logger.info("Loading ActrSystem from TOML...")

    system = await ActrSystem.from_toml(args.actr_toml)
    logger.info("ActrSystem loaded, creating workload...")

    workload = StreamClientWorkload(StreamClientService())
    logger.info("Workload created, attaching to system...")

    node = system.attach(workload)
    logger.info("ActrNode attached, starting...")

    ref = await node.start()
    logger.info("✅ Python Client started! Actor ID: %s", ref.actor_id())

    start_req = pb2.StartStreamRequest(
        client_id=args.client_id,
        stream_id=f"stream-{args.client_id}",
        message_count=args.message_count,
    )

    response_bytes = await ref.call(
        "data_stream_peer.StreamClient.StartStream",
        start_req,
    )
    response = pb2.StartStreamResponse.FromString(response_bytes)
    logger.info("StartStream response: %s", response)

    await ref.wait_for_ctrl_c_and_shutdown()
    logger.info("Client shutting down...")
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
